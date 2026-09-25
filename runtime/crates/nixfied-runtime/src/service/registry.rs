pub(crate) use crate::registry::records::StoredEndpoint as StoredServiceEndpoint;
use crate::registry::records::read_open_endpoints;
use crate::registry::sql_error;
use crate::registry::sqlite::RegistryContext;
use nixfied_manifest::ContainmentRequirement;
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::admission::RunAdmission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::output::EvidenceSource;
use crate::redaction::CaptureOutcome;
use crate::registry::events::{EventInsert, append_event, insert_event};
use crate::registry::status::{self, DbStatus, PortStatus, ProcessRole, ProcessStatus};
use crate::registry::{Registry, RegistryIdentity};
use crate::state::HostPlacement;

use super::{StoredProcessIdentity, TrackedProcessIdentity};

pub(crate) struct ServiceRecord<'a> {
    pub(crate) service_instance_id: &'a str,
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

/// Endpoint evidence recorded atomically with its owning service process.
/// Host startup locks and kernel preflight protect the earlier startup interval.
pub(crate) struct EndpointRecord<'a> {
    pub(crate) endpoint_key: &'a str,
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

pub(crate) struct VerifiedEndpointActivation<'a> {
    pub(crate) endpoint_key: &'a str,
    pub(crate) address: &'a str,
    pub(crate) port: u16,
    pub(crate) ownership_json: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredServiceProcess {
    pub(crate) service_name: String,
    pub(crate) process_key: String,
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) platform_start: Option<String>,
    pub(crate) tracked_processes: Vec<TrackedProcessIdentity>,
    pub(crate) run_id: String,
    pub(crate) status: ProcessStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceSnapshot {
    pub(crate) process: Option<StoredServiceProcess>,
    pub(crate) endpoints: Vec<StoredServiceEndpoint>,
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

/// Record startup intent under slot ownership before prepare or spawn.
/// The run already exists; endpoint evidence is recorded with the child process.
pub(crate) fn record_service_start_intent(
    registry: &mut Registry,
    run_id: &str,
    computed_manifest_hash: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, identity, run_id, computed_manifest_hash)?;
    read_open_endpoints(&transaction, None)?;
    ensure_predecessor_settled(&transaction, run_id)?;
    refuse_active_service(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "service.start-intent",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: None,
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Record the run row up front, before any service starts, so every admitted run
/// leaves durable evidence — including a service-less selection (a task or
/// environment of only service-less tasks) whose service loop never runs and so
/// never reaches `record_service_start_intent`. This is the only run-row creator.
pub fn record_run_created(
    registry: &mut Registry,
    run_id: &str,
    admission: &RunAdmission,
    placement: &HostPlacement,
) -> RuntimeResult<()> {
    let source_json = serde_json::to_string(&admission.source()).map_err(json_error)?;
    // The owner's own process identity is recovery evidence for its session.
    let owner = std::process::id();
    let owner_identity = serde_json::json!({
        "pid": owner,
        "platformStart": crate::service::platform_start_identity(owner),
    })
    .to_string();
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO runs (
              run_id, environment, slot, manifest_path, computed_manifest_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path, owner_identity, diagnostic_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ",
            params![
                run_id,
                identity.environment.as_str(),
                identity.slot,
                admission.common().manifest_path().display().to_string(),
                admission.common().computed_manifest_hash(),
                admission.common().runtime_abi(),
                admission.common().toolchain_id(),
                admission.common().generator_json(),
                admission.common().target_json(),
                source_json,
                placement.summary_path().display().to_string(),
                owner_identity,
                crate::output::DIAGNOSTIC_SOURCE,
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "run.created",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: None,
            computed_manifest_hash: Some(admission.common().computed_manifest_hash()),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Settle failed startup when no child exists, or after its containment is gone.
pub(crate) fn settle_service_start(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    outcome: ServiceStartOutcome,
) -> RuntimeResult<()> {
    let event_type = match outcome {
        ServiceStartOutcome::Canceled => "service.start-canceled",
        ServiceStartOutcome::Failed => "service.start-failed",
    };
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    // Startup after process registration may already have endpoint evidence.
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type,
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: None,
            computed_manifest_hash: None,
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
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
        identity,
        redactor,
    } = registry.context()?;
    let process_command_json = redactor.redact_json_str(process.command_json)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, identity, run_id, computed_manifest_hash)?;
    require_open_sources(&transaction, run_id)?;
    ensure_predecessor_settled(&transaction, run_id)?;
    refuse_active_service(&transaction, service.service_instance_id)?;
    transaction
        .execute(
            "
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity,
              command_json, run_id, service_instance_id, status, service_name, role,
              source_label, presentation, stdout_path, stderr_path,
              stop_signal, stop_timeout_ms, containment
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'service', ?12, ?13, ?14, ?15,
                      ?16, ?17, ?18)
            ",
            params![
                process.process_key,
                identity.environment.as_str(),
                identity.slot,
                process.pid,
                process.pgid,
                process.start_identity,
                process_command_json,
                run_id,
                service.service_instance_id,
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
        ensure_no_active_port_transaction(&transaction, endpoint.address, endpoint.port)?;
        transaction
            .execute(
                "INSERT INTO ports (
                endpoint_key, environment, slot, service_instance_id, address, port,
                status, owner_process_key
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    endpoint.endpoint_key,
                    identity.environment,
                    identity.slot,
                    service.service_instance_id,
                    endpoint.address,
                    endpoint.port,
                    PortStatus::Reserved.as_str(),
                    process.process_key
                ],
            )
            .map_err(sql_error)?;
    }
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "run.admitted",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: None,
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json: "{}",
        },
    )?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "service.starting",
            run_id: Some(run_id),
            service_instance_id: Some(service.service_instance_id),
            process_key: Some(process.process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json: &process_command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub(crate) fn activate_service_ready(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_manifest_hash: &str,
    endpoints: &[VerifiedEndpointActivation<'_>],
    lifecycle: (&str, &str, &str),
) -> RuntimeResult<()> {
    let (operation_id, operation_class, terminal_success) = lifecycle;
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let stored = stored_endpoint_map(&read_open_endpoints(
        &transaction,
        Some(service_instance_id),
    )?);
    let expected = endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.endpoint_key.to_string(),
                (
                    endpoint.address.to_string(),
                    endpoint.port,
                    PortStatus::Reserved.as_str().to_string(),
                    process_key.to_string(),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if expected.len() != endpoints.len() || stored != expected {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "ready endpoint evidence mismatch for {service_instance_id}: stored {stored:?}, expected {expected:?}"
            ),
        ));
    }
    for endpoint in endpoints {
        let changed = transaction
            .execute(
                "
                UPDATE ports
                SET status = ?5
                WHERE endpoint_key = ?1
                  AND service_instance_id = ?2
                  AND address = ?3
                  AND port = ?4
                  AND status = ?6
                  AND owner_process_key = ?7
                ",
                params![
                    endpoint.endpoint_key,
                    service_instance_id,
                    endpoint.address,
                    endpoint.port,
                    PortStatus::Active.as_str(),
                    PortStatus::Reserved.as_str(),
                    process_key,
                ],
            )
            .map_err(sql_error)?;
        if changed != 1 {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "ready activation changed {changed} rows for endpoint {}",
                    endpoint.endpoint_key
                ),
            ));
        }
        insert_event(
            &transaction,
            identity,
            redactor,
            EventInsert {
                event_type: "port.owner-verified",
                run_id: Some(run_id),
                service_instance_id: Some(service_instance_id),
                process_key: Some(process_key),
                computed_manifest_hash: Some(computed_manifest_hash),
                payload_json: endpoint.ownership_json,
            },
        )?;
    }
    let changed_process = transaction
        .execute(
            "
            UPDATE processes
            SET status = ?4
            WHERE process_key = ?1 AND service_instance_id = ?2 AND status = ?3
            ",
            params![
                process_key,
                service_instance_id,
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
        identity,
        redactor,
        EventInsert {
            event_type: "service.probe-ready",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
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
        identity,
        redactor,
        EventInsert {
            event_type: "service.lifecycle.terminal",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json: &lifecycle_payload,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
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
    service_instance_id: &str,
    process_key: &str,
    computed_manifest_hash: &str,
    terminal: ServiceTerminal<'_>,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
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
    transaction
        .execute(
            "UPDATE processes SET status = ?2, ownership = ?3, capture = coalesce(?4, capture)
             WHERE process_key = ?1",
            params![
                process_key,
                process_status.as_str(),
                ownership.as_str(),
                capture.map(CaptureOutcome::as_str)
            ],
        )
        .map_err(sql_error)?;
    if ownership == Ownership::Settled {
        release_service_ports(&transaction, service_instance_id)?;
    }
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type,
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Append one event outside any other registry transition.
pub(crate) fn record_event(
    registry: &mut Registry,
    event_type: &str,
    run_id: Option<&str>,
    service_instance_id: Option<&str>,
    process_key: Option<&str>,
    computed_manifest_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    append_event(
        connection,
        identity,
        redactor,
        EventInsert {
            event_type,
            run_id,
            service_instance_id,
            process_key,
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json,
        },
    )?;
    Ok(())
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
    service_instance_id: &str,
    process: &ProcessRecord<'_>,
    computed_manifest_hash: &str,
    platform_start: Option<&str>,
    payload_json: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
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
                SET status = ?8, start_identity = ?7
                WHERE process_key = ?1
                  AND run_id = ?2
                  AND service_instance_id = ?3
                  AND pid = ?4
                  AND pgid = ?5
                  AND CAST(json_extract(start_identity, '$.pid') AS INTEGER) = ?4
                  AND CAST(json_extract(start_identity, '$.pgid') AS INTEGER) = ?5
                  AND json_extract(start_identity, '$.platformStart') IS ?6
                  AND CAST(json_extract(?7, '$.pid') AS INTEGER) = ?4
                  AND CAST(json_extract(?7, '$.pgid') AS INTEGER) = ?5
                  AND json_extract(?7, '$.platformStart') IS ?6
                  AND status IN ({})
                ",
                status::sql_in_list(status::PROCESS_ACTIVE)
            ),
            params![
                process.process_key,
                run_id,
                service_instance_id,
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
    require_run(&transaction, identity, run_id, computed_manifest_hash)?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "service.proc-escape",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process.process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Settle an unresolved terminal process after OS observation proves its
/// recorded process, group, and tracked descendants are gone (or were
/// terminated by the exclusive recovery owner). Endpoint evidence it still owns
/// is released with it; the terminal status is retained as history.
pub(crate) fn settle_unresolved_process(
    registry: &mut Registry,
    process_key: &str,
    run_id: &str,
    computed_manifest_hash: &str,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
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
                   AND status NOT IN ({})
                   AND EXISTS (
                     SELECT 1 FROM runs r
                     WHERE r.run_id = processes.run_id AND r.computed_manifest_hash = ?3
                   )",
                status::sql_in_list(status::PROCESS_ACTIVE)
            ),
            params![process_key, run_id, computed_manifest_hash],
        )
        .map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("unresolved process {process_key} no longer matches its recorded obligation"),
        ));
    }
    transaction
        .execute(
            &format!(
                "UPDATE ports SET status = ?2 WHERE owner_process_key = ?1 AND status IN ({})",
                status::sql_in_list(status::PORT_OPEN)
            ),
            params![process_key, PortStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "process.ownership-settled",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: Some(process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub(crate) fn ensure_service_instance_probe_ready(
    registry: &Registry,
    service_name: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let snapshot = read_service_snapshot(registry, service_instance_id)?;
    match snapshot.process.as_ref().map(|process| process.status) {
        Some(ProcessStatus::Ready) => Ok(()),
        Some(status) => Err(RuntimeError::new(
            ErrorCode::DependencyUnavailable,
            format!(
                "service {service_name} process is {}, not ready",
                status.as_str()
            ),
        )),
        None => Err(RuntimeError::new(
            ErrorCode::DependencyUnavailable,
            format!("service {service_name} has not been started"),
        )),
    }
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
        identity,
        redactor,
    } = registry.context()?;
    let command_json = redactor.redact_json_str(process.command_json)?;
    let transaction = connection.transaction().map_err(sql_error)?;
    require_run(
        &transaction,
        identity,
        process.run_id,
        process.computed_manifest_hash,
    )?;
    require_open_sources(&transaction, process.run_id)?;
    transaction
        .execute(
            "
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity,
              command_json, run_id, service_instance_id, status, role, service_name,
              source_label, presentation, stdout_path, stderr_path,
              stop_signal, stop_timeout_ms, containment
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                      ?16, ?17, ?18)
            ",
            params![
                process.process_key,
                identity.environment.as_str(),
                identity.slot,
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
        identity,
        redactor,
        EventInsert {
            event_type: match owner {
                InvocationOwner::Task => "task.running",
                InvocationOwner::Probe(_) => "probe.running",
            },
            run_id: Some(process.run_id),
            service_instance_id: None,
            process_key: Some(process.process_key),
            computed_manifest_hash: Some(process.computed_manifest_hash),
            payload_json: &command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
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
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    require_run(&transaction, identity, run_id, manifest_hash)?;
    let changed = transaction
        .execute(
            "UPDATE processes SET execution_outcome = ?3, exit_code = ?4
         WHERE process_key = ?1 AND run_id = ?2 AND service_instance_id IS NULL
           AND execution_outcome IS NULL AND environment = ?5 AND slot = ?6 AND role = ?7",
            params![
                process_key,
                run_id,
                outcome.as_str(),
                exit_code,
                identity.environment,
                identity.slot,
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
        identity,
        redactor,
        EventInsert {
            event_type: match owner {
                InvocationOwner::Task => "task.execution-observed",
                InvocationOwner::Probe(_) => "probe.execution-observed",
            },
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: Some(process_key),
            computed_manifest_hash: Some(manifest_hash),
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
        None,
        Some(invocation.process_key),
        invocation.manifest_hash,
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
        manifest_hash: computed_manifest_hash,
        owner,
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
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    let changed = transaction.execute(
        "UPDATE processes SET status = ?2, ownership = 'settled', capture = coalesce(?7, capture) WHERE process_key = ?1 AND run_id = ?3 AND role = ?4 AND environment = ?5 AND slot = ?6",
        params![process_key, process_status.as_str(), run_id, owner.role().as_str(), identity.environment, identity.slot, capture.map(CaptureOutcome::as_str)],
    ).map_err(sql_error)?;
    if changed != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "invocation settlement must update exactly one matching process",
        ));
    }
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type,
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: Some(process_key),
            computed_manifest_hash: Some(computed_manifest_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub(crate) fn read_service_snapshot(
    registry: &Registry,
    service_instance_id: &str,
) -> RuntimeResult<ServiceSnapshot> {
    read_service_snapshot_conn(registry.connection(), service_instance_id)
}

fn read_service_snapshot_conn(
    connection: &Connection,
    service_instance_id: &str,
) -> RuntimeResult<ServiceSnapshot> {
    let process_rows = {
        let mut statement = connection
            .prepare(&format!(
                "
                SELECT p.process_key, p.pid, p.pgid, p.start_identity, p.run_id, p.status, p.service_name
                FROM processes p
                WHERE p.service_instance_id = ?1
                  AND ({actionable})
                ORDER BY p.process_key
                ",
                actionable = status::actionable_process_sql(),
            ))
            .map_err(sql_error)?;
        statement
            .query_map(params![service_instance_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    if process_rows.len() > 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "service instance {service_instance_id} has {} actionable process rows",
                process_rows.len()
            ),
        ));
    }
    let process = process_rows
        .into_iter()
        .next()
        .map(
            |(
                process_key,
                pid,
                pgid,
                start_identity_json,
                run_id,
                process_status,
                service_name,
            )|
             -> RuntimeResult<StoredServiceProcess> {
                let start_identity = serde_json::from_str::<StoredProcessIdentity>(
                    &start_identity_json,
                )
                .map_err(|error| {
                    RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        format!("invalid start identity for process {process_key}: {error}"),
                    )
                })?;
                if service_name.is_empty() {
                    return Err(RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        "stored service label is empty",
                    ));
                }
                Ok(StoredServiceProcess {
                    service_name,
                    process_key,
                    pid,
                    pgid,
                    platform_start: start_identity.platform_start,
                    tracked_processes: start_identity.tracked_processes,
                    run_id,
                    status: ProcessStatus::parse_db(&process_status)?,
                })
            },
        )
        .transpose()?;

    let endpoints = read_open_endpoints(connection, Some(service_instance_id))?;

    Ok(ServiceSnapshot { process, endpoints })
}

fn stored_endpoint_map(
    endpoints: &[StoredServiceEndpoint],
) -> BTreeMap<String, (String, u16, String, String)> {
    endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.endpoint_key.clone(),
                (
                    endpoint.address.clone(),
                    endpoint.port,
                    endpoint.status.as_str().to_owned(),
                    endpoint.owner_process_key.clone(),
                ),
            )
        })
        .collect()
}

/// Closed source registration forbids every new process source for the run.
fn require_open_sources(transaction: &Transaction<'_>, run_id: &str) -> RuntimeResult<()> {
    let sources: String = transaction
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

fn require_run(
    transaction: &Transaction<'_>,
    identity: &RegistryIdentity,
    run_id: &str,
    computed_manifest_hash: &str,
) -> RuntimeResult<()> {
    let matches: i64 = transaction
        .query_row(
            "
            SELECT count(*)
            FROM runs
            WHERE run_id = ?1
              AND environment = ?2
              AND slot = ?3
              AND computed_manifest_hash = ?4
            ",
            params![
                run_id,
                identity.environment.as_str(),
                identity.slot,
                computed_manifest_hash,
            ],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if matches != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("run transition found {matches} exact rows for {run_id}, expected 1"),
        ));
    }
    Ok(())
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

/// A service instance starts at most once while an earlier process of it is
/// still an ownership obligation.
fn refuse_active_service(connection: &Connection, service_instance_id: &str) -> RuntimeResult<()> {
    let existing: Option<String> = connection
        .query_row(
            &format!(
                "
                SELECT p.status
                FROM processes p
                WHERE p.service_instance_id = ?1
                  AND ({actionable})
                ORDER BY p.process_key
                LIMIT 1
                ",
                actionable = status::actionable_process_sql(),
            ),
            params![service_instance_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    match existing {
        Some(status) => Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("service instance {service_instance_id} has actionable process {status}"),
        )),
        None => Ok(()),
    }
}

fn ensure_no_active_port_transaction(
    transaction: &Transaction<'_>,
    address: &str,
    port: u16,
) -> RuntimeResult<()> {
    let mut statement = transaction
        .prepare(
            "
            SELECT endpoint_key, status FROM ports
            WHERE address = ?1 AND port = ?2
            ORDER BY endpoint_key
            ",
        )
        .map_err(sql_error)?;
    let rows = statement
        .query_map(params![address, port], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql_error)?;
    for row in rows {
        let (endpoint_key, raw_status) = row.map_err(sql_error)?;
        if status::PORT_OPEN.contains(&PortStatus::parse_db(&raw_status)?) {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!("endpoint {endpoint_key} already has active port {address}:{port}"),
            ));
        }
    }
    Ok(())
}

fn release_service_ports(
    transaction: &rusqlite::Transaction<'_>,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    transaction
        .execute(
            "
            UPDATE ports
            SET status = ?2
            WHERE service_instance_id = ?1
            ",
            params![service_instance_id, PortStatus::Released.as_str()],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn json_error(error: serde_json::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    const RUN_ID: &str = "run-test";
    const MANIFEST_HASH: &str = "manifest-hash";
    const SERVICE_ID: &str = "service-instance";
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
            let identity = RegistryIdentity::default_slot("test-project", "test-abi", "test-tool");
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
            service_instance_id: SERVICE_ID,
            service_name: "service",
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
            endpoint_key: "service-instance:endpoint",
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
                  run_id, environment, slot, execution_outcome, manifest_path, computed_manifest_hash,
                  runtime_abi, toolchain_id, generator_json, target_json, source_json,
                  summary_path, owner_identity, diagnostic_path
                ) VALUES (?1, ?2, ?3, NULL, '/manifest', ?4, ?5, ?6, '{}', '{}', '{}', NULL, '{}', 'diagnostics.log')
                ",
                params![
                    run_id,
                    identity.environment,
                    identity.slot,
                    MANIFEST_HASH,
                    identity.runtime_abi,
                    identity.toolchain_id,
                ],
            )
            .expect("test run should insert");
    }

    fn record_intent(registry: &mut Registry, run_id: &str) {
        record_service_start_intent(registry, run_id, MANIFEST_HASH, SERVICE_ID)
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
        let error = fixture
            .registry
            .connection()
            .execute(
                "INSERT INTO ports (endpoint_key, environment, slot, service_instance_id,
                 address, port, status, owner_process_key)
             VALUES ('service-instance:endpoint', 'dev', 0, 'service-instance',
                 '127.0.0.1', 24222, 'reserved', NULL)",
                [],
            )
            .unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::ConstraintViolation)
        );
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
        for mutation in ["missing", "hash", "slot"] {
            let mut fixture = TestRegistry::new();
            if mutation != "missing" {
                insert_run(&fixture.registry, RUN_ID);
                fixture
                    .registry
                    .connection()
                    .execute_batch(match mutation {
                        "hash" => "UPDATE runs SET computed_manifest_hash = 'other'",
                        "slot" => "UPDATE runs SET slot = slot + 1",
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
            record_service_start_intent(&mut fixture.registry, RUN_ID, MANIFEST_HASH, SERVICE_ID)
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
        let process = read_service_snapshot(&fixture.registry, SERVICE_ID)
            .unwrap()
            .process
            .unwrap();
        assert_eq!(process.service_name, "service");
        assert_eq!(process.run_id, RUN_ID);
        assert_eq!(process.process_key, PROCESS_KEY);

        for mutation in [
            "UPDATE processes SET service_name = NULL",
            "UPDATE processes SET service_name = ''",
            "UPDATE processes SET service_instance_id = NULL",
            "UPDATE processes SET role = NULL",
            "UPDATE processes SET role = 'unknown'",
            "UPDATE processes SET role = 'task'",
            "UPDATE processes SET role = 'probe'",
        ] {
            assert!(
                fixture
                    .registry
                    .connection()
                    .execute_batch(mutation)
                    .is_err()
            );
        }
        settle_service_terminal(
            &mut fixture.registry,
            RUN_ID,
            SERVICE_ID,
            PROCESS_KEY,
            MANIFEST_HASH,
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
    fn snapshot_multiplicity_and_start_conflict_keep_distinct_precedence() {
        let mut fixture = TestRegistry::new();
        record_started(&mut fixture.registry);
        fixture
            .registry
            .connection()
            .execute_batch(
                "INSERT INTO processes (
                process_key, environment, slot, pid, pgid, start_identity,
                command_json, run_id, service_instance_id, status, service_name, role,
                source_label, presentation, stdout_path, stderr_path, stop_signal, stop_timeout_ms, containment
             ) SELECT 'z-second', environment, slot, pid, pgid, 'invalid-json',
                      command_json, run_id, service_instance_id, status, service_name, role,
                      source_label, presentation, stdout_path || '.2', stderr_path || '.2', stop_signal, stop_timeout_ms, containment FROM processes;",
            )
            .unwrap();
        let error = read_service_snapshot(&fixture.registry, SERVICE_ID).unwrap_err();
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        assert_eq!(
            error.message,
            "service instance service-instance has 2 actionable process rows"
        );
        let error =
            record_service_start_intent(&mut fixture.registry, RUN_ID, MANIFEST_HASH, SERVICE_ID)
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        assert_eq!(
            error.message,
            "service instance service-instance has actionable process running"
        );
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
                SERVICE_ID,
                PROCESS_KEY,
                MANIFEST_HASH,
                &[VerifiedEndpointActivation {
                    endpoint_key: endpoint().endpoint_key,
                    address,
                    port: endpoint().port,
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
        let snapshot = read_service_snapshot(&fixture.registry, SERVICE_ID).unwrap();
        assert_eq!(snapshot.endpoints[0].address, "0:0:0:0:0:0:0:1");
        assert_eq!(snapshot.endpoints[0].host.to_string(), "::1");
    }

    #[test]
    fn stored_endpoint_decoding_is_shared_by_ready_and_snapshot_reads() {
        for mutation in [
            "UPDATE ports SET endpoint_key = 'other:endpoint'",
            "UPDATE ports SET endpoint_key = service_instance_id || ':'",
            "UPDATE ports SET address = 'not-an-address'",
            "UPDATE ports SET address = '0.0.0.0'",
            "UPDATE ports SET port = -1",
            "UPDATE ports SET port = 65536",
        ] {
            for consumer in ["ready", "snapshot"] {
                let mut fixture = TestRegistry::new();
                record_started(&mut fixture.registry);
                fixture
                    .registry
                    .connection()
                    .execute_batch(mutation)
                    .unwrap();
                let before: (i64, i64, i64) = fixture
                    .registry
                    .connection()
                    .query_row(
                        "SELECT (SELECT count(*) FROM events), (SELECT count(*) FROM processes),
                            (SELECT count(*) FROM sqlite_master WHERE name = 'services')",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .unwrap();
                let result = match consumer {
                    "ready" => activate_service_ready(
                        &mut fixture.registry,
                        RUN_ID,
                        SERVICE_ID,
                        PROCESS_KEY,
                        MANIFEST_HASH,
                        &[VerifiedEndpointActivation {
                            endpoint_key: endpoint().endpoint_key,
                            address: "127.0.0.1",
                            port: 24222,
                            ownership_json: "{}",
                        }],
                        ("service.ready", "ready", "ready"),
                    ),
                    "snapshot" => read_service_snapshot(&fixture.registry, SERVICE_ID).map(|_| ()),
                    _ => unreachable!(),
                };
                assert_eq!(
                    result.unwrap_err().code,
                    ErrorCode::RegistryCorrupt,
                    "{consumer}: {mutation}"
                );
                let after: (i64, i64, i64) = fixture
                    .registry
                    .connection()
                    .query_row(
                        "SELECT (SELECT count(*) FROM events), (SELECT count(*) FROM processes),
                            (SELECT count(*) FROM sqlite_master WHERE name = 'services')",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .unwrap();
                assert_eq!(after, before, "{consumer}: {mutation}");
            }
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
    fn process_registration_checks_every_matching_endpoint_before_writing() {
        for (later_status, expected_code) in [
            ("reserved", ErrorCode::RegistryCorrupt),
            ("active", ErrorCode::RegistryCorrupt),
            ("invalid-status", ErrorCode::RegistryCorrupt),
        ] {
            let mut fixture = TestRegistry::new();
            insert_run(&fixture.registry, RUN_ID);
            let registry_identity = fixture.registry.identity();
            fixture
                .registry
                .connection()
                .execute(
                    "
                    INSERT INTO ports (
                      endpoint_key, environment, slot, service_instance_id, address, port, status, owner_process_key
                    ) VALUES
                      ('a:old', ?1, ?2, 'old-service', ?3, ?4, 'released', 'old-process'),
                      ('z:existing', ?1, ?2, 'existing-service', ?3, ?4, ?5, 'existing-process')
                    ",
                    params![
                        registry_identity.environment,
                        registry_identity.slot,
                        endpoint().address,
                        endpoint().port,
                        later_status,
                    ],
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
            .expect_err("a released row must not hide a later open or malformed row");
            assert_eq!(error.code, expected_code, "status {later_status}");
            let counts: (i64, i64, i64) = fixture
                .registry
                .connection()
                .query_row(
                    "SELECT (SELECT count(*) FROM ports),
                            (SELECT count(*) FROM processes),
                            (SELECT count(*) FROM events)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(counts, (2, 0, 0), "status {later_status}");
        }
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
                "missing-process" => {
                    fixture
                        .registry
                        .connection()
                        .execute(
                            "DELETE FROM processes WHERE process_key = ?1",
                            [PROCESS_KEY],
                        )
                        .unwrap();
                }
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
            let process = process_record(if mutation == "wrong-process" {
                124
            } else {
                123
            });
            let error = mark_process_escape(
                &mut fixture.registry,
                RUN_ID,
                SERVICE_ID,
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
                      (SELECT count(*) FROM ports WHERE status = 'reserved'),
                      (SELECT count(*) FROM events WHERE event_type = 'service.proc-escape')
                    ",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            assert_eq!(state, ((mutation != "missing-process") as i64, 1, 1, 0));
        }
    }
}
