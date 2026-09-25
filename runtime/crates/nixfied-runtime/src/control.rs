use crate::registry::records::{StoredEndpoint as PortRow, read_open_endpoints};
use crate::registry::sqlite::RegistryContext;
use std::path::Path;

use rusqlite::params;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::events::{EventInsert, insert_event};
use crate::registry::status::{self, DbStatus, PortStatus, ProcessRole, ProcessStatus};
use crate::registry::{Registry, RegistryReader};
use crate::service::{
    ProcessRecord, StoredProcessIdentity, TaskTerminalStatus, mark_process_escape,
    mark_service_stopped, mark_task_finished, process_escape_start_identity,
    process_group_has_live_member, process_is_live_with_identity,
    process_is_live_with_start_identity, release_unresolved_escape_ports, terminate_process_group,
    terminate_process_tree_with_snapshot,
};
use crate::state::{CleanupMode, CleanupOutcome, StateIdentity, clean_marked_state};

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
    pub reconciled_status: String,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownReport {
    pub stopped: Vec<String>,
    pub stale: Vec<String>,
}

/// A reconciled observation is evidence at a moment, not authority to signal.
pub struct ReconciledProcess {
    row: ProcessRow,
}

pub fn ps(registry: &RegistryReader) -> RuntimeResult<PsReport> {
    let processes = process_rows(registry.connection())?
        .into_iter()
        .map(|row| {
            let active = status::PROCESS_ACTIVE.contains(&row.status);
            let live = if active || row.unresolved_escape {
                row.reconciled_liveness()?
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
                reconciled_status: observed_status.as_str().to_string(),
                live,
            })
        })
        .collect::<RuntimeResult<Vec<_>>>()?;
    Ok(PsReport { processes })
}

pub fn reconcile_registry(registry: &mut Registry) -> RuntimeResult<Vec<ReconciledProcess>> {
    registry.authority().validate()?;
    read_open_endpoints(registry.connection(), None)?;
    let rows = process_rows(registry.connection())?;
    for row in rows {
        let active = status::PROCESS_ACTIVE.contains(&row.status);
        let live = if active || row.unresolved_escape {
            row.reconciled_liveness()?
        } else {
            false
        };
        if active && !live {
            mark_process_stale(registry, &row)?;
        } else if row.unresolved_escape && !live {
            reconcile_unresolved_escape(registry, &row)?;
        }
    }
    let rows = process_rows(registry.connection())?;
    reconcile_stale_port_reservations(registry, &rows)?;

    Ok(process_rows(registry.connection())?
        .into_iter()
        .map(|row| ReconciledProcess { row })
        .collect())
}

/// State preparation never performs teardown. The slot owner must first settle
/// all recorded process and endpoint obligations through recovery.
pub(crate) fn require_settled_slot(registry: &Registry) -> RuntimeResult<()> {
    registry.authority().validate()?;
    let processes = process_rows(registry.connection())?;
    let endpoints = read_open_endpoints(registry.connection(), None)?;
    if processes
        .iter()
        .any(|row| status::PROCESS_ACTIVE.contains(&row.status) || row.unresolved_escape)
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
    let reconciled = reconcile_registry(registry)?;
    let mut stale = reconciled
        .into_iter()
        .filter(|process| process.row.status == ProcessStatus::Stale)
        .map(|process| process.row.process_key)
        .collect::<Vec<_>>();
    let rows = process_rows(registry.connection())?;
    let mut stopped = Vec::new();
    for row in rows
        .into_iter()
        .filter(|row| status::PROCESS_ACTIVE.contains(&row.status) || row.unresolved_escape)
    {
        if !row.reconciled_liveness()? {
            if row.unresolved_escape {
                reconcile_unresolved_escape(registry, &row)?;
            } else {
                mark_process_stale(registry, &row)?;
            }
            stale.push(row.process_key);
            continue;
        }
        if row.unresolved_escape {
            terminate_process_tree_with_snapshot(
                row.pid,
                row.pgid,
                libc::SIGTERM,
                timeout_ms,
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
        if let Err(error) = terminate_process_group(row.pgid, timeout_ms) {
            return Err(settle_control_escape(
                registry,
                &row,
                escape_start_identity.as_deref(),
                error,
            ));
        }
        settle_down_process(registry, &row)?;
        stopped.push(row.process_key);
    }
    Ok(DownReport { stopped, stale })
}

pub fn clean_reconciled_state(
    registry: &mut Registry,
    state_base: &Path,
    identity: &StateIdentity,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    reconcile_registry(registry)?;
    clean_marked_state(state_base, identity, registry, mode)
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
    unresolved_escape: bool,
}

impl ProcessRow {
    fn is_live(&self) -> RuntimeResult<bool> {
        process_is_live_with_identity(
            self.pid,
            self.pgid,
            self.start_identity.platform_start.as_deref(),
        )
    }

    fn reconciled_liveness(&self) -> RuntimeResult<bool> {
        if !self.unresolved_escape {
            return self.is_live();
        }
        if process_is_live_with_start_identity(
            self.pid,
            self.start_identity.platform_start.as_deref(),
        )? || process_group_has_live_member(self.pgid)?
        {
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
                   THEN 1 ELSE 0 END, p.role, p.service_name
            FROM processes p
            LEFT JOIN runs r ON r.run_id = p.run_id
            ORDER BY p.process_key
            ",
            escaped_process = status::unresolved_escape_sql(),
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
                unresolved_escape,
                role,
                service_name,
            )| {
                let role = ProcessRole::parse_db(&role)?;
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
                    unresolved_escape,
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
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
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

fn reconcile_stale_port_reservations(
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
            if process.reconciled_liveness()? {
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
        return mark_service_stopped(
            registry,
            &row.run_id,
            service_instance_id,
            &row.process_key,
            &row.computed_manifest_hash,
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
        );
    }
    mark_task_finished(
        registry,
        &row.run_id,
        &row.process_key,
        &row.computed_manifest_hash,
        TaskTerminalStatus::Canceled,
        &payload_json,
    )
}

fn settle_down_process(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    if row.unresolved_escape {
        reconcile_unresolved_escape(registry, row)
    } else {
        mark_stopped(registry, row)
    }
}

fn reconcile_unresolved_escape(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    let service_instance_id = row.service_instance_id.as_deref().ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "escaped process {} has open ports but no service instance",
                row.process_key
            ),
        )
    })?;
    release_unresolved_escape_ports(
        registry,
        &row.process_key,
        &row.run_id,
        service_instance_id,
        &row.computed_manifest_hash,
    )
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("control registry operation failed: {error}"),
    )
}
