//! The session owner records execution outcome; workload transitions cannot.
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::events::{EventInsert, insert_event};
use super::sqlite::RegistryContext;
use super::status::{DbStatus, FinalizationStatus};
use super::{Registry, RegistryIdentity};
use crate::{ErrorCode, RuntimeError, RuntimeResult};

pub use super::status::ExecutionOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProgress {
    Executing,
    Finalizing(ExecutionOutcome),
    Finalized(ExecutionOutcome),
}

fn decode(outcome: Option<&str>, finalization: &str) -> RuntimeResult<SessionProgress> {
    let outcome = outcome.map(ExecutionOutcome::parse_db).transpose()?;
    match (outcome, FinalizationStatus::parse_db(finalization)?) {
        (None, FinalizationStatus::Unfinished) => Ok(SessionProgress::Executing),
        (Some(outcome), FinalizationStatus::Unfinished) => Ok(SessionProgress::Finalizing(outcome)),
        (Some(outcome), FinalizationStatus::Complete) => Ok(SessionProgress::Finalized(outcome)),
        (None, FinalizationStatus::Complete) => {
            Err(invalid("finalized session has no execution outcome"))
        }
    }
}

fn progress(
    connection: &Connection,
    identity: &RegistryIdentity,
    run_id: &str,
    manifest_hash: &str,
) -> RuntimeResult<SessionProgress> {
    let row: Option<(Option<String>, String)> = connection
        .query_row(
            "SELECT execution_outcome, finalization FROM runs
         WHERE run_id = ?1 AND environment = ?2 AND slot = ?3 AND computed_manifest_hash = ?4",
            params![run_id, identity.environment, identity.slot, manifest_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    let (outcome, finalization) =
        row.ok_or_else(|| invalid("session provenance does not match its owner"))?;
    decode(outcome.as_deref(), &finalization)
}

/// Commit the execution result before presentation or resource finalization.
/// Repeating the same result is harmless; replacing a known result is refused.
pub fn record_execution_outcome(
    registry: &mut Registry,
    run_id: &str,
    manifest_hash: &str,
    outcome: ExecutionOutcome,
) -> RuntimeResult<()> {
    if outcome == ExecutionOutcome::Interrupted {
        return Err(invalid(
            "only predecessor recovery may record interrupted execution",
        ));
    }
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    match progress(&transaction, identity, run_id, manifest_hash)? {
        SessionProgress::Executing => {}
        SessionProgress::Finalizing(stored) | SessionProgress::Finalized(stored) => {
            return if stored == outcome {
                Ok(())
            } else {
                Err(invalid("session execution outcome is immutable"))
            };
        }
    }
    write_unknown_outcome(&transaction, run_id, outcome)?;
    let payload = serde_json::json!({"outcome": outcome.as_str()}).to_string();
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "run.execution-settled",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: None,
            computed_manifest_hash: Some(manifest_hash),
            payload_json: &payload,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Called by the exclusive successor before beginning its own session. Validate
/// the complete stored set first; preserve every known predecessor outcome.
pub fn record_interrupted_sessions(registry: &mut Registry) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT run_id, computed_manifest_hash, environment, slot, execution_outcome, finalization
             FROM runs ORDER BY run_id",
        ).map_err(sql_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    let mut interrupted = Vec::new();
    for (run_id, hash, environment, slot, outcome, finalization) in rows {
        if environment != identity.environment || slot != identity.slot {
            return Err(invalid("session belongs to another slot"));
        }
        if decode(outcome.as_deref(), &finalization)? == SessionProgress::Executing {
            interrupted.push((run_id, hash));
        }
    }
    for (run_id, hash) in interrupted {
        write_unknown_outcome(&transaction, &run_id, ExecutionOutcome::Interrupted)?;
        insert_event(
            &transaction,
            identity,
            redactor,
            EventInsert {
                event_type: "run.interrupted",
                run_id: Some(&run_id),
                service_instance_id: None,
                process_key: None,
                computed_manifest_hash: Some(&hash),
                payload_json: "{}",
            },
        )?;
    }
    transaction.commit().map_err(sql_error)
}

fn write_unknown_outcome(
    transaction: &Transaction<'_>,
    run_id: &str,
    outcome: ExecutionOutcome,
) -> RuntimeResult<()> {
    let changed = transaction
        .execute(
            "UPDATE runs SET execution_outcome = ?2
         WHERE run_id = ?1 AND execution_outcome IS NULL AND finalization = 'unfinished'",
            params![run_id, outcome.as_str()],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(invalid(
            "execution outcome update did not affect exactly one pending session",
        ));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, message)
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    invalid(error.to_string())
}
