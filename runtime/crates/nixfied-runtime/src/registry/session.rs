//! The session owner records execution outcome; workload transitions cannot.
use super::sql_error;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::Registry;
use super::events::{EventInsert, insert_event};
use super::sqlite::RegistryContext;
use super::status::{DbStatus, FinalizationStatus};
use crate::admission::RunAdmission;
use crate::admission::source::RecordedSource;
use crate::output::utf8_locator;
use crate::state::HostPlacement;
use crate::{ErrorCode, RuntimeError, RuntimeResult};

pub use super::status::ExecutionOutcome;

/// A declared request borrows actual run admission. Stored metadata cannot
/// construct it or stand in for its source, secret, closure or host checks.
pub struct DeclaredRunRequest<'a> {
    admission: &'a RunAdmission,
    recorded: RecordedRunRequest,
}

impl<'a> DeclaredRunRequest<'a> {
    pub fn admission(&self) -> &'a RunAdmission {
        self.admission
    }
    pub fn root_task(&self) -> &nixfied_manifest::TaskId {
        self.recorded.root_task()
    }
    pub fn operation_timeout_ms(&self) -> u64 {
        self.recorded.operation_timeout_ms()
    }
    pub fn new(
        admission: &'a RunAdmission,
        root_task: &nixfied_manifest::TaskId,
        operation_timeout_ms: u64,
    ) -> RuntimeResult<Self> {
        let (root_task, _) = admission
            .common()
            .execution_manifest()
            .tasks()
            .get_key_value(root_task)
            .ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::TaskSelectionInvalid,
                    "selected root task is not declared",
                )
            })?;
        Ok(Self {
            admission,
            recorded: RecordedRunRequest {
                computed_manifest_hash: admission.common().computed_manifest_hash().to_owned(),
                root_task: root_task.clone(),
                source: RecordedSource::from_admitted(admission.source())?,
                operation_timeout_ms,
            },
        })
    }

    pub fn recorded(&self) -> &RecordedRunRequest {
        &self.recorded
    }

    pub fn preflight(&self, placement: &HostPlacement) -> RuntimeResult<()> {
        utf8_locator(self.admission.common().manifest_path(), "manifestPath")?;
        utf8_locator(placement.state_base(), "stateBase")?;
        for (field, path) in [
            ("stateRoot", placement.state_root()),
            ("registryDir", placement.registry_dir()),
            ("registryPath", placement.registry_path()),
            ("runDir", placement.run_dir()),
            ("logsDir", placement.logs_dir()),
            ("summaryPath", placement.summary_path()),
        ] {
            utf8_locator(&path, field)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedRunRequest {
    computed_manifest_hash: String,
    root_task: nixfied_manifest::TaskId,
    source: RecordedSource,
    operation_timeout_ms: u64,
}

/// Strict transport data for comparison, never an admission proof.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordedRunRequestWire {
    computed_manifest_hash: String,
    root_task: String,
    source: crate::admission::source::RecordedSourceWire,
    operation_timeout_ms: u64,
}

impl RecordedRunRequestWire {
    pub fn from_recorded(request: &RecordedRunRequest) -> Self {
        Self {
            computed_manifest_hash: request.computed_manifest_hash.clone(),
            root_task: request.root_task.to_string(),
            source: crate::admission::source::RecordedSourceWire::from_recorded(&request.source),
            operation_timeout_ms: request.operation_timeout_ms,
        }
    }
    pub fn checked(&self) -> RuntimeResult<RecordedRunRequest> {
        let root_task =
            RecordedRunRequest::checked_root(&self.computed_manifest_hash, &self.root_task)?;
        Ok(RecordedRunRequest {
            computed_manifest_hash: self.computed_manifest_hash.clone(),
            root_task,
            source: self.source.clone().check()?,
            operation_timeout_ms: self.operation_timeout_ms,
        })
    }
}

impl RecordedRunRequest {
    pub fn decode(
        hash: &str,
        root_task: &str,
        source_json: &str,
        timeout: &str,
    ) -> RuntimeResult<Self> {
        let root_task = Self::checked_root(hash, root_task)?;
        let operation_timeout_ms = timeout
            .parse::<u64>()
            .ok()
            .filter(|value| value.to_string() == timeout)
            .ok_or_else(|| invalid("invalid recorded operation_timeout_ms"))?;
        Ok(Self {
            computed_manifest_hash: hash.to_owned(),
            root_task,
            source: RecordedSource::decode(source_json)?,
            operation_timeout_ms,
        })
    }

    fn checked_root(hash: &str, root_task: &str) -> RuntimeResult<nixfied_manifest::TaskId> {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("invalid recorded computed_manifest_hash"));
        }
        if !nixfied_manifest::validation::is_step_safe_id(root_task) {
            return Err(invalid("invalid recorded root_task"));
        }
        Ok(nixfied_manifest::TaskId::new(root_task))
    }

    pub fn computed_manifest_hash(&self) -> &str {
        &self.computed_manifest_hash
    }
    pub fn root_task(&self) -> &nixfied_manifest::TaskId {
        &self.root_task
    }
    pub fn source(&self) -> &RecordedSource {
        &self.source
    }
    pub fn operation_timeout_ms(&self) -> u64 {
        self.operation_timeout_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", try_from = "OwnerIdentityWire")]
pub(crate) struct OwnerIdentity {
    pid: std::num::NonZeroU32,
    platform_start: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerIdentityWire {
    pid: std::num::NonZeroU32,
    platform_start: String,
}

impl TryFrom<OwnerIdentityWire> for OwnerIdentity {
    type Error = &'static str;
    fn try_from(value: OwnerIdentityWire) -> Result<Self, Self::Error> {
        if value.pid.get() > i32::MAX as u32 || value.platform_start.is_empty() {
            return Err("invalid owner identity fields");
        }
        Ok(Self {
            pid: value.pid,
            platform_start: value.platform_start,
        })
    }
}

impl OwnerIdentity {
    pub(crate) fn pid(&self) -> u32 {
        self.pid.get()
    }
    pub(crate) fn start_identity(&self) -> &str {
        &self.platform_start
    }
    fn current() -> RuntimeResult<Self> {
        let pid = std::process::id();
        match crate::service::sample_leader(pid)? {
            crate::service::LeaderSample::Present {
                start_identity,
                zombie: false,
                ..
            } => Self::try_from(OwnerIdentityWire {
                pid: pid.try_into().map_err(|_| invalid("invalid self PID"))?,
                platform_start: start_identity,
            })
            .map_err(invalid),
            _ => Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                "cannot establish exact live self identity",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProgress {
    Executing,
    Finalizing(ExecutionOutcome),
    Finalized(ExecutionOutcome),
}

pub(crate) fn decode(outcome: Option<&str>, finalization: &str) -> RuntimeResult<SessionProgress> {
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

/// The owner's exact run row. Every run transition checks it first.
pub(crate) fn require_run(
    connection: &Connection,
    run_id: &str,
    manifest_hash: &str,
) -> RuntimeResult<SessionProgress> {
    let row: Option<(Option<String>, String)> = connection
        .query_row(
            "SELECT execution_outcome, finalization FROM runs
         WHERE run_id = ?1 AND computed_manifest_hash = ?2",
            params![run_id, manifest_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    let (outcome, finalization) =
        row.ok_or_else(|| invalid("session provenance does not match its owner"))?;
    decode(outcome.as_deref(), &finalization)
}

/// Closed source registration forbids every new process source for the run.
pub(crate) fn require_open_sources(connection: &Connection, run_id: &str) -> RuntimeResult<()> {
    let sources: String = connection
        .query_row(
            "SELECT sources FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if sources != "open" {
        return Err(RuntimeError::new(
            ErrorCode::LifecycleFailed,
            "the session closed source registration; no new workload may register",
        ));
    }
    Ok(())
}

/// A fresh unpredictable run identity; the exclusive run-directory claim
/// refuses a collision.
pub fn new_run_id() -> RuntimeResult<String> {
    Ok(format!("run-{}", crate::token::random_hex()?))
}

/// Bounded machine selectors and private bootstrap identities share this exact
/// grammar. Filesystem placement deliberately has a broader component domain.
pub fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= 128
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Record the run row up front, before any service starts, so every admitted run
/// leaves durable evidence — including a service-less selection (a task or
/// environment of only service-less tasks) whose service loop never runs and so
/// never reaches `record_service_start_intent`. This is the only run-row creator.
pub fn record_run_created(
    registry: &mut Registry,
    run_id: &str,
    request: &DeclaredRunRequest<'_>,
    placement: &HostPlacement,
) -> RuntimeResult<()> {
    request.preflight(placement)?;
    let admission = request.admission;
    let source_json = serde_json::to_string(request.recorded().source())
        .map_err(|_| invalid("cannot encode recorded source"))?;
    let owner_identity = serde_json::to_string(&OwnerIdentity::current()?)
        .map_err(|_| invalid("cannot encode owner identity"))?;
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO runs (
              run_id, manifest_path, computed_manifest_hash, runtime_abi,
              toolchain_id, generator_json, target_json, source_json,
              summary_path, owner_identity, diagnostic_path, root_task, operation_timeout_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ",
            params![
                run_id,
                utf8_locator(admission.common().manifest_path(), "manifestPath")?,
                admission.common().computed_manifest_hash(),
                admission.common().runtime_abi(),
                admission.common().toolchain_id(),
                admission.common().generator_json(),
                admission.common().target_json(),
                source_json,
                utf8_locator(&placement.summary_path(), "summaryPath")?,
                owner_identity,
                crate::output::DIAGNOSTIC_SOURCE,
                request.root_task().as_str(),
                request.operation_timeout_ms().to_string(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "run.created",
            run_id: Some(run_id),
            process_key: None,
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
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
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    match require_run(&transaction, run_id, manifest_hash)? {
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
        redactor,
        EventInsert {
            event_type: "run.execution-settled",
            run_id: Some(run_id),
            process_key: None,
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
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let rows = {
        let mut statement = transaction
            .prepare("SELECT run_id, execution_outcome, finalization FROM runs ORDER BY run_id")
            .map_err(sql_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    let mut interrupted = Vec::new();
    for (run_id, outcome, finalization) in rows {
        if decode(outcome.as_deref(), &finalization)? == SessionProgress::Executing {
            interrupted.push(run_id);
        }
    }
    for run_id in interrupted {
        write_unknown_outcome(&transaction, &run_id, ExecutionOutcome::Interrupted)?;
        insert_event(
            &transaction,
            redactor,
            EventInsert {
                event_type: "run.interrupted",
                run_id: Some(&run_id),
                process_key: None,
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
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    match require_run(&transaction, run_id, manifest_hash)? {
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
        redactor,
        EventInsert {
            event_type: "run.finalized",
            run_id: Some(run_id),
            process_key: None,
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
    reason: &RuntimeError,
) -> RuntimeResult<()> {
    let payload = serde_json::json!({"code": reason.code, "message": reason.message}).to_string();
    let mut event = EventInsert::new("run.finalization-unfinished", &payload);
    event.run_id = Some(run_id);
    registry.append_event(event).map(|_| ())
}

/// Called by the exclusive successor only after it settled predecessor process,
/// deletion, and retention obligations. Known outcomes are preserved; the
/// successor completes finalization without adopting any predecessor work.
pub fn record_recovered_sessions(registry: &mut Registry) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT run_id, execution_outcome, finalization
                 FROM runs WHERE finalization = 'unfinished' ORDER BY run_id",
            )
            .map_err(sql_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    for (run_id, outcome, finalization) in rows {
        if decode(outcome.as_deref(), &finalization)? == SessionProgress::Executing {
            return Err(invalid(
                "recovery cannot settle a session whose interruption was not recorded",
            ));
        }
        write_complete(&transaction, &run_id)?;
        insert_event(
            &transaction,
            redactor,
            EventInsert {
                event_type: "run.recovered",
                run_id: Some(&run_id),
                process_key: None,
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
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, run_id, manifest_hash)?;
    let changed = transaction
        .execute(
            "UPDATE runs SET sources = 'closed' WHERE run_id = ?1 AND sources = 'open'",
            params![run_id],
        )
        .map_err(sql_error)?;
    if changed == 1 {
        insert_event(
            &transaction,
            redactor,
            EventInsert {
                event_type: "run.sources-closed",
                run_id: Some(run_id),
                process_key: None,
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
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, run_id, manifest_hash)?;
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
        redactor,
        EventInsert {
            event_type: "run.output-sealed",
            run_id: Some(run_id),
            process_key: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_owner_identity_has_one_exact_nonnullable_native_shape() {
        for json in [
            r#"{"pid":1,"platformStart":"expected"}"#,
            r#"{"pid":2147483647,"platformStart":"expected"}"#,
        ] {
            let identity: OwnerIdentity = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&identity).unwrap(), json);
        }
        for json in [
            r#"{}"#,
            r#"{"pid":1}"#,
            r#"{"platformStart":"expected"}"#,
            r#"{"pid":0,"platformStart":"expected"}"#,
            r#"{"pid":2147483648,"platformStart":"expected"}"#,
            r#"{"pid":1,"platformStart":null}"#,
            r#"{"pid":1,"platformStart":""}"#,
            r#"{"pid":1,"platformStart":"expected","extra":true}"#,
            r#"{"pid":1,"pid":2,"platformStart":"expected"}"#,
        ] {
            assert!(serde_json::from_str::<OwnerIdentity>(json).is_err());
        }
    }
}
