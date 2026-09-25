use std::collections::BTreeMap;

use nixfied_manifest::ContainmentRequirement;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::output::EvidenceSource;
use crate::redaction::CaptureOutcome;
use crate::registry::Registry;
use crate::registry::events::{EventInsert, insert_event};
use crate::registry::session::{require_open_sources, require_run};
use crate::registry::sql_error;
use crate::registry::sqlite::RegistryContext;
use crate::registry::status::{self, DbStatus, ProcessRole, ProcessStatus};

pub(crate) struct ServiceRecord<'a> {
    pub(crate) service_name: &'a str,
    pub(crate) source: &'a EvidenceSource,
    pub(crate) stop: StopPolicy,
}

/// How to terminate a recorded process without the manifest that started it.
/// Recovery uses these persisted facts, never a newly supplied manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StopPolicy {
    pub(crate) signal: i32,
    pub(crate) timeout_ms: u64,
    pub(crate) containment: ContainmentRequirement,
}

impl StopPolicy {
    /// Tasks and probes: their owning group, SIGTERM, then SIGKILL after 1 s.
    pub(crate) const INVOCATION: Self = Self {
        signal: libc::SIGTERM,
        timeout_ms: 1000,
        containment: ContainmentRequirement::ProcessGroup,
    };

    /// The stored containment column.
    fn containment(&self) -> &'static str {
        match self.containment {
            ContainmentRequirement::ProcessGroup => "process-group",
            ContainmentRequirement::ProcessTree => "process-tree",
        }
    }

    /// Parse the stored containment column.
    pub(crate) fn parse_containment(stored: &str) -> RuntimeResult<ContainmentRequirement> {
        match stored {
            "process-group" => Ok(ContainmentRequirement::ProcessGroup),
            "process-tree" => Ok(ContainmentRequirement::ProcessTree),
            _ => Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                "invalid process containment",
            )),
        }
    }
}

/// Immutable endpoint evidence recorded atomically with its owning service
/// process; it is settled exactly when that process is. Host startup locks and
/// kernel preflight protect the earlier startup interval.
pub(crate) struct EndpointRecord<'a> {
    pub(crate) endpoint_id: &'a str,
    pub(crate) address: &'a str,
    pub(crate) port: u16,
}

pub(crate) struct ProcessRecord<'a> {
    pub(crate) process_key: &'a str,
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) start_identity: &'a str,
    pub(crate) command_json: &'a str,
}

/// A recorded endpoint whose exact listener ownership was verified at ready.
pub(crate) struct VerifiedEndpoint<'a> {
    pub(crate) endpoint: EndpointRecord<'a>,
    pub(crate) ownership_json: &'a str,
}

pub(crate) struct InvocationProcessRecord<'a> {
    pub(crate) source: &'a EvidenceSource,
    pub(crate) run_id: &'a str,
    pub(crate) process_key: &'a str,
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) start_identity: &'a str,
    pub(crate) command_json: &'a str,
    pub(crate) computed_manifest_hash: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskTerminalStatus {
    Succeeded,
    Failed,
    /// The task exceeded its own timeout budget: an execution failure with its
    /// own event, never conflated with an operator cancellation.
    TimedOut,
    Canceled,
}

impl TaskTerminalStatus {
    /// The execution outcome this terminal status records.
    pub(crate) fn execution_outcome(self) -> crate::registry::session::ExecutionOutcome {
        use crate::registry::session::ExecutionOutcome;
        match self {
            Self::Succeeded => ExecutionOutcome::Succeeded,
            Self::Failed | Self::TimedOut => ExecutionOutcome::Failed,
            Self::Canceled => ExecutionOutcome::Canceled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceStartOutcome {
    Canceled,
    Failed,
}

/// The declared service a start event attributes before any process exists.
fn service_payload(service_name: &str) -> String {
    serde_json::json!({ "serviceId": service_name }).to_string()
}

/// Record startup intent under slot ownership before prepare or spawn.
/// The run already exists; endpoint evidence is recorded with the child process.
pub(crate) fn record_service_start_intent(
    registry: &mut Registry,
    run_id: &str,
    computed_manifest_hash: &str,
    service_name: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, run_id, computed_manifest_hash)?;
    ensure_predecessor_settled(&transaction, run_id)?;
    refuse_active_service(&transaction, run_id, service_name)?;
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "service.start-intent",
            run_id: Some(run_id),
            process_key: None,
            payload_json: &service_payload(service_name),
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Settle failed startup when no child exists, or after its containment is gone.
/// A registered child's endpoint evidence settles with its process.
pub(crate) fn settle_service_start(
    registry: &mut Registry,
    run_id: &str,
    service_name: &str,
    outcome: ServiceStartOutcome,
) -> RuntimeResult<()> {
    let event_type = match outcome {
        ServiceStartOutcome::Canceled => "service.start-canceled",
        ServiceStartOutcome::Failed => "service.start-failed",
    };
    record_event(
        registry,
        event_type,
        Some(run_id),
        None,
        &service_payload(service_name),
    )
}

pub(crate) fn record_service_start(
    registry: &mut Registry,
    run_id: &str,
    computed_manifest_hash: &str,
    service: &ServiceRecord<'_>,
    process: &ProcessRecord<'_>,
    endpoints: &[EndpointRecord<'_>],
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let process_command_json = redactor.redact_json_str(process.command_json)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, run_id, computed_manifest_hash)?;
    require_open_sources(&transaction, run_id)?;
    ensure_predecessor_settled(&transaction, run_id)?;
    refuse_active_service(&transaction, run_id, service.service_name)?;
    transaction
        .execute(
            "
            INSERT INTO processes (
              process_key, pid, pgid, start_identity, command_json, run_id,
              status, service_name, role, source_label, presentation,
              stdout_path, stderr_path, stop_signal, stop_timeout_ms, containment
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'service', ?9, ?10, ?11, ?12,
                      ?13, ?14, ?15)
            ",
            params![
                process.process_key,
                process.pid,
                process.pgid,
                process.start_identity,
                process_command_json,
                run_id,
                ProcessStatus::Running.as_str(),
                service.service_name,
                service.source.label,
                service.source.presentation.as_str(),
                service.source.stdout_relative,
                service.source.stderr_relative,
                service.stop.signal,
                service.stop.timeout_ms,
                service.stop.containment(),
            ],
        )
        .map_err(sql_error)?;
    for endpoint in endpoints {
        transaction
            .execute(
                "INSERT INTO ports (owner_process_key, endpoint_id, address, port)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    process.process_key,
                    endpoint.endpoint_id,
                    endpoint.address,
                    endpoint.port,
                ],
            )
            .map_err(sql_error)?;
    }
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "service.starting",
            run_id: Some(run_id),
            process_key: Some(process.process_key),
            payload_json: &process_command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Commit readiness only when the verified endpoint set is exactly the
/// evidence recorded with the process. Endpoint rows are never rewritten; the
/// verification is recorded as `port.owner-verified` events.
pub(crate) fn activate_service_ready(
    registry: &mut Registry,
    run_id: &str,
    process_key: &str,
    endpoints: &[VerifiedEndpoint<'_>],
    lifecycle: (&str, &str, &str),
) -> RuntimeResult<()> {
    let (operation_id, operation_class, terminal_success) = lifecycle;
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let stored = recorded_endpoints(&transaction, process_key)?;
    let expected = endpoints
        .iter()
        .map(|verified| {
            (
                verified.endpoint.endpoint_id.to_owned(),
                (verified.endpoint.address.to_owned(), verified.endpoint.port),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if expected.len() != endpoints.len() || stored != expected {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "ready endpoint evidence mismatch for {process_key}: stored {stored:?}, expected {expected:?}"
            ),
        ));
    }
    for verified in endpoints {
        insert_event(
            &transaction,
            redactor,
            EventInsert {
                event_type: "port.owner-verified",
                run_id: Some(run_id),
                process_key: Some(process_key),
                payload_json: verified.ownership_json,
            },
        )?;
    }
    let changed_process = transaction
        .execute(
            "
            UPDATE processes
            SET status = ?4
            WHERE process_key = ?1 AND run_id = ?2 AND role = 'service' AND status = ?3
            ",
            params![
                process_key,
                run_id,
                ProcessStatus::Running.as_str(),
                ProcessStatus::Ready.as_str(),
            ],
        )
        .map_err(sql_error)?;
    if changed_process != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("ready activation changed {changed_process} process rows"),
        ));
    }
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "service.probe-ready",
            run_id: Some(run_id),
            process_key: Some(process_key),
            payload_json: "{}",
        },
    )?;
    let lifecycle_payload = serde_json::json!({
        "operationId": operation_id,
        "class": operation_class,
        "terminalResult": terminal_success,
        "errorCode": serde_json::Value::Null,
        "message": serde_json::Value::Null,
    })
    .to_string();
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "service.lifecycle.terminal",
            run_id: Some(run_id),
            process_key: Some(process_key),
            payload_json: &lifecycle_payload,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// A service's terminal record; the payload accompanies its event.
pub(crate) enum ServiceTerminal<'a> {
    /// `None` from recovery: the predecessor's capture outcome stays as recorded.
    Stopped(Option<CaptureOutcome>),
    Canceled(&'a str, ServiceSettlement),
    Failed(&'a str, ServiceSettlement),
}

/// What the owner proved when a service reached a terminal status.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ServiceSettlement {
    pub(crate) ownership: Ownership,
    pub(crate) capture: CaptureOutcome,
}

/// Whether the owner proved that a process and its captured writers are gone.
/// Leader exit alone never settles ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ownership {
    Settled,
    Unresolved,
}

impl Ownership {
    fn as_str(self) -> &'static str {
        match self {
            Self::Settled => "settled",
            Self::Unresolved => "unresolved",
        }
    }
}

pub(crate) fn settle_service_terminal(
    registry: &mut Registry,
    run_id: &str,
    process_key: &str,
    terminal: ServiceTerminal<'_>,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let (process_status, event_type, payload_json, ownership, capture) = match terminal {
        ServiceTerminal::Stopped(capture) => (
            ProcessStatus::Stopped,
            "service.stopped",
            "{}",
            Ownership::Settled,
            capture,
        ),
        ServiceTerminal::Canceled(payload, settlement) => (
            ProcessStatus::Canceled,
            "service.canceled",
            payload,
            settlement.ownership,
            Some(settlement.capture),
        ),
        ServiceTerminal::Failed(payload, settlement) => (
            ProcessStatus::Failed,
            "service.failed",
            payload,
            settlement.ownership,
            Some(settlement.capture),
        ),
    };
    let changed = transaction
        .execute(
            "UPDATE processes SET status = ?3, ownership = ?4, capture = coalesce(?5, capture)
             WHERE process_key = ?1 AND run_id = ?2 AND role = 'service'",
            params![
                process_key,
                run_id,
                process_status.as_str(),
                ownership.as_str(),
                capture.map(CaptureOutcome::as_str)
            ],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("service settlement changed {changed} process rows for {process_key}"),
        ));
    }
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type,
            run_id: Some(run_id),
            process_key: Some(process_key),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Append one event outside any other registry transition.
pub(crate) fn record_event(
    registry: &mut Registry,
    event_type: &str,
    run_id: Option<&str>,
    process_key: Option<&str>,
    payload_json: &str,
) -> RuntimeResult<()> {
    registry
        .append_event(EventInsert {
            event_type,
            run_id,
            process_key,
            payload_json,
        })
        .map(|_| ())
}

/// Record a process's checked capture outcome learned outside a terminal
/// settlement. Unknown or incomplete capture never becomes complete later.
pub(crate) fn record_capture_outcome(
    registry: &mut Registry,
    run_id: &str,
    process_key: &str,
    capture: CaptureOutcome,
) -> RuntimeResult<()> {
    let RegistryContext { connection, .. } = registry.context()?;
    let changed = connection
        .execute(
            "UPDATE processes SET capture = ?3 WHERE process_key = ?1 AND run_id = ?2 AND capture = 'pending'",
            params![process_key, run_id, capture.as_str()],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "capture outcome must settle exactly one pending process source",
        ));
    }
    Ok(())
}

pub(crate) fn mark_process_escape(
    registry: &mut Registry,
    run_id: &str,
    process: &ProcessRecord<'_>,
    computed_manifest_hash: &str,
    platform_start: Option<&str>,
    payload_json: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let changed_process = transaction
        .execute(
            &format!(
                "
                UPDATE processes
                SET status = ?7, start_identity = ?6
                WHERE process_key = ?1
                  AND run_id = ?2
                  AND role = 'service'
                  AND pid = ?3
                  AND pgid = ?4
                  AND CAST(json_extract(start_identity, '$.pid') AS INTEGER) = ?3
                  AND CAST(json_extract(start_identity, '$.pgid') AS INTEGER) = ?4
                  AND json_extract(start_identity, '$.platformStart') IS ?5
                  AND CAST(json_extract(?6, '$.pid') AS INTEGER) = ?3
                  AND CAST(json_extract(?6, '$.pgid') AS INTEGER) = ?4
                  AND json_extract(?6, '$.platformStart') IS ?5
                  AND status IN ({})
                ",
                status::sql_in_list(status::PROCESS_ACTIVE)
            ),
            params![
                process.process_key,
                run_id,
                process.pid,
                process.pgid,
                platform_start,
                process.start_identity,
                ProcessStatus::Escaped.as_str(),
            ],
        )
        .map_err(sql_error)?;
    if changed_process != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "escape settlement changed {changed_process} process rows for {}, expected 1",
                process.process_key
            ),
        ));
    }
    require_run(&transaction, run_id, computed_manifest_hash)?;
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "service.proc-escape",
            run_id: Some(run_id),
            process_key: Some(process.process_key),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Settle an unresolved terminal process after OS observation proves its
/// recorded process, group, and tracked descendants are gone (or were
/// terminated by the exclusive recovery owner). Endpoint evidence it owns
/// settles with it; the terminal status is retained as history.
pub(crate) fn settle_unresolved_process(
    registry: &mut Registry,
    process_key: &str,
    run_id: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let changed = transaction
        .execute(
            &format!(
                "UPDATE processes SET ownership = 'settled'
                 WHERE process_key = ?1 AND run_id = ?2 AND ownership = 'unresolved'
                   AND status NOT IN ({})",
                status::sql_in_list(status::PROCESS_ACTIVE)
            ),
            params![process_key, run_id],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("unresolved process {process_key} no longer matches its recorded obligation"),
        ));
    }
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: "process.ownership-settled",
            run_id: Some(run_id),
            process_key: Some(process_key),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)
}

#[derive(Clone, Copy)]
pub(crate) enum InvocationOwner<'a> {
    Task,
    Probe(&'a str),
}
impl<'a> InvocationOwner<'a> {
    pub(crate) fn role(self) -> ProcessRole {
        match self {
            Self::Task => ProcessRole::Task,
            Self::Probe(_) => ProcessRole::Probe,
        }
    }
    fn service_name(self) -> Option<&'a str> {
        match self {
            Self::Task => None,
            Self::Probe(name) => Some(name),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct InvocationIdentity<'a> {
    pub run_id: &'a str,
    pub process_key: &'a str,
    pub manifest_hash: &'a str,
    pub owner: InvocationOwner<'a>,
}

pub(crate) fn record_invocation_started(
    registry: &mut Registry,
    process: &InvocationProcessRecord<'_>,
    owner: InvocationOwner<'_>,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let command_json = redactor.redact_json_str(process.command_json)?;
    let transaction = connection.transaction().map_err(sql_error)?;
    require_run(&transaction, process.run_id, process.computed_manifest_hash)?;
    require_open_sources(&transaction, process.run_id)?;
    transaction
        .execute(
            "
            INSERT INTO processes (
              process_key, pid, pgid, start_identity, command_json, run_id,
              status, role, service_name, source_label, presentation,
              stdout_path, stderr_path, stop_signal, stop_timeout_ms, containment
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ",
            params![
                process.process_key,
                process.pid,
                process.pgid,
                process.start_identity,
                command_json,
                process.run_id,
                ProcessStatus::Running.as_str(),
                owner.role().as_str(),
                owner.service_name(),
                process.source.label,
                process.source.presentation.as_str(),
                process.source.stdout_relative,
                process.source.stderr_relative,
                StopPolicy::INVOCATION.signal,
                StopPolicy::INVOCATION.timeout_ms,
                StopPolicy::INVOCATION.containment(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: match owner {
                InvocationOwner::Task => "task.running",
                InvocationOwner::Probe(_) => "probe.running",
            },
            run_id: Some(process.run_id),
            process_key: Some(process.process_key),
            payload_json: &command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

pub(crate) fn record_invocation_observed(
    registry: &mut Registry,
    invocation: InvocationIdentity<'_>,
    outcome: crate::registry::session::ExecutionOutcome,
    exit_code: Option<i32>,
) -> RuntimeResult<()> {
    let InvocationIdentity {
        run_id,
        process_key,
        manifest_hash,
        owner,
    } = invocation;
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
            "UPDATE processes SET execution_outcome = ?3, exit_code = ?4
         WHERE process_key = ?1 AND run_id = ?2 AND execution_outcome IS NULL AND role = ?5",
            params![
                process_key,
                run_id,
                outcome.as_str(),
                exit_code,
                owner.role().as_str()
            ],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "task observation must update exactly one previously unobserved process",
        ));
    }
    let payload =
        serde_json::json!({"outcome": outcome.as_str(), "exitCode": exit_code}).to_string();
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type: match owner {
                InvocationOwner::Task => "task.execution-observed",
                InvocationOwner::Probe(_) => "probe.execution-observed",
            },
            run_id: Some(run_id),
            process_key: Some(process_key),
            payload_json: &payload,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// Termination intent of a registered task or probe, before any signal.
pub(crate) fn record_invocation_canceling(
    registry: &mut Registry,
    invocation: InvocationIdentity<'_>,
    payload_json: &str,
) -> RuntimeResult<()> {
    record_event(
        registry,
        match invocation.owner {
            InvocationOwner::Task => "task.canceling",
            InvocationOwner::Probe(_) => "probe.canceling",
        },
        Some(invocation.run_id),
        Some(invocation.process_key),
        payload_json,
    )
}

/// `capture` is the owner's checked outcome; recovery settlement passes `None`
/// and never invents a capture result for a predecessor's source.
pub(crate) fn mark_invocation_finished(
    registry: &mut Registry,
    invocation: InvocationIdentity<'_>,
    terminal_status: TaskTerminalStatus,
    payload_json: &str,
    capture: Option<CaptureOutcome>,
) -> RuntimeResult<()> {
    let InvocationIdentity {
        run_id,
        process_key,
        owner,
        ..
    } = invocation;
    let (process_status, event_type) = match terminal_status {
        TaskTerminalStatus::Succeeded => (ProcessStatus::Succeeded, "task.succeeded"),
        TaskTerminalStatus::Failed => (ProcessStatus::Failed, "task.failed"),
        TaskTerminalStatus::TimedOut => (ProcessStatus::Failed, "task.timed-out"),
        TaskTerminalStatus::Canceled => (ProcessStatus::Canceled, "task.canceled"),
    };
    let event_type = match owner {
        InvocationOwner::Task => event_type,
        InvocationOwner::Probe(_) => match terminal_status {
            TaskTerminalStatus::Succeeded => "probe.succeeded",
            TaskTerminalStatus::Failed => "probe.failed",
            TaskTerminalStatus::TimedOut => "probe.timed-out",
            TaskTerminalStatus::Canceled => "probe.canceled",
        },
    };
    let RegistryContext {
        connection,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    let changed = transaction
        .execute(
            "UPDATE processes SET status = ?2, ownership = 'settled', capture = coalesce(?5, capture)
             WHERE process_key = ?1 AND run_id = ?3 AND role = ?4",
            params![
                process_key,
                process_status.as_str(),
                run_id,
                owner.role().as_str(),
                capture.map(CaptureOutcome::as_str)
            ],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "invocation settlement must update exactly one matching process",
        ));
    }
    insert_event(
        &transaction,
        redactor,
        EventInsert {
            event_type,
            run_id: Some(run_id),
            process_key: Some(process_key),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)
}

/// The endpoint evidence recorded with one service process.
fn recorded_endpoints(
    connection: &Connection,
    process_key: &str,
) -> RuntimeResult<BTreeMap<String, (String, u16)>> {
    connection
        .prepare("SELECT endpoint_id, address, port FROM ports WHERE owner_process_key = ?1")
        .map_err(sql_error)?
        .query_map([process_key], |row| {
            Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
        })
        .map_err(sql_error)?
        .collect::<Result<_, _>>()
        .map_err(sql_error)
}

fn ensure_predecessor_settled(transaction: &Transaction<'_>, run_id: &str) -> RuntimeResult<()> {
    let mut statement = transaction
        .prepare(&format!(
            "SELECT p.status, ({}) FROM processes p WHERE p.run_id != ?1",
            status::actionable_process_sql()
        ))
        .map_err(sql_error)?;
    let rows = statement
        .query_map([run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })
        .map_err(sql_error)?;
    for row in rows {
        let (stored_status, actionable) = row.map_err(sql_error)?;
        ProcessStatus::parse_db(&stored_status)?;
        if actionable {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                "service startup requires predecessor process settlement",
            ));
        }
    }
    Ok(())
}

/// A declared service starts at most once in a run while an earlier process
/// of it is still an ownership obligation.
fn refuse_active_service(
    connection: &Connection,
    run_id: &str,
    service_name: &str,
) -> RuntimeResult<()> {
    let existing: Option<String> = connection
        .query_row(
            &format!(
                "
                SELECT p.status
                FROM processes p
                WHERE p.run_id = ?1 AND p.service_name = ?2 AND p.role = 'service'
                  AND ({actionable})
                ORDER BY p.process_key
                LIMIT 1
                ",
                actionable = status::actionable_process_sql(),
            ),
            params![run_id, service_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    match existing {
        Some(status) => Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("service {service_name} of run {run_id} has actionable process {status}"),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::registry::RegistryIdentity;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    const RUN_ID: &str = "run-test";
    const MANIFEST_HASH: &str = "manifest-hash";
    const SERVICE: &str = "service";
    const PROCESS_KEY: &str = "process-key";
    const START_IDENTITY: &str =
        r#"{"pid":123,"pgid":123,"platformStart":null,"trackedProcesses":[]}"#;

    struct TestRegistry {
        registry: Registry,
        _root: crate::test_support::TestDir,
    }

    impl TestRegistry {
        fn new() -> Self {
            let root = crate::test_support::TestDir::new("service-registry-test");
            let identity =
                RegistryIdentity::for_slot("test-project", "dev", 0, "test-abi", "test-tool");
            let registry = Registry::open_or_create(
                crate::state::ownership::fixture_guard(&root, &identity),
                &identity,
            )
            .expect("test registry should open");
            Self {
                registry,
                _root: root,
            }
        }

        fn path(&self) -> PathBuf {
            self.registry.path().to_path_buf()
        }
    }

    fn fixture_source(stem: &str) -> &'static EvidenceSource {
        Box::leak(Box::new(EvidenceSource::in_logs(
            Path::new("/run/logs"),
            stem,
            crate::output::SourcePresentation::Shown,
            stem,
        )))
    }

    fn service_record() -> ServiceRecord<'static> {
        ServiceRecord {
            service_name: SERVICE,
            source: fixture_source("service"),
            stop: StopPolicy::INVOCATION,
        }
    }

    fn process_record(pid: u32) -> ProcessRecord<'static> {
        ProcessRecord {
            process_key: PROCESS_KEY,
            pid,
            pgid: 123,
            start_identity: START_IDENTITY,
            command_json: "{}",
        }
    }

    fn endpoint() -> EndpointRecord<'static> {
        EndpointRecord {
            endpoint_id: "endpoint",
            address: "127.0.0.1",
            port: 24222,
        }
    }

    fn insert_run(registry: &Registry, run_id: &str) {
        let identity = registry.identity();
        registry
            .connection()
            .execute(
                "
                INSERT INTO runs (
                  run_id, execution_outcome, manifest_path, computed_manifest_hash,
                  runtime_abi, toolchain_id, generator_json, target_json, source_json,
                  summary_path, owner_identity, diagnostic_path
                ) VALUES (?1, NULL, '/manifest', ?2, ?3, ?4, '{}', '{}', '{}', NULL, '{}', 'diagnostics.log')
                ",
                params![
                    run_id,
                    MANIFEST_HASH,
                    identity.runtime_abi,
                    identity.toolchain_id,
                ],
            )
            .expect("test run should insert");
    }

    fn record_intent(registry: &mut Registry, run_id: &str) {
        record_service_start_intent(registry, run_id, MANIFEST_HASH, SERVICE)
            .expect("startup intent should commit");
    }

    fn task_invocation<'a>(
        run_id: &'a str,
        process_key: &'a str,
        manifest_hash: &'a str,
    ) -> InvocationIdentity<'a> {
        InvocationIdentity {
            run_id,
            process_key,
            manifest_hash,
            owner: InvocationOwner::Task,
        }
    }

    fn record_started(registry: &mut Registry) {
        insert_run(registry, RUN_ID);
        record_intent(registry, RUN_ID);
        record_service_start(
            registry,
            RUN_ID,
            MANIFEST_HASH,
            &service_record(),
            &process_record(123),
            &[endpoint()],
        )
        .expect("test service should be recorded");
    }

    #[test]
    fn closed_source_registration_rejects_every_new_process_source() {
        let mut fixture = TestRegistry::new();
        let registry = &mut fixture.registry;
        insert_run(registry, RUN_ID);
        registry
            .connection()
            .execute("UPDATE runs SET sources = 'closed'", [])
            .unwrap();
        let task = InvocationProcessRecord {
            source: fixture_source("task"),
            run_id: RUN_ID,
            process_key: PROCESS_KEY,
            pid: 123,
            pgid: 123,
            start_identity: START_IDENTITY,
            command_json: "{}",
            computed_manifest_hash: MANIFEST_HASH,
        };
        let error = record_invocation_started(registry, &task, InvocationOwner::Task).unwrap_err();
        assert_eq!(error.code, ErrorCode::LifecycleFailed);
        let error = record_service_start(
            registry,
            RUN_ID,
            MANIFEST_HASH,
            &service_record(),
            &process_record(123),
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::LifecycleFailed);
        let processes: i64 = registry
            .connection()
            .query_row("SELECT count(*) FROM processes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(processes, 0);
    }

    #[test]
    fn task_observation_is_atomic_immutable_and_scoped() {
        use crate::registry::session::ExecutionOutcome;

        let mut fixture = TestRegistry::new();
        let registry = &mut fixture.registry;
        insert_run(registry, RUN_ID);
        record_invocation_started(
            registry,
            &InvocationProcessRecord {
                source: fixture_source("task"),
                run_id: RUN_ID,
                process_key: PROCESS_KEY,
                pid: 123,
                pgid: 123,
                start_identity: START_IDENTITY,
                command_json: "{}",
                computed_manifest_hash: MANIFEST_HASH,
            },
            InvocationOwner::Task,
        )
        .unwrap();

        for (run, process, manifest) in [
            ("other-run", PROCESS_KEY, MANIFEST_HASH),
            (RUN_ID, "other-process", MANIFEST_HASH),
            (RUN_ID, PROCESS_KEY, "other-manifest"),
        ] {
            assert!(
                record_invocation_observed(
                    registry,
                    task_invocation(run, process, manifest),
                    ExecutionOutcome::Succeeded,
                    Some(7),
                )
                .is_err()
            );
        }

        // Failure after the process update must roll back both result and event.
        registry
            .connection()
            .execute_batch(
                "CREATE TRIGGER reject_observation BEFORE INSERT ON events
             WHEN NEW.event_type = 'task.execution-observed'
             BEGIN SELECT RAISE(ABORT, 'injected event failure'); END;",
            )
            .unwrap();
        assert!(
            record_invocation_observed(
                registry,
                task_invocation(RUN_ID, PROCESS_KEY, MANIFEST_HASH),
                ExecutionOutcome::Succeeded,
                Some(7),
            )
            .is_err()
        );
        let observed: Option<String> = registry
            .connection()
            .query_row(
                "SELECT execution_outcome FROM processes WHERE process_key = ?1",
                [PROCESS_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(observed, None);
        registry
            .connection()
            .execute_batch("DROP TRIGGER reject_observation")
            .unwrap();

        record_invocation_observed(
            registry,
            task_invocation(RUN_ID, PROCESS_KEY, MANIFEST_HASH),
            ExecutionOutcome::Succeeded,
            Some(7),
        )
        .unwrap();
        for outcome in [ExecutionOutcome::Succeeded, ExecutionOutcome::Failed] {
            assert!(
                record_invocation_observed(
                    registry,
                    task_invocation(RUN_ID, PROCESS_KEY, MANIFEST_HASH),
                    outcome,
                    Some(1),
                )
                .is_err()
            );
        }
        let result: (String, i32, String, i64, Option<String>) = registry
            .connection()
            .query_row(
                "SELECT execution_outcome, exit_code, status,
                (SELECT count(*) FROM events WHERE event_type = 'task.execution-observed'),
                (SELECT execution_outcome FROM runs WHERE run_id = ?2)
             FROM processes WHERE process_key = ?1",
                params![PROCESS_KEY, RUN_ID],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(result, ("succeeded".into(), 7, "running".into(), 1, None));
    }

    #[test]
    fn startup_intent_records_no_endpoint_evidence() {
        let mut fixture = TestRegistry::new();
        insert_run(&fixture.registry, RUN_ID);
        record_intent(&mut fixture.registry, RUN_ID);
        let ports: i64 = fixture
            .registry
            .connection()
            .query_row("SELECT count(*) FROM ports", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ports, 0);
        // Endpoint evidence cannot exist without an existing owning process.
        for owner in ["NULL", "'absent-process'"] {
            let error = fixture
                .registry
                .connection()
                .execute(
                    &format!(
                        "INSERT INTO ports (owner_process_key, endpoint_id, address, port)
                         VALUES ({owner}, 'endpoint', '127.0.0.1', 24222)"
                    ),
                    [],
                )
                .unwrap_err();
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation)
            );
        }
        record_service_start(
            &mut fixture.registry,
            RUN_ID,
            MANIFEST_HASH,
            &service_record(),
            &process_record(123),
            &[endpoint()],
        )
        .unwrap();
        let owner: String = fixture
            .registry
            .connection()
            .query_row("SELECT owner_process_key FROM ports", [], |row| row.get(0))
            .unwrap();
        assert_eq!(owner, PROCESS_KEY);
    }

    #[test]
    fn process_recording_requires_exact_run_provenance_before_mutation() {
        for mutation in ["missing", "hash"] {
            let mut fixture = TestRegistry::new();
            if mutation != "missing" {
                insert_run(&fixture.registry, RUN_ID);
                fixture
                    .registry
                    .connection()
                    .execute_batch(match mutation {
                        "hash" => "UPDATE runs SET computed_manifest_hash = 'other'",
                        _ => unreachable!(),
                    })
                    .unwrap();
            }
            let error = record_service_start(
                &mut fixture.registry,
                RUN_ID,
                MANIFEST_HASH,
                &service_record(),
                &process_record(123),
                &[endpoint()],
            )
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::RegistryCorrupt);
            let mutations: i64 = fixture
                .registry
                .connection()
                .query_row(
                    "SELECT (SELECT count(*) FROM sqlite_master WHERE name = 'services') + (SELECT count(*) FROM processes)
                        + (SELECT count(*) FROM ports) + (SELECT count(*) FROM events)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(mutations, 0, "{mutation}");
        }
    }

    #[test]
    fn mutation_revalidates_slot_authority_before_writing() {
        let mut fixture = TestRegistry::new();
        insert_run(&fixture.registry, RUN_ID);
        let lock = fixture.path().parent().unwrap().join("slot.lock");
        std::fs::rename(&lock, lock.with_file_name("displaced.lock")).unwrap();
        let error =
            record_service_start_intent(&mut fixture.registry, RUN_ID, MANIFEST_HASH, SERVICE)
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::StateUnowned);
        let events: i64 = fixture
            .registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 0);
    }

    #[test]
    fn successor_rejects_corrupt_predecessor_status_before_new_intent() {
        let mut fixture = TestRegistry::new();
        record_started(&mut fixture.registry);
        insert_run(&fixture.registry, "successor");
        fixture
            .registry
            .connection()
            .execute_batch("UPDATE processes SET status = 'unknown-status'")
            .unwrap();
        let events_before: i64 = fixture
            .registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        let error = record_service_start_intent(
            &mut fixture.registry,
            "successor",
            MANIFEST_HASH,
            "new-session-service",
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        let events_after: i64 = fixture
            .registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events_after, events_before);
    }

    #[test]
    fn service_label_is_process_evidence_and_cannot_be_detached() {
        let mut fixture = TestRegistry::new();
        record_started(&mut fixture.registry);
        let process: (String, String, String) = fixture
            .registry
            .connection()
            .query_row(
                "SELECT service_name, run_id, role FROM processes WHERE process_key = ?1",
                [PROCESS_KEY],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(process, (SERVICE.into(), RUN_ID.into(), "service".into()));

        for mutation in [
            "UPDATE processes SET service_name = NULL",
            "UPDATE processes SET service_name = ''",
            "UPDATE processes SET run_id = 'absent-run'",
            "UPDATE processes SET role = NULL",
            "UPDATE processes SET role = 'unknown'",
            "UPDATE processes SET role = 'task'",
        ] {
            assert!(
                fixture
                    .registry
                    .connection()
                    .execute_batch(mutation)
                    .is_err(),
                "{mutation}"
            );
        }
        settle_service_terminal(
            &mut fixture.registry,
            RUN_ID,
            PROCESS_KEY,
            ServiceTerminal::Stopped(Some(CaptureOutcome::Complete)),
        )
        .unwrap();
        let evidence: (String, String) = fixture
            .registry
            .connection()
            .query_row(
                "SELECT service_name, status FROM processes WHERE process_key = ?1",
                [PROCESS_KEY],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(evidence, ("service".into(), "stopped".into()));
    }

    #[test]
    fn start_intent_refuses_an_actionable_process_of_the_same_service() {
        let mut fixture = TestRegistry::new();
        record_started(&mut fixture.registry);
        let error =
            record_service_start_intent(&mut fixture.registry, RUN_ID, MANIFEST_HASH, SERVICE)
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        assert_eq!(
            error.message,
            "service service of run run-test has actionable process running"
        );
        // Another declared service of the same run is independent.
        record_service_start_intent(&mut fixture.registry, RUN_ID, MANIFEST_HASH, "other").unwrap();
    }

    #[test]
    fn endpoint_evidence_retains_stored_ipv6_spelling() {
        let mut fixture = TestRegistry::new();
        insert_run(&fixture.registry, RUN_ID);
        record_service_start(
            &mut fixture.registry,
            RUN_ID,
            MANIFEST_HASH,
            &service_record(),
            &process_record(123),
            &[EndpointRecord {
                address: "0:0:0:0:0:0:0:1",
                ..endpoint()
            }],
        )
        .unwrap();
        for address in ["::1", "0:0:0:0:0:0:0:1"] {
            let result = activate_service_ready(
                &mut fixture.registry,
                RUN_ID,
                PROCESS_KEY,
                &[VerifiedEndpoint {
                    endpoint: EndpointRecord {
                        address,
                        ..endpoint()
                    },
                    ownership_json: "{}",
                }],
                ("service.ready", "ready", "ready"),
            );
            if address == "::1" {
                assert_eq!(result.unwrap_err().code, ErrorCode::RegistryCorrupt);
            } else {
                result.unwrap();
            }
        }
        let stored: String = fixture
            .registry
            .connection()
            .query_row("SELECT address FROM ports", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, "0:0:0:0:0:0:0:1");
    }

    #[test]
    fn ready_activation_requires_the_exact_recorded_endpoint_set() {
        for mutation in [
            "UPDATE ports SET endpoint_id = 'other'",
            "UPDATE ports SET address = '0.0.0.0'",
            "UPDATE ports SET port = port + 1",
            "DELETE FROM ports",
            "INSERT INTO ports (owner_process_key, endpoint_id, address, port)
             SELECT owner_process_key, 'extra', address, port + 1 FROM ports",
        ] {
            let mut fixture = TestRegistry::new();
            record_started(&mut fixture.registry);
            fixture
                .registry
                .connection()
                .execute_batch(mutation)
                .unwrap();
            let before: (i64, String) = fixture
                .registry
                .connection()
                .query_row(
                    "SELECT (SELECT count(*) FROM events), (SELECT status FROM processes)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let result = activate_service_ready(
                &mut fixture.registry,
                RUN_ID,
                PROCESS_KEY,
                &[VerifiedEndpoint {
                    endpoint: endpoint(),
                    ownership_json: "{}",
                }],
                ("service.ready", "ready", "ready"),
            );
            assert_eq!(
                result.unwrap_err().code,
                ErrorCode::RegistryCorrupt,
                "{mutation}"
            );
            let after: (i64, String) = fixture
                .registry
                .connection()
                .query_row(
                    "SELECT (SELECT count(*) FROM events), (SELECT status FROM processes)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(after, before, "{mutation}");
        }
        // Stored endpoint evidence keeps a nonempty identity and a TCP port.
        let fixture = TestRegistry::new();
        let mut registry = fixture.registry;
        record_started(&mut registry);
        for mutation in [
            "UPDATE ports SET endpoint_id = ''",
            "UPDATE ports SET port = 0",
            "UPDATE ports SET port = 65536",
        ] {
            assert!(registry.connection().execute_batch(mutation).is_err());
        }
    }

    #[test]
    fn event_failure_rolls_back_service_process_ports_and_prior_event() {
        let mut fixture = TestRegistry::new();
        insert_run(&fixture.registry, RUN_ID);
        record_intent(&mut fixture.registry, RUN_ID);
        fixture
            .registry
            .connection()
            .execute_batch(
                "CREATE TRIGGER deny_start_event BEFORE INSERT ON events
             WHEN NEW.event_type = 'service.starting'
             BEGIN SELECT RAISE(ABORT, 'test event rejection'); END;",
            )
            .unwrap();
        let error = record_service_start(
            &mut fixture.registry,
            RUN_ID,
            MANIFEST_HASH,
            &service_record(),
            &process_record(123),
            &[endpoint()],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        let state: (i64, i64, i64, i64) = fixture
            .registry
            .connection()
            .query_row(
                "SELECT (SELECT count(*) FROM sqlite_master WHERE name = 'services'), (SELECT count(*) FROM processes),
                    (SELECT count(*) FROM ports),
                    (SELECT count(*) FROM events)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(state, (0, 0, 0, 1));
        let event: String = fixture
            .registry
            .connection()
            .query_row("SELECT event_type FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(event, "service.start-intent");
    }

    #[test]
    fn start_intent_waits_for_the_registry_writer() {
        let mut waiting = TestRegistry::new();
        insert_run(&waiting.registry, RUN_ID);
        let path = waiting.path();

        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let writer = thread::spawn(move || {
            // Fault injector holds an external SQLite write transaction; it has
            // no runtime mutation authority and cannot publish runtime events.
            let mut connection = rusqlite::Connection::open(path).unwrap();
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            transaction
                .execute(
                    "UPDATE registry_meta SET created_at = created_at WHERE id = 1",
                    [],
                )
                .unwrap();
            ready_tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(100));
            transaction.commit().unwrap();
        });
        ready_rx.recv().unwrap();
        record_intent(&mut waiting.registry, RUN_ID);
        writer.join().unwrap();
    }

    #[test]
    fn escape_settlement_requires_exact_evidence_and_rolls_back() {
        for mutation in [
            "missing-process",
            "wrong-process",
            "wrong-run",
            "event-failure",
        ] {
            let mut fixture = TestRegistry::new();
            record_started(&mut fixture.registry);
            match mutation {
                "wrong-run" => {
                    fixture
                        .registry
                        .connection()
                        .execute(
                            "UPDATE runs SET computed_manifest_hash = 'wrong-hash' WHERE run_id = ?1",
                            [RUN_ID],
                        )
                        .unwrap();
                }
                "event-failure" => fixture
                    .registry
                    .connection()
                    .execute_batch(
                        "
                        CREATE TRIGGER reject_proc_escape_event
                        BEFORE INSERT ON events
                        WHEN NEW.event_type = 'service.proc-escape'
                        BEGIN
                          SELECT RAISE(ABORT, 'injected escape event failure');
                        END;
                        ",
                    )
                    .unwrap(),
                _ => {}
            }
            let process = ProcessRecord {
                process_key: if mutation == "missing-process" {
                    "absent-process"
                } else {
                    PROCESS_KEY
                },
                ..process_record(if mutation == "wrong-process" {
                    124
                } else {
                    123
                })
            };
            let error = mark_process_escape(
                &mut fixture.registry,
                RUN_ID,
                &process,
                MANIFEST_HASH,
                None,
                r#"{"reason":"test"}"#,
            )
            .expect_err("escape settlement must require exact durable evidence");
            assert_eq!(error.code, ErrorCode::RegistryCorrupt);
            let state: (i64, i64, i64, i64) = fixture
                .registry
                .connection()
                .query_row(
                    "
                    SELECT
                      (SELECT count(*) FROM processes WHERE status = 'running'),
                      (SELECT count(*) FROM runs WHERE execution_outcome IS NULL),
                      (SELECT count(*) FROM ports),
                      (SELECT count(*) FROM events WHERE event_type = 'service.proc-escape')
                    ",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            assert_eq!(state, (1, 1, 1, 0));
        }
    }

    #[test]
    fn registry_rows_reference_their_run_and_process() {
        let fixture = TestRegistry::new();
        let connection = fixture.registry.connection();
        for statement in [
            "INSERT INTO processes (
               process_key, pid, pgid, start_identity, command_json, run_id, status, role,
               source_label, presentation, stdout_path, stderr_path, stop_signal,
               stop_timeout_ms, containment
             ) VALUES ('p', 1, 1, '{}', '{}', 'absent-run', 'running', 'task', 'task',
                       'shown', 'out', 'err', 15, 1000, 'process-group')",
            "INSERT INTO events (at, event_type, run_id, payload_json)
             VALUES ('now', 'test', 'absent-run', '{}')",
            "INSERT INTO events (at, event_type, process_key, payload_json)
             VALUES ('now', 'test', 'absent-process', '{}')",
        ] {
            let error = connection.execute_batch(statement).unwrap_err();
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation),
                "{statement}"
            );
        }
    }
}
