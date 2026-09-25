use crate::registry::records::{StoredEndpoint as PortRow, read_open_endpoints};
use crate::registry::sqlite::RegistryContext;
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use crate::cancellation::CancellationToken;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::events::{EventInsert, insert_event};
use crate::registry::session::{record_interrupted_sessions, record_recovered_sessions};
use crate::registry::status::{self, DbStatus, PortStatus, ProcessRole, ProcessStatus};
use crate::registry::{Registry, RegistryIdentity, RegistryReader};
use crate::service::{
    ProcessRecord, StopPolicy, StoredProcessIdentity, TaskTerminalStatus, mark_process_escape,
    mark_service_stopped, mark_task_finished, process_escape_start_identity,
    process_group_has_live_member, process_is_live_with_identity,
    process_is_live_with_start_identity, process_present, settle_unresolved_process,
    terminate_process_group_signal, terminate_process_tree_with_snapshot,
};
use crate::session_control::{CancellationDelivery, request_cancellation};
use crate::state::HostPlacement;
use crate::state::ownership::SlotGuard;
use crate::state::{RetentionOutcome, StateIdentity, apply_retention};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PsReport {
    pub processes: Vec<ProcessObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessObservation {
    pub process_key: String,
    pub run_id: String,
    pub service_instance_id: Option<String>,
    pub pid: u32,
    pub pgid: i32,
    pub registry_status: String,
    /// The recorded status as the host currently observes it.
    pub observed_status: String,
    /// `unresolved` until the owner or a recovery successor proved the
    /// process, its group, and its tracked descendants are gone.
    pub ownership: String,
    pub live: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownReport {
    /// The live session that received this command's cancellation request and
    /// then settled; absent when no live owner was reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canceled_run_id: Option<String>,
    pub stopped: Vec<String>,
    pub stale: Vec<String>,
}

/// Request cancellation of the slot's live session through its own FIFO and
/// observe that same session until it settles; otherwise recover a dead owner
/// under exclusive slot authority. A request never targets a successor, and a
/// timeout reports incomplete shutdown without signaling anything itself.
pub fn down(
    placement: &HostPlacement,
    registry_identity: &RegistryIdentity,
    state: &StateIdentity,
    timeout_ms: u64,
) -> RuntimeResult<DownReport> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let registry_path = placement.registry_path();
    if RegistryReader::open_existing(&registry_path, registry_identity)?.is_none()
        && !placement.registry_dir.join("slot.lock").exists()
    {
        return Ok(DownReport::default());
    }
    let mut selected: Option<String> = None;
    let mut requested = false;
    loop {
        if selected.is_none()
            && let Some(reader) = RegistryReader::open_existing(&registry_path, registry_identity)?
        {
            selected = latest_unfinished_session(&reader)?;
            if let Some(run_id) = &selected {
                requested =
                    request_cancellation(&placement.registry_dir.join("runs").join(run_id))?
                        == CancellationDelivery::Requested;
            }
        }
        // The chosen session ends `down` once it settles, whether its own owner
        // or a successor's recovery settled it.
        if let Some(run_id) = &selected
            && let Some(reader) = RegistryReader::open_existing(&registry_path, registry_identity)?
            && session_settled(&reader, run_id)?
        {
            return Ok(DownReport {
                canceled_run_id: selected.filter(|_| requested),
                ..DownReport::default()
            });
        }
        if let Some(guard) = SlotGuard::try_acquire(placement, &CancellationToken::new())? {
            let mut registry = Registry::open_or_create(guard, registry_identity)?;
            let recovered = recover_slot(&mut registry, &placement.state_base, state, timeout_ms);
            let closed = registry.close();
            let mut report = match (recovered, closed) {
                (Ok(report), Ok(())) => report.down,
                (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
                (Err(error), Err(close)) => return Err(error.with_cause(close)),
            };
            report.canceled_run_id = selected.filter(|_| requested);
            return Ok(report);
        }
        if std::time::Instant::now() >= deadline {
            let mut error = RuntimeError::new(
                ErrorCode::LifecycleFailed,
                "the slot's session did not finish before the down timeout; its owner still holds the slot",
            );
            if let Some(run_id) = &selected {
                error = error.with_detail("runId", run_id);
            }
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn latest_unfinished_session(reader: &RegistryReader) -> RuntimeResult<Option<String>> {
    reader
        .connection()
        .query_row(
            "SELECT run_id FROM runs WHERE finalization = 'unfinished' ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)
}

fn session_settled(reader: &RegistryReader, run_id: &str) -> RuntimeResult<bool> {
    reader
        .connection()
        .query_row(
            "SELECT finalization = 'complete' FROM runs WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .map_err(sql_error)
}

pub fn ps(registry: &RegistryReader) -> RuntimeResult<PsReport> {
    let processes = process_rows(registry.connection())?
        .into_iter()
        .map(|row| {
            let active = status::PROCESS_ACTIVE.contains(&row.status);
            let live = if active || row.unsettled {
                row.observed_liveness()?
            } else {
                false
            };
            let observed_status = if active {
                if live {
                    ProcessStatus::Running
                } else {
                    ProcessStatus::Stale
                }
            } else {
                row.status
            };
            Ok(ProcessObservation {
                process_key: row.process_key,
                run_id: row.run_id,
                service_instance_id: row.service_instance_id,
                pid: row.pid,
                pgid: row.pgid,
                registry_status: row.status.as_str().to_string(),
                observed_status: observed_status.as_str().to_string(),
                ownership: if active || row.unsettled {
                    "unresolved"
                } else {
                    "settled"
                }
                .to_string(),
                live,
            })
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    Ok(PsReport { processes })
}

/// Recovery's first pass under slot authority: settle every recorded process
/// proven gone and release endpoint evidence no live owner holds. Returns the
/// process keys it settled.
fn settle_dead_processes(registry: &mut Registry) -> RuntimeResult<Vec<String>> {
    registry.authority().validate()?;
    read_open_endpoints(registry.connection(), None)?;
    let rows = process_rows(registry.connection())?;
    let mut settled = Vec::new();
    for row in rows {
        let active = status::PROCESS_ACTIVE.contains(&row.status);
        let live = if active || row.unsettled {
            row.observed_liveness()?
        } else {
            false
        };
        if active && !live {
            mark_process_stale(registry, &row)?;
            settled.push(row.process_key);
        } else if row.unsettled && !live {
            settle_unsettled(registry, &row)?;
            settled.push(row.process_key);
        }
    }
    let rows = process_rows(registry.connection())?;
    release_orphaned_endpoints(registry, &rows)?;
    Ok(settled)
}

/// State preparation never performs teardown. The slot owner must first settle
/// all recorded process and endpoint obligations through recovery.
pub(crate) fn require_settled_slot(registry: &Registry) -> RuntimeResult<()> {
    registry.authority().validate()?;
    let processes = process_rows(registry.connection())?;
    let endpoints = read_open_endpoints(registry.connection(), None)?;
    if processes
        .iter()
        .any(|row| status::PROCESS_ACTIVE.contains(&row.status) || row.unsettled)
        || !endpoints.is_empty()
    {
        return Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "state preparation requires settled predecessor processes and endpoints",
        ));
    }
    Ok(())
}

pub fn down_owned_process_groups(
    registry: &mut Registry,
    timeout_ms: u64,
) -> RuntimeResult<DownReport> {
    let mut stale = settle_dead_processes(registry)?;
    let rows = process_rows(registry.connection())?;
    let mut stopped = Vec::new();
    for row in rows
        .into_iter()
        .filter(|row| status::PROCESS_ACTIVE.contains(&row.status) || row.unsettled)
    {
        if !row.observed_liveness()? {
            if row.unsettled {
                settle_unsettled(registry, &row)?;
            } else {
                mark_process_stale(registry, &row)?;
            }
            stale.push(row.process_key);
            continue;
        }
        if row.unsettled {
            terminate_process_tree_with_snapshot(
                row.pid,
                row.pgid,
                row.stop.signal,
                row.stop.timeout_ms.min(timeout_ms),
                &row.start_identity.tracked_processes,
            )?;
            settle_down_process(registry, &row)?;
            stopped.push(row.process_key);
            continue;
        }
        let escape_start_identity = row.service_instance_id.as_ref().map(|_| {
            process_escape_start_identity(
                row.pid,
                row.pgid,
                row.start_identity.platform_start.as_deref(),
                &row.start_identity.tracked_processes,
            )
        });
        // Terminate with the recorded policy; the command timeout only caps it.
        let stop_timeout = row.stop.timeout_ms.min(timeout_ms);
        let terminated = if row.stop.tree {
            terminate_process_tree_with_snapshot(
                row.pid,
                row.pgid,
                row.stop.signal,
                stop_timeout,
                &row.start_identity.tracked_processes,
            )
            .map(|_| ())
        } else {
            terminate_process_group_signal(row.pgid, row.stop.signal, stop_timeout).map(|_| ())
        };
        if let Err(error) = terminated {
            return Err(settle_control_escape(
                registry,
                &row,
                escape_start_identity.as_deref(),
                error,
            ));
        }
        // Group termination cannot reach descendants that left the group. An
        // exiting leader may briefly remain observable, so wait a bounded time.
        let settle_deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut live = row.observed_liveness()?;
        while live && std::time::Instant::now() < settle_deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            live = row.observed_liveness()?;
        }
        if live {
            return Err(settle_control_escape(
                registry,
                &row,
                escape_start_identity.as_deref(),
                RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!(
                        "process {} still has a live tracked descendant after group termination",
                        row.process_key
                    ),
                ),
            ));
        }
        settle_down_process(registry, &row)?;
        stopped.push(row.process_key);
    }
    Ok(DownReport {
        canceled_run_id: None,
        stopped,
        stale,
    })
}

/// What exclusive predecessor recovery settled before any new work.
#[derive(Debug)]
pub struct RecoveryReport {
    pub down: DownReport,
    pub retention: RetentionOutcome,
}

/// The exclusive successor's settlement of every interrupted or unfinished
/// predecessor: record unknown outcomes as interrupted, stop recorded process
/// obligations, resume pending deletion, apply the predecessor tree's own
/// retention, then complete the predecessors' finalization. It never resumes
/// tasks or adopts services; any unsafe step refuses and blocks new work.
pub fn recover_slot(
    registry: &mut Registry,
    state_base: &Path,
    identity: &StateIdentity,
    timeout_ms: u64,
) -> RuntimeResult<RecoveryReport> {
    record_interrupted_sessions(registry)?;
    let down = down_owned_process_groups(registry, timeout_ms)?;
    let retention = apply_retention(state_base, identity, registry)?;
    record_recovered_sessions(registry)?;
    Ok(RecoveryReport { down, retention })
}

#[derive(Debug)]
struct ProcessRow {
    role: ProcessRole,
    service_name: Option<String>,
    process_key: String,
    pid: u32,
    pgid: i32,
    start_identity: StoredProcessIdentity,
    command_json: String,
    run_id: String,
    service_instance_id: Option<String>,
    status: ProcessStatus,
    computed_manifest_hash: String,
    unsettled: bool,
    stop: StopPolicy,
}

impl ProcessRow {
    fn is_live(&self) -> RuntimeResult<bool> {
        process_is_live_with_identity(
            self.pid,
            self.pgid,
            self.start_identity.platform_start.as_deref(),
        )
    }

    /// A recorded process is gone only when its leader, every member of its
    /// process group, and every tracked descendant are gone. Leader exit alone
    /// never settles ownership.
    fn observed_liveness(&self) -> RuntimeResult<bool> {
        if self.is_live()? {
            return Ok(true);
        }
        // An escaped leader may have left its recorded group.
        if self.unsettled
            && process_is_live_with_start_identity(
                self.pid,
                self.start_identity.platform_start.as_deref(),
            )?
        {
            return Ok(true);
        }
        // A PID is never reused while its process group exists. So a present
        // leader with another identity means reuse, and that group is not ours;
        // an absent leader leaves any surviving group members ours.
        if !process_present(self.pid)? && process_group_has_live_member(self.pgid)? {
            return Ok(true);
        }
        for tracked in &self.start_identity.tracked_processes {
            if tracked.platform_start.is_none() {
                return Err(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!(
                        "escaped process {} has an identity-unconfirmed tracked descendant {}",
                        self.process_key, tracked.pid
                    ),
                ));
            }
            if process_is_live_with_start_identity(tracked.pid, tracked.platform_start.as_deref())?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn settle_control_escape(
    registry: &mut Registry,
    row: &ProcessRow,
    start_identity: Option<&str>,
    termination_error: RuntimeError,
) -> RuntimeError {
    let (Some(service_instance_id), Some(start_identity)) =
        (row.service_instance_id.as_deref(), start_identity)
    else {
        return termination_error;
    };
    let payload = serde_json::json!({
        "pid": row.pid,
        "pgid": row.pgid,
        "errorCode": termination_error.code,
        "message": termination_error.message.as_str(),
        "terminationError": termination_error.message.as_str(),
    })
    .to_string();
    let process = ProcessRecord {
        process_key: &row.process_key,
        pid: row.pid,
        pgid: row.pgid,
        start_identity,
        command_json: &row.command_json,
    };
    match mark_process_escape(
        registry,
        &row.run_id,
        service_instance_id,
        &process,
        &row.computed_manifest_hash,
        row.start_identity.platform_start.as_deref(),
        &payload,
    ) {
        Ok(()) => RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to prove termination of service process group {}: {}",
                row.pgid, termination_error.message
            ),
        ),
        Err(settlement_error) => settlement_error,
    }
}

fn process_rows(connection: &rusqlite::Connection) -> RuntimeResult<Vec<ProcessRow>> {
    let mut statement = connection
        .prepare(&format!(
            "
            SELECT
              p.process_key, p.pid, p.pgid, p.start_identity, p.command_json,
              p.run_id, p.service_instance_id, p.status, r.computed_manifest_hash,
              CASE WHEN {escaped_process}
                   THEN 1 ELSE 0 END, p.role, p.service_name,
              p.stop_signal, p.stop_timeout_ms, p.containment
            FROM processes p
            LEFT JOIN runs r ON r.run_id = p.run_id
            ORDER BY p.process_key
            ",
            escaped_process = status::unsettled_terminal_sql(),
        ))
        .map_err(sql_error)?;
    let rows = statement
        .query_map([], |row| {
            let start_identity: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, i32>(2)?,
                start_identity,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)? != 0,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
                (
                    row.get::<_, i32>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, String>(14)?,
                ),
            ))
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    rows.into_iter()
        .map(
            |(
                process_key,
                pid,
                pgid,
                start_identity_json,
                command_json,
                run_id,
                service_instance_id,
                status,
                computed_manifest_hash,
                unsettled,
                role,
                service_name,
                (stop_signal, stop_timeout_ms, containment),
            )| {
                let role = ProcessRole::parse_db(&role)?;
                let stop = StopPolicy {
                    signal: stop_signal,
                    timeout_ms: u64::try_from(stop_timeout_ms).map_err(|_| {
                        RuntimeError::new(ErrorCode::RegistryCorrupt, "invalid stop timeout")
                    })?,
                    tree: match containment.as_str() {
                        "process-group" => false,
                        "process-tree" => true,
                        _ => {
                            return Err(RuntimeError::new(
                                ErrorCode::RegistryCorrupt,
                                "invalid process containment",
                            ));
                        }
                    },
                };
                let coherent = match role {
                    ProcessRole::Task => service_instance_id.is_none() && service_name.is_none(),
                    ProcessRole::Service => {
                        service_instance_id.is_some()
                            && service_name.as_ref().is_some_and(|name| !name.is_empty())
                    }
                    ProcessRole::Probe => {
                        service_instance_id.is_none()
                            && service_name.as_ref().is_some_and(|name| !name.is_empty())
                    }
                };
                if !coherent {
                    return Err(RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        "process role and service attribution disagree",
                    ));
                }
                let start_identity = serde_json::from_str::<StoredProcessIdentity>(
                    &start_identity_json,
                )
                .map_err(|error| {
                    RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        format!("invalid start identity for process {process_key}: {error}"),
                    )
                })?;
                Ok(ProcessRow {
                    role,
                    service_name,
                    process_key,
                    pid,
                    pgid,
                    start_identity,
                    command_json,
                    run_id,
                    service_instance_id,
                    status: ProcessStatus::parse_db(&status)?,
                    computed_manifest_hash,
                    unsettled,
                    stop,
                })
            },
        )
        .collect()
}

fn mark_process_stale(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = ?2, ownership = 'settled' WHERE process_key = ?1",
            params![row.process_key, ProcessStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    if let Some(service_instance_id) = row.service_instance_id.as_deref() {
        transaction
            .execute(
                &format!(
                    "
                UPDATE ports
                SET status = ?2
                WHERE service_instance_id = ?1
                  AND status IN ({})
                ",
                    status::sql_in_list(status::PORT_OPEN)
                ),
                params![service_instance_id, PortStatus::Stale.as_str()],
            )
            .map_err(sql_error)?;
    }
    let payload_json = serde_json::json!({
        "pid": row.pid,
        "pgid": row.pgid,
        "previousStatus": row.status.as_str(),
        "command": row.command_json,
    })
    .to_string();
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "process.stale",
            run_id: Some(&row.run_id),
            service_instance_id: row.service_instance_id.as_deref(),
            process_key: Some(&row.process_key),
            computed_manifest_hash: Some(&row.computed_manifest_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn release_orphaned_endpoints(
    registry: &mut Registry,
    processes: &[ProcessRow],
) -> RuntimeResult<()> {
    let ports = read_open_endpoints(registry.connection(), None)?;
    for port in ports {
        let mut proof_process = None;
        let mut live_owner = false;
        for process in processes
            .iter()
            .filter(|process| port.is_owned_by_process(process))
        {
            if process.observed_liveness()? {
                live_owner = true;
                break;
            }
            if proof_process.is_none() {
                proof_process = Some(process);
            }
        }
        if live_owner {
            continue;
        }
        if let Some(process) = proof_process {
            mark_port_stale(registry, &port, process)?;
        }
    }
    Ok(())
}

impl PortRow {
    fn is_owned_by_process(&self, process: &ProcessRow) -> bool {
        self.owner_process_key == process.process_key
            && process.service_instance_id.as_deref() == Some(self.service_instance_id.as_str())
    }
}

fn mark_port_stale(
    registry: &mut Registry,
    port: &PortRow,
    process: &ProcessRow,
) -> RuntimeResult<()> {
    let RegistryContext {
        connection,
        identity,
        redactor,
    } = registry.context()?;
    let transaction = connection.transaction().map_err(sql_error)?;
    transaction
        .execute(
            &format!(
                "
            UPDATE ports
            SET status = ?2
            WHERE endpoint_key = ?1
              AND status IN ({})
            ",
                status::sql_in_list(status::PORT_OPEN)
            ),
            params![port.endpoint_key.as_str(), PortStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    let payload_json = serde_json::json!({
        "endpointKey": port.endpoint_key.as_str(),
        "address": port.address.as_str(),
        "port": port.port,
        "previousStatus": port.status.as_str(),
        "ownerProcessKey": port.owner_process_key.as_str(),
    })
    .to_string();
    insert_event(
        &transaction,
        identity,
        redactor,
        EventInsert {
            event_type: "port.stale",
            run_id: Some(&process.run_id),
            service_instance_id: Some(&port.service_instance_id),
            process_key: Some(&process.process_key),
            computed_manifest_hash: Some(&process.computed_manifest_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn mark_stopped(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    if let Some(service_instance_id) = row.service_instance_id.as_deref() {
        // Recovery proves process death, never a predecessor's capture outcome.
        return mark_service_stopped(
            registry,
            &row.run_id,
            service_instance_id,
            &row.process_key,
            &row.computed_manifest_hash,
            None,
        );
    }
    let payload_json = serde_json::json!({
        "pid": row.pid,
        "pgid": row.pgid,
        "reason": "down",
        "command": row.command_json,
    })
    .to_string();
    if row.role == ProcessRole::Probe {
        return crate::service::mark_invocation_finished(
            registry,
            crate::service::InvocationIdentity {
                run_id: &row.run_id,
                process_key: &row.process_key,
                manifest_hash: &row.computed_manifest_hash,
                owner: crate::service::InvocationOwner::Probe(
                    row.service_name
                        .as_deref()
                        .expect("probe attribution decoded"),
                ),
            },
            TaskTerminalStatus::Canceled,
            &payload_json,
            None,
        );
    }
    mark_task_finished(
        registry,
        &row.run_id,
        &row.process_key,
        &row.computed_manifest_hash,
        TaskTerminalStatus::Canceled,
        &payload_json,
        None,
    )
}

fn settle_down_process(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    if row.unsettled {
        settle_unsettled(registry, row)
    } else {
        mark_stopped(registry, row)
    }
}

fn settle_unsettled(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    settle_unresolved_process(
        registry,
        &row.process_key,
        &row.run_id,
        &row.computed_manifest_hash,
    )
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("control registry operation failed: {error}"),
    )
}
