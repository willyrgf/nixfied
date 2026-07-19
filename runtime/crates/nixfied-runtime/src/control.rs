use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use nixfied_model::ServiceLifetime;
use rusqlite::params;
use serde::Serialize;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::status::{
    self, DbStatus, PortStatus, ProcessStatus, RunLeaseStatus, RunStatus, ServiceStatus,
};
use crate::registry::{Registry, RegistryIdentity};
use crate::service::StoredProcessIdentity;
use crate::service::process::{
    process_group_has_live_member, process_is_live_with_identity,
    process_is_live_with_start_identity, signal_process_group,
    terminate_process_tree_with_snapshot,
};
use crate::service::registry::{
    TaskTerminalStatus, mark_service_stopped, mark_task_finished, release_unresolved_escape_ports,
    service_lifetime_as_str,
};
use crate::state::{CleanupMode, CleanupOutcome, StateIdentity, clean_marked_state};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PsReport {
    pub processes: Vec<ProcessObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessObservation {
    pub process_key: String,
    pub run_id: String,
    pub service_instance_id: Option<String>,
    pub pid: u32,
    pub pgid: i32,
    pub registry_status: String,
    pub reconciled_status: String,
    pub service_status: Option<String>,
    pub service_lifetime: Option<String>,
    pub borrower_count: i64,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownReport {
    pub stopped: Vec<String>,
    pub stale: Vec<String>,
}

pub fn ps(registry: &mut Registry) -> RuntimeResult<PsReport> {
    reconcile_registry(registry)
}

pub fn reconcile_registry(registry: &mut Registry) -> RuntimeResult<PsReport> {
    let rows = process_rows(registry)?;
    for row in rows {
        let active = is_active_status(&row.status);
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
    reconcile_expired_run_leases(registry)?;
    reconcile_stale_port_reservations(registry)?;
    reconcile_service_lifetime_statuses(registry)?;
    reconcile_until_idle_services(registry)?;
    reconcile_service_lifetime_statuses(registry)?;

    let rows = process_rows(registry)?;
    let mut observations = Vec::with_capacity(rows.len());
    for row in rows {
        let active = is_active_status(&row.status);
        let live = if active || row.unresolved_escape {
            row.reconciled_liveness()?
        } else {
            false
        };
        let reconciled_status = if active && live {
            ProcessStatus::Running.as_str().to_string()
        } else {
            row.status.clone()
        };
        let borrower_count = match row.service_instance_id.as_deref() {
            Some(service_instance_id) => {
                active_borrower_count(registry, service_instance_id, &row.run_id)?
            }
            None => 0,
        };
        observations.push(ProcessObservation {
            process_key: row.process_key,
            run_id: row.run_id,
            service_instance_id: row.service_instance_id,
            pid: row.pid,
            pgid: row.pgid,
            registry_status: row.status,
            reconciled_status,
            service_status: row.service_status,
            service_lifetime: row.service_lifetime,
            borrower_count,
            live,
        });
    }
    Ok(PsReport {
        processes: observations,
    })
}

/// Which registry processes a teardown acts on.
#[derive(Debug, Clone, Copy)]
pub enum ProcessFilter<'a> {
    All,
    /// Only processes started by a run of a different model hash — the
    /// upgrade path's teardown of what an older model build left running.
    ModelHashNot(&'a str),
}

impl ProcessFilter<'_> {
    fn matches(&self, row: &ProcessRow) -> bool {
        match self {
            ProcessFilter::All => true,
            ProcessFilter::ModelHashNot(hash) => row.computed_model_hash != *hash,
        }
    }
}

pub fn down_owned_process_groups(
    registry: &mut Registry,
    timeout_ms: u64,
) -> RuntimeResult<DownReport> {
    down_processes(registry, timeout_ms, ProcessFilter::All)
}

pub fn down_processes(
    registry: &mut Registry,
    timeout_ms: u64,
    filter: ProcessFilter<'_>,
) -> RuntimeResult<DownReport> {
    // Reconciliation marks dead rows stale regardless of the teardown filter;
    // the filter only scopes which live processes are signaled.
    let reconciled = reconcile_registry(registry)?;
    let mut stale = reconciled
        .processes
        .into_iter()
        .filter(|process| process.reconciled_status == ProcessStatus::Stale.as_str())
        .map(|process| process.process_key)
        .collect::<Vec<_>>();
    let rows = process_rows(registry)?;
    let mut stopped = Vec::new();
    for row in rows.into_iter().filter(|row| {
        (is_active_status(&row.status) || row.unresolved_escape) && filter.matches(row)
    }) {
        if let Some(service_instance_id) = row.service_instance_id.as_deref() {
            let borrower_count = active_borrower_count(registry, service_instance_id, &row.run_id)?;
            if borrower_count > 0 && !row.unresolved_escape {
                return Err(RuntimeError::new(
                    ErrorCode::LeaseConflict,
                    format!(
                        "service instance {service_instance_id} has {borrower_count} active borrower lease(s)"
                    ),
                ));
            }
        }
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
        signal_process_group(row.pgid, libc::SIGTERM)?;
        if row.wait_until_process_group_empty(timeout_ms)? {
            settle_down_process(registry, &row)?;
            stopped.push(row.process_key);
            continue;
        }
        signal_process_group(row.pgid, libc::SIGKILL)?;
        if !row.wait_until_process_group_empty(1000)? {
            return Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to stop owned process group {}", row.pgid),
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
    state_root: &Path,
    identity: &StateIdentity,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    reconcile_registry(registry)?;
    clean_marked_state(state_base, state_root, identity, registry, mode)
}

#[derive(Debug)]
struct ProcessRow {
    process_key: String,
    pid: u32,
    pgid: i32,
    start_identity: StoredProcessIdentity,
    command_json: String,
    run_id: String,
    service_instance_id: Option<String>,
    status: String,
    computed_model_hash: String,
    service_status: Option<String>,
    service_lifetime: Option<String>,
    unresolved_escape: bool,
}

#[derive(Debug)]
struct RunLeaseRow {
    run_id: String,
    service_instance_id: String,
    owner_token: String,
    heartbeat_at: String,
    expires_at: String,
    status: String,
    computed_model_hash: String,
}

#[derive(Debug)]
struct PortRow {
    endpoint_key: String,
    service_instance_id: String,
    address: String,
    port: u16,
    status: String,
    owner_process_key: Option<String>,
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

    fn wait_until_process_group_empty(&self, timeout_ms: u64) -> RuntimeResult<bool> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if !process_group_has_live_member(self.pgid)? {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn process_rows(registry: &Registry) -> RuntimeResult<Vec<ProcessRow>> {
    let mut statement = registry
        .connection()
        .prepare(&format!(
            "
            SELECT
              p.process_key, p.pid, p.pgid, p.start_identity, p.command_json,
              p.run_id, p.service_instance_id, p.status, r.computed_model_hash,
              s.status, s.service_lifetime,
              CASE WHEN p.status = '{escaped_process}'
                         AND s.status = '{escaped_service}'
                         AND EXISTS (
                           SELECT 1 FROM ports ep
                           WHERE ep.service_instance_id = p.service_instance_id
                             AND ep.owner_process_key = p.process_key
                             AND ep.status IN ({open_ports})
                         )
                   THEN 1 ELSE 0 END
            FROM processes p
            JOIN runs r ON r.run_id = p.run_id
            LEFT JOIN services s ON s.service_instance_id = p.service_instance_id
            ORDER BY p.process_key
            ",
            escaped_process = ProcessStatus::Escaped.as_str(),
            escaped_service = ServiceStatus::Escaped.as_str(),
            open_ports = status::sql_in_list(status::PORT_OPEN),
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
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, i64>(11)? != 0,
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
                computed_model_hash,
                service_status,
                service_lifetime,
                unresolved_escape,
            )| {
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
                    process_key,
                    pid,
                    pgid,
                    start_identity,
                    command_json,
                    run_id,
                    service_instance_id,
                    status,
                    computed_model_hash,
                    service_status,
                    service_lifetime,
                    unresolved_escape,
                })
            },
        )
        .collect()
}

fn mark_process_stale(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![row.process_key, ProcessStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    if let Some(service_instance_id) = row.service_instance_id.as_deref() {
        transaction
            .execute(
                "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
                params![service_instance_id, ServiceStatus::Stale.as_str()],
            )
            .map_err(sql_error)?;
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
        "previousStatus": row.status,
        "command": row.command_json,
    })
    .to_string();
    insert_event(
        &transaction,
        &identity,
        ControlEvent {
            event_type: "process.stale",
            run_id: &row.run_id,
            service_instance_id: row.service_instance_id.as_deref(),
            process_key: Some(&row.process_key),
            computed_model_hash: Some(&row.computed_model_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn reconcile_expired_run_leases(registry: &mut Registry) -> RuntimeResult<()> {
    for lease in expired_run_leases(registry)? {
        mark_expired_lease_stale(registry, &lease)?;
    }
    Ok(())
}

fn reconcile_stale_port_reservations(registry: &mut Registry) -> RuntimeResult<()> {
    let ports = active_port_rows(registry)?;
    let processes = process_rows(registry)?;
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

fn expired_run_leases(registry: &Registry) -> RuntimeResult<Vec<RunLeaseRow>> {
    let mut statement = registry
        .connection()
        .prepare(&format!(
            "
            SELECT l.run_id, l.service_instance_id, l.owner_token, l.heartbeat_at,
                   l.expires_at, l.status, r.computed_model_hash
            FROM run_leases l
            JOIN runs r ON r.run_id = l.run_id
            WHERE l.status IN ({})
              AND l.expires_at <= strftime('%Y-%m-%dT%H:%M:%fZ','now')
            ORDER BY l.run_id
            ",
            status::sql_in_list(status::LEASE_OPEN)
        ))
        .map_err(sql_error)?;
    statement
        .query_map([], |row| {
            Ok(RunLeaseRow {
                run_id: row.get(0)?,
                service_instance_id: row.get(1)?,
                owner_token: row.get(2)?,
                heartbeat_at: row.get(3)?,
                expires_at: row.get(4)?,
                status: row.get(5)?,
                computed_model_hash: row.get(6)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)
}

fn active_port_rows(registry: &Registry) -> RuntimeResult<Vec<PortRow>> {
    let mut statement = registry
        .connection()
        .prepare(&format!(
            "
            SELECT endpoint_key, service_instance_id, address, port, status, owner_process_key
            FROM ports
            WHERE status IN ({})
            ORDER BY endpoint_key
            ",
            status::sql_in_list(status::PORT_OPEN)
        ))
        .map_err(sql_error)?;
    statement
        .query_map([], |row| {
            Ok(PortRow {
                endpoint_key: row.get(0)?,
                service_instance_id: row.get(1)?,
                address: row.get(2)?,
                port: row.get::<_, u16>(3)?,
                status: row.get(4)?,
                owner_process_key: row.get(5)?,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)
}

fn reconcile_service_lifetime_statuses(registry: &mut Registry) -> RuntimeResult<()> {
    let rows = process_rows(registry)?;
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    for row in rows
        .iter()
        .filter(|row| is_active_status(&row.status))
        .filter_map(|row| {
            Some((
                row.service_instance_id.as_deref()?,
                row.run_id.as_str(),
                row.service_status.as_deref()?,
                row.service_lifetime.as_deref()?,
            ))
        })
    {
        let (service_instance_id, owner_run_id, raw_service_status, service_lifetime) = row;
        let service_status = ServiceStatus::parse_db(raw_service_status)?;
        if !matches!(
            service_status,
            ServiceStatus::ProbeReady | ServiceStatus::Standing | ServiceStatus::Borrowed
        ) {
            continue;
        }
        let borrower_count =
            active_borrower_count_conn(&transaction, service_instance_id, owner_run_id)?;
        let next_status = if borrower_count > 0 {
            ServiceStatus::Borrowed
        } else if service_lifetime == service_lifetime_as_str(ServiceLifetime::RunScoped) {
            ServiceStatus::ProbeReady
        } else {
            ServiceStatus::Standing
        };
        transaction
            .execute(
                "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
                params![service_instance_id, next_status.as_str()],
            )
            .map_err(sql_error)?;
    }
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn reconcile_until_idle_services(registry: &mut Registry) -> RuntimeResult<()> {
    let rows = process_rows(registry)?;
    for row in rows
        .into_iter()
        .filter(|row| is_active_status(&row.status))
        .filter(|row| {
            row.service_lifetime.as_deref()
                == Some(service_lifetime_as_str(ServiceLifetime::UntilIdle))
        })
    {
        let Some(service_instance_id) = row.service_instance_id.as_deref() else {
            continue;
        };
        if run_lease_is_open(registry, service_instance_id, &row.run_id)? {
            continue;
        }
        if active_borrower_count(registry, service_instance_id, &row.run_id)? > 0 {
            continue;
        }
        if !row.is_live()? {
            mark_process_stale(registry, &row)?;
            continue;
        }
        signal_process_group(row.pgid, libc::SIGTERM)?;
        if !row.wait_until_process_group_empty(1000)? {
            signal_process_group(row.pgid, libc::SIGKILL)?;
            if !row.wait_until_process_group_empty(1000)? {
                return Err(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!("failed to stop idle until-idle process group {}", row.pgid),
                ));
            }
        }
        mark_stopped(registry, &row)?;
    }
    Ok(())
}

fn active_borrower_count(
    registry: &Registry,
    service_instance_id: &str,
    owner_run_id: &str,
) -> RuntimeResult<i64> {
    active_borrower_count_conn(registry.connection(), service_instance_id, owner_run_id)
}

fn run_lease_is_open(
    registry: &Registry,
    service_instance_id: &str,
    run_id: &str,
) -> RuntimeResult<bool> {
    let count: i64 = registry
        .connection()
        .query_row(
            &format!(
                "
            SELECT count(*)
            FROM run_leases
            WHERE service_instance_id = ?1
              AND run_id = ?2
              AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![service_instance_id, run_id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    Ok(count > 0)
}

fn active_borrower_count_conn(
    conn: &rusqlite::Connection,
    service_instance_id: &str,
    owner_run_id: &str,
) -> RuntimeResult<i64> {
    conn.query_row(
        &format!(
            "
        SELECT count(*)
        FROM run_leases
        WHERE service_instance_id = ?1
          AND run_id != ?2
          AND status IN ({})
        ",
            status::sql_in_list(status::LEASE_OPEN)
        ),
        params![service_instance_id, owner_run_id],
        |row| row.get(0),
    )
    .map_err(sql_error)
}

impl PortRow {
    fn is_owned_by_process(&self, process: &ProcessRow) -> bool {
        if let Some(owner_process_key) = self.owner_process_key.as_deref() {
            return owner_process_key == process.process_key;
        }
        process.service_instance_id.as_deref() == Some(self.service_instance_id.as_str())
    }
}

fn mark_expired_lease_stale(registry: &mut Registry, lease: &RunLeaseRow) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let changed = transaction
        .execute(
            &format!(
                "
                UPDATE run_leases
                SET status = ?3
                WHERE run_id = ?1 AND service_instance_id = ?2
                  AND status IN ({open_leases})
                  AND expires_at <= strftime('%Y-%m-%dT%H:%M:%fZ','now')
                  AND NOT EXISTS (
                    SELECT 1
                    FROM processes p
                    WHERE p.run_id = ?1
                      AND p.service_instance_id = ?2
                      AND (
                        p.status IN ({active_processes})
                        OR (
                          p.status = '{escaped}'
                          AND EXISTS (
                            SELECT 1 FROM ports ep
                            WHERE ep.service_instance_id = p.service_instance_id
                              AND ep.owner_process_key = p.process_key
                              AND ep.status IN ({open_ports})
                          )
                        )
                      )
                  )
                ",
                open_leases = status::sql_in_list(status::LEASE_OPEN),
                active_processes = status::sql_in_list(status::PROCESS_ACTIVE),
                escaped = ProcessStatus::Escaped.as_str(),
                open_ports = status::sql_in_list(status::PORT_OPEN),
            ),
            params![
                lease.run_id.as_str(),
                lease.service_instance_id.as_str(),
                RunLeaseStatus::Stale.as_str(),
            ],
        )
        .map_err(sql_error)?;
    if changed == 0 {
        transaction.commit().map_err(sql_error)?;
        return Ok(());
    }
    transaction
        .execute(
            &format!(
                "
                UPDATE runs
                SET status = ?2
                WHERE run_id = ?1
                  AND status NOT IN ({terminal_runs})
                  AND NOT EXISTS (
                    SELECT 1 FROM run_leases
                    WHERE run_id = ?1 AND status IN ({open_leases})
                  )
                ",
                terminal_runs = status::sql_in_list(status::RUN_TERMINAL),
                open_leases = status::sql_in_list(status::LEASE_OPEN),
            ),
            params![lease.run_id.as_str(), RunStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    // Release only owner-less ports reserved before a process row existed (a
    // crash between `reserve_service_start` and `record_service_start`). A
    // borrower lease names the owner's service too, but can never release the
    // owner's process-bound port evidence.
    transaction
        .execute(
            &format!(
                "
            UPDATE ports
            SET status = ?2
            WHERE service_instance_id = ?1
              AND owner_process_key IS NULL
              AND status IN ({})
            ",
                status::sql_in_list(status::PORT_OPEN)
            ),
            params![
                lease.service_instance_id.as_str(),
                PortStatus::Stale.as_str()
            ],
        )
        .map_err(sql_error)?;
    let payload_json = serde_json::json!({
        "serviceInstanceId": lease.service_instance_id.as_str(),
        "ownerToken": lease.owner_token.as_str(),
        "heartbeatAt": lease.heartbeat_at.as_str(),
        "expiresAt": lease.expires_at.as_str(),
        "previousStatus": lease.status.as_str(),
    })
    .to_string();
    insert_event(
        &transaction,
        &identity,
        ControlEvent {
            event_type: "run.lease-stale",
            run_id: &lease.run_id,
            service_instance_id: Some(&lease.service_instance_id),
            process_key: None,
            computed_model_hash: Some(&lease.computed_model_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn mark_port_stale(
    registry: &mut Registry,
    port: &PortRow,
    process: &ProcessRow,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
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
        "ownerProcessKey": port.owner_process_key.as_deref(),
    })
    .to_string();
    insert_event(
        &transaction,
        &identity,
        ControlEvent {
            event_type: "port.stale",
            run_id: &process.run_id,
            service_instance_id: Some(&port.service_instance_id),
            process_key: Some(&process.process_key),
            computed_model_hash: Some(&process.computed_model_hash),
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
            &row.computed_model_hash,
        );
    }
    let payload_json = serde_json::json!({
        "pid": row.pid,
        "pgid": row.pgid,
        "reason": "down",
        "command": row.command_json,
    })
    .to_string();
    mark_task_finished(
        registry,
        &row.run_id,
        &row.process_key,
        &row.computed_model_hash,
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
        &row.computed_model_hash,
    )
}

fn insert_event(
    transaction: &rusqlite::Transaction<'_>,
    identity: &RegistryIdentity,
    event: ControlEvent<'_>,
) -> RuntimeResult<()> {
    transaction
        .execute(
            "
            INSERT INTO events (
              at, environment, slot, event_type, run_id, service_instance_id,
              process_key, computed_model_hash, payload_json
            ) VALUES (
              strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
            )
            ",
            params![
                identity.environment.as_str(),
                identity.slot,
                event.event_type,
                event.run_id,
                event.service_instance_id,
                event.process_key,
                event.computed_model_hash,
                event.payload_json,
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

struct ControlEvent<'a> {
    event_type: &'a str,
    run_id: &'a str,
    service_instance_id: Option<&'a str>,
    process_key: Option<&'a str>,
    computed_model_hash: Option<&'a str>,
    payload_json: &'a str,
}

fn is_active_status(status: &str) -> bool {
    ProcessStatus::from_db(status).is_some_and(|status| status::PROCESS_ACTIVE.contains(&status))
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("control registry operation failed: {error}"),
    )
}
