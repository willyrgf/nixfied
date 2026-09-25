//! The session owner records execution outcome; workload transitions cannot.
use super::sql_error;
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

/// The session owner's settlement writer. Complete finalization requires a
/// known execution outcome and is claimed only after the caller settled every
/// process, capture, and retention obligation. Repeating it is harmless.
pub fn record_finalization_complete(
    registry: &mut Registry,
    run_id: &str,
    manifest_hash: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    match progress(&transaction, identity, run_id, manifest_hash)? {
        SessionProgress::Executing => {
            return Err(invalid(
                "finalization cannot complete before the execution outcome is known",
            ));
        }
        SessionProgress::Finalized(_) => return Ok(()),
        SessionProgress::Finalizing(_) => {}
    }
    write_complete(&transaction, run_id)?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "run.finalized",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: None,
            computed_manifest_hash: Some(manifest_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Attempt to record that the live owner could not settle its obligations.
/// Finalization stays unfinished; a successor must recover or refuse.
pub fn record_finalization_unfinished(
    registry: &mut Registry,
    run_id: &str,
    manifest_hash: &str,
    reason: &RuntimeError,
) -> RuntimeResult<()> {
    let payload = serde_json::json!({"code": reason.code, "message": reason.message}).to_string();
    let mut event = EventInsert::new("run.finalization-unfinished", &payload);
    event.run_id = Some(run_id);
    event.computed_manifest_hash = Some(manifest_hash);
    registry.append_event(event).map(|_| ())
}

/// Called by the exclusive successor only after it settled predecessor process,
/// deletion, and retention obligations. Known outcomes are preserved; the
/// successor completes finalization without adopting any predecessor work.
pub fn record_recovered_sessions(registry: &mut Registry) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT run_id, computed_manifest_hash, execution_outcome, finalization
                 FROM runs WHERE finalization = 'unfinished' ORDER BY run_id",
            )
            .map_err(sql_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    for (run_id, hash, outcome, finalization) in rows {
        if decode(outcome.as_deref(), &finalization)? == SessionProgress::Executing {
            return Err(invalid(
                "recovery cannot settle a session whose interruption was not recorded",
            ));
        }
        write_complete(&transaction, &run_id)?;
        insert_event(
            &transaction,
            identity,
            redactor,
            EventInsert {
                event_type: "run.recovered",
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

/// Whether the owner could publish a checked output seal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPublication {
    Sealed,
    /// Some source lacks a checked capture outcome; completeness stays unknown.
    Unsealed,
}

/// Disable new source admission: after this commit no process may register
/// a source for the run. Repeating it is harmless.
pub fn close_source_registration(
    registry: &mut Registry,
    run_id: &str,
    manifest_hash: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    progress(&transaction, identity, run_id, manifest_hash)?;
    let changed = transaction
        .execute(
            "UPDATE runs SET sources = 'closed' WHERE run_id = ?1 AND sources = 'open'",
            params![run_id],
        )
        .map_err(sql_error)?;
    if changed == 1 {
        insert_event(
            &transaction,
            identity,
            redactor,
            EventInsert {
                event_type: "run.sources-closed",
                run_id: Some(run_id),
                service_instance_id: None,
                process_key: None,
                computed_manifest_hash: Some(manifest_hash),
                payload_json: "{}",
            },
        )?;
    }
    transaction.commit().map_err(sql_error)
}

/// Publish the run's output seal only when registration is closed and every
/// source recorded a checked capture outcome. The caller has already closed
/// its diagnostic writer. Sealed files are immutable afterwards.
pub fn seal_output(
    registry: &mut Registry,
    run_id: &str,
    manifest_hash: &str,
) -> RuntimeResult<OutputPublication> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    progress(&transaction, identity, run_id, manifest_hash)?;
    let (sources, pending): (String, i64) = transaction
        .query_row(
            "SELECT sources, (SELECT count(*) FROM processes WHERE run_id = ?1 AND capture = 'pending')
             FROM runs WHERE run_id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql_error)?;
    if sources != "closed" {
        return Err(invalid(
            "output cannot be sealed while source registration is open",
        ));
    }
    if pending > 0 {
        transaction.commit().map_err(sql_error)?;
        return Ok(OutputPublication::Unsealed);
    }
    // A repeated seal is harmless and records no second event.
    let changed = transaction
        .execute(
            "UPDATE runs SET output = 'sealed' WHERE run_id = ?1 AND output = 'unsealed'",
            params![run_id],
        )
        .map_err(sql_error)?;
    if changed == 0 {
        transaction.commit().map_err(sql_error)?;
        return Ok(OutputPublication::Sealed);
    }
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "run.output-sealed",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: None,
            computed_manifest_hash: Some(manifest_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(OutputPublication::Sealed)
}

fn write_complete(transaction: &Transaction<'_>, run_id: &str) -> RuntimeResult<()> {
    let changed = transaction
        .execute(
            "UPDATE runs SET finalization = 'complete'
             WHERE run_id = ?1 AND execution_outcome IS NOT NULL AND finalization = 'unfinished'",
            params![run_id],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(invalid(
            "finalization update did not affect exactly one settled session",
        ));
    }
    Ok(())
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
