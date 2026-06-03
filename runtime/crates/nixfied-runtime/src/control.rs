use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::Registry;
use crate::service::process::{
    process_group_has_live_member, process_is_live_with_identity, signal_process_group,
};
use crate::service::registry::mark_service_stopped;
use crate::state::{CleanupOutcome, StateIdentity, clean_marked_state};

const ACTIVE_PROCESS_STATUSES: &[&str] = &["starting", "running", "ready"];

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
    let mut observations = Vec::with_capacity(rows.len());
    for row in rows {
        let active = is_active_status(&row.status);
        let live = if active { row.is_live()? } else { false };
        let reconciled_status = if active && !live {
            mark_process_stale(registry, &row)?;
            "stale".to_string()
        } else if live {
            "running".to_string()
        } else {
            row.status.clone()
        };
        observations.push(ProcessObservation {
            process_key: row.process_key,
            run_id: row.run_id,
            service_instance_id: row.service_instance_id,
            pid: row.pid,
            pgid: row.pgid,
            registry_status: row.status,
            reconciled_status,
            live,
        });
    }
    Ok(PsReport {
        processes: observations,
    })
}

pub fn down_owned_process_groups(
    registry: &mut Registry,
    timeout_ms: u64,
) -> RuntimeResult<DownReport> {
    let reconciled = reconcile_registry(registry)?;
    let mut stale = reconciled
        .processes
        .into_iter()
        .filter(|process| process.reconciled_status == "stale")
        .map(|process| process.process_key)
        .collect::<Vec<_>>();
    let rows = process_rows(registry)?;
    let mut stopped = Vec::new();
    for row in rows
        .into_iter()
        .filter(|row| row.service_instance_id.is_some() && is_active_status(&row.status))
    {
        if !row.is_live()? {
            mark_process_stale(registry, &row)?;
            stale.push(row.process_key);
            continue;
        }
        signal_process_group(row.pgid, libc::SIGTERM)?;
        if row.wait_until_process_group_empty(timeout_ms)? {
            mark_stopped(registry, &row)?;
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
        mark_stopped(registry, &row)?;
        stopped.push(row.process_key);
    }
    Ok(DownReport { stopped, stale })
}

pub fn clean_reconciled_state(
    registry: &mut Registry,
    state_base: &Path,
    state_root: &Path,
    identity: &StateIdentity,
) -> RuntimeResult<CleanupOutcome> {
    reconcile_registry(registry)?;
    clean_marked_state(state_base, state_root, identity, registry)
}

#[derive(Debug)]
struct ProcessRow {
    process_key: String,
    pid: u32,
    pgid: i32,
    start_identity: StoredStartIdentity,
    command_json: String,
    run_id: String,
    service_instance_id: Option<String>,
    status: String,
    computed_model_hash: String,
}

impl ProcessRow {
    fn is_live(&self) -> RuntimeResult<bool> {
        process_is_live_with_identity(
            self.pid,
            self.pgid,
            self.start_identity.platform_start.as_deref(),
        )
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredStartIdentity {
    platform_start: Option<String>,
}

fn process_rows(registry: &Registry) -> RuntimeResult<Vec<ProcessRow>> {
    let mut statement = registry
        .connection()
        .prepare(
            "
            SELECT
              p.process_key, p.pid, p.pgid, p.start_identity, p.command_json,
              p.run_id, p.service_instance_id, p.status, r.computed_model_hash
            FROM processes p
            JOIN runs r ON r.run_id = p.run_id
            ORDER BY p.process_key
            ",
        )
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
            )| {
                let start_identity = serde_json::from_str::<StoredStartIdentity>(
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
                })
            },
        )
        .collect()
}

fn mark_process_stale(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = 'stale' WHERE process_key = ?1",
            params![row.process_key],
        )
        .map_err(sql_error)?;
    if let Some(service_instance_id) = row.service_instance_id.as_deref() {
        transaction
            .execute(
                "UPDATE services SET status = 'stale' WHERE service_instance_id = ?1",
                params![service_instance_id],
            )
            .map_err(sql_error)?;
        transaction
            .execute(
                "
                UPDATE ports
                SET status = 'released'
                WHERE service_instance_id = ?1
                ",
                params![service_instance_id],
            )
            .map_err(sql_error)?;
    }
    insert_event(
        &transaction,
        "process.stale",
        &row.run_id,
        row.service_instance_id.as_deref(),
        &row.process_key,
        &row.computed_model_hash,
        &serde_json::json!({
            "pid": row.pid,
            "pgid": row.pgid,
            "previousStatus": row.status,
            "command": row.command_json,
        })
        .to_string(),
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn mark_stopped(registry: &mut Registry, row: &ProcessRow) -> RuntimeResult<()> {
    let Some(service_instance_id) = row.service_instance_id.as_deref() else {
        return Ok(());
    };
    mark_service_stopped(
        registry,
        &row.run_id,
        service_instance_id,
        &row.process_key,
        &row.computed_model_hash,
    )
}

fn insert_event(
    transaction: &rusqlite::Transaction<'_>,
    event_type: &str,
    run_id: &str,
    service_instance_id: Option<&str>,
    process_key: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    transaction
        .execute(
            "
            INSERT INTO events (
              at, event_type, run_id, service_instance_id, process_key,
              computed_model_hash, payload_json
            ) VALUES (
              strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?1, ?2, ?3, ?4, ?5, ?6
            )
            ",
            params![
                event_type,
                run_id,
                service_instance_id,
                process_key,
                computed_model_hash,
                payload_json,
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn is_active_status(status: &str) -> bool {
    ACTIVE_PROCESS_STATUSES.contains(&status)
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string())
}
