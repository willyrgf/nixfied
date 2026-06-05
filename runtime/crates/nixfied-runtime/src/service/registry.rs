use std::path::Path;

use nixfied_model::{Model, ServiceSpec};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::leases::lease_ttl_modifier;
use crate::registry::{Registry, RegistryIdentity};
use crate::state::HostPlacement;

pub struct RunRecord<'a> {
    pub run_id: &'a str,
    pub owner_token: &'a str,
    pub model: &'a Model,
    pub admission: &'a Admission,
    pub placement: &'a HostPlacement,
}

pub struct ServiceRecord<'a> {
    pub service_instance_id: &'a str,
    pub service_name: &'a str,
    pub service_address_hash: &'a str,
    pub service: &'a ServiceSpec,
    pub endpoint_json: &'a str,
    pub state_root: &'a Path,
    pub endpoint_key: &'a str,
    pub endpoint_address: &'a str,
    pub endpoint_port: u16,
}

pub struct ProcessRecord<'a> {
    pub process_key: &'a str,
    pub pid: u32,
    pub pgid: i32,
    pub start_identity: &'a str,
    pub command_json: &'a str,
    pub run_id: &'a str,
    pub service_instance_id: &'a str,
}

pub struct TaskProcessRecord<'a> {
    pub run_id: &'a str,
    pub process_key: &'a str,
    pub pid: u32,
    pub pgid: i32,
    pub start_identity: &'a str,
    pub command_json: &'a str,
    pub computed_model_hash: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTerminalStatus {
    Succeeded,
    Failed,
    Canceled,
}

pub fn ensure_service_start_allowed(
    registry: &Registry,
    run_id: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    ensure_no_existing_run_conn(registry.connection(), run_id)?;
    ensure_no_active_lease_conn(registry.connection(), service_instance_id)?;
    ensure_no_active_service_conn(registry.connection(), service_instance_id)
}

pub fn record_service_start(
    registry: &mut Registry,
    run: &RunRecord<'_>,
    service: &ServiceRecord<'_>,
    process: &ProcessRecord<'_>,
) -> RuntimeResult<()> {
    let generator_json = serde_json::to_string(&run.model.generator).map_err(json_error)?;
    let target_json = serde_json::to_string(&run.model.target).map_err(json_error)?;
    let source_json = serde_json::to_string(&run.admission.source).map_err(json_error)?;
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    ensure_no_existing_run_transaction(&transaction, run.run_id)?;
    ensure_no_active_lease_transaction(&transaction, service.service_instance_id)?;
    ensure_no_active_service_transaction(&transaction, service.service_instance_id)?;
    ensure_no_active_port_transaction(
        &transaction,
        service.endpoint_address,
        service.endpoint_port,
    )?;
    transaction
        .execute(
            "
            INSERT INTO runs (
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (?1, ?2, ?3, 'service-starting', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ",
            params![
                run.run_id,
                identity.environment.as_str(),
                identity.slot,
                run.admission.model_path.display().to_string(),
                run.admission.computed_model_hash.as_str(),
                run.admission.runtime_abi.as_str(),
                run.admission.toolchain_id.as_str(),
                generator_json,
                target_json,
                source_json,
                run.placement.summary_path.display().to_string(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO run_leases (
              run_id, environment, slot, service_instance_id, owner_token,
              heartbeat_at, expires_at, status
            ) VALUES (
              ?1, ?2, ?3, ?4, ?5,
              strftime('%Y-%m-%dT%H:%M:%fZ','now'),
              strftime('%Y-%m-%dT%H:%M:%fZ','now', ?6),
              'active'
            )
            ",
            params![
                run.run_id,
                identity.environment.as_str(),
                identity.slot,
                service.service_instance_id,
                run.owner_token,
                lease_ttl_modifier(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR REPLACE INTO services (
              service_instance_id, environment, slot, service_name,
              service_address_hash, endpoint_identity_hash, state_identity_hash,
              runtime_compatibility_hash, target_identity_hash, status,
              endpoint_json, state_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'starting', ?10, ?11)
            ",
            params![
                service.service_instance_id,
                identity.environment.as_str(),
                identity.slot,
                service.service_name,
                service.service_address_hash,
                service.service.identity.endpoint_identity_hash.as_str(),
                service.service.identity.state_identity_hash.as_str(),
                service.service.identity.runtime_compatibility_hash.as_str(),
                service.service.identity.target_identity_hash.as_str(),
                service.endpoint_json,
                service.state_root.display().to_string(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR REPLACE INTO processes (
              process_key, environment, slot, pid, pgid, start_identity,
              command_json, run_id, service_instance_id, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'running')
            ",
            params![
                process.process_key,
                identity.environment.as_str(),
                identity.slot,
                process.pid,
                process.pgid,
                process.start_identity,
                process.command_json,
                process.run_id,
                process.service_instance_id,
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR REPLACE INTO ports (
              endpoint_key, environment, slot, service_instance_id, address, port,
              status, owner_process_key
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'reserved', NULL)
            ",
            params![
                service.endpoint_key,
                identity.environment.as_str(),
                identity.slot,
                service.service_instance_id,
                service.endpoint_address,
                service.endpoint_port,
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "run.admitted",
            run_id: Some(run.run_id),
            service_instance_id: None,
            process_key: None,
            computed_model_hash: Some(&run.admission.computed_model_hash),
            payload_json: "{}",
        },
    )?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.starting",
            run_id: Some(run.run_id),
            service_instance_id: Some(service.service_instance_id),
            process_key: Some(process.process_key),
            computed_model_hash: Some(&run.admission.computed_model_hash),
            payload_json: process.command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_endpoint_owner_verified(
    registry: &mut Registry,
    endpoint_key: &str,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE ports
            SET status = 'active', owner_process_key = ?2
            WHERE endpoint_key = ?1
            ",
            params![endpoint_key, process_key],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "port.owner-verified",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_service_probe_ready(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = 'probe-ready' WHERE service_instance_id = ?1",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.probe-ready",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_service_stopped(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    let run_was_canceling = matches!(
        transaction
            .query_row(
                "SELECT status FROM runs WHERE run_id = ?1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .as_deref(),
        Some("canceling" | "canceled")
    );
    let (terminal_status, lease_status, event_type) = if run_was_canceling {
        ("canceled", "canceled", "service.canceled")
    } else {
        ("stopped", "completed", "service.stopped")
    };
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, terminal_status],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
            params![service_instance_id, terminal_status],
        )
        .map_err(sql_error)?;
    if run_was_canceling {
        transaction
            .execute(
                "UPDATE runs SET status = 'canceled' WHERE run_id = ?1",
                params![run_id],
            )
            .map_err(sql_error)?;
    }
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ('active', 'canceling')
            ",
            params![run_id, service_instance_id, lease_status],
        )
        .map_err(sql_error)?;
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type,
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn record_service_canceling(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = 'canceling' WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = 'canceling'
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status = 'active'
            ",
            params![run_id, service_instance_id],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.canceling",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_service_canceled(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = 'canceled' WHERE process_key = ?1",
            params![process_key],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = 'canceled' WHERE service_instance_id = ?1",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = 'canceled' WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = 'canceled'
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ('active', 'canceling')
            ",
            params![run_id, service_instance_id],
        )
        .map_err(sql_error)?;
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.canceled",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn record_task_canceling(
    registry: &mut Registry,
    run_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = 'canceling' WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = 'canceling'
            WHERE run_id = ?1 AND status = 'active'
            ",
            params![run_id],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "task.canceling",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_service_failed(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = 'failed' WHERE process_key = ?1",
            params![process_key],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = 'failed' WHERE service_instance_id = ?1",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = 'service-failed' WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = 'failed'
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ('active', 'canceling')
            ",
            params![run_id, service_instance_id],
        )
        .map_err(sql_error)?;
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.failed",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_process_escape(
    registry: &mut Registry,
    process_key: &str,
    run_id: &str,
    service_instance_id: &str,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = 'escaped' WHERE process_key = ?1",
            params![process_key],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = 'escaped' WHERE service_instance_id = ?1",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = 'proc-escaped' WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = 'failed'
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ('active', 'canceling')
            ",
            params![run_id, service_instance_id],
        )
        .map_err(sql_error)?;
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.proc-escape",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn ensure_service_instance_probe_ready(
    registry: &Registry,
    service_name: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let status = registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            params![service_instance_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    match status.as_deref() {
        Some("probe-ready") => Ok(()),
        Some(status) => Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("service {service_name} is {status}, not probe-ready"),
        )),
        None => Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("service {service_name} has not been started"),
        )),
    }
}

pub fn record_task_started(
    registry: &mut Registry,
    process: &TaskProcessRecord<'_>,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity,
              command_json, run_id, service_instance_id, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, 'running')
            ",
            params![
                process.process_key,
                identity.environment.as_str(),
                identity.slot,
                process.pid,
                process.pgid,
                process.start_identity,
                process.command_json,
                process.run_id
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "task.running",
            run_id: Some(process.run_id),
            service_instance_id: None,
            process_key: Some(process.process_key),
            computed_model_hash: Some(process.computed_model_hash),
            payload_json: process.command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_task_finished(
    registry: &mut Registry,
    run_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    terminal_status: TaskTerminalStatus,
    payload_json: &str,
) -> RuntimeResult<()> {
    let (status, event_type, run_status, lease_status) = match terminal_status {
        TaskTerminalStatus::Succeeded => ("succeeded", "task.succeeded", "task-succeeded", None),
        TaskTerminalStatus::Failed => ("failed", "task.failed", "task-failed", Some("failed")),
        TaskTerminalStatus::Canceled => ("canceled", "task.canceled", "canceled", Some("canceled")),
    };
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, status],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, run_status],
        )
        .map_err(sql_error)?;
    if let Some(lease_status) = lease_status {
        transaction
            .execute(
                "
                UPDATE run_leases
                SET status = ?2
                WHERE run_id = ?1 AND status IN ('active', 'canceling')
                ",
                params![run_id, lease_status],
            )
            .map_err(sql_error)?;
    }
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type,
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn insert_event(
    transaction: &rusqlite::Transaction<'_>,
    identity: &RegistryIdentity,
    event: EventRecord<'_>,
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

struct EventRecord<'a> {
    event_type: &'a str,
    run_id: Option<&'a str>,
    service_instance_id: Option<&'a str>,
    process_key: Option<&'a str>,
    computed_model_hash: Option<&'a str>,
    payload_json: &'a str,
}

fn ensure_no_existing_run_conn(conn: &Connection, run_id: &str) -> RuntimeResult<()> {
    let existing = conn
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            params![run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_existing_run(run_id, existing)
}

fn ensure_no_existing_run_transaction(
    transaction: &Transaction<'_>,
    run_id: &str,
) -> RuntimeResult<()> {
    let existing = transaction
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            params![run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_existing_run(run_id, existing)
}

fn refuse_existing_run(run_id: &str, existing: Option<String>) -> RuntimeResult<()> {
    if let Some(status) = existing {
        Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("run {run_id} already exists with status {status}"),
        ))
    } else {
        Ok(())
    }
}

fn ensure_no_active_service_conn(
    conn: &Connection,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let existing = conn
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            params![service_instance_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_active_service(service_instance_id, existing)
}

fn ensure_no_active_service_transaction(
    transaction: &Transaction<'_>,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let existing = transaction
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            params![service_instance_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_active_service(service_instance_id, existing)
}

fn refuse_active_service(service_instance_id: &str, existing: Option<String>) -> RuntimeResult<()> {
    if let Some(status) = existing {
        if matches!(
            status.as_str(),
            "stopped" | "escaped" | "failed" | "stale" | "canceled"
        ) {
            return Ok(());
        }
        Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!(
                "M0 service reuse is unsupported; service instance {service_instance_id} is {status}"
            ),
        ))
    } else {
        Ok(())
    }
}

fn ensure_no_active_lease_conn(conn: &Connection, service_instance_id: &str) -> RuntimeResult<()> {
    let existing = conn
        .query_row(
            "
            SELECT run_id, status FROM run_leases
            WHERE service_instance_id = ?1 AND status IN ('active', 'canceling')
            ORDER BY run_id
            LIMIT 1
            ",
            params![service_instance_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_active_lease(service_instance_id, existing)
}

fn ensure_no_active_lease_transaction(
    transaction: &Transaction<'_>,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let existing = transaction
        .query_row(
            "
            SELECT run_id, status FROM run_leases
            WHERE service_instance_id = ?1 AND status IN ('active', 'canceling')
            ORDER BY run_id
            LIMIT 1
            ",
            params![service_instance_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_active_lease(service_instance_id, existing)
}

fn refuse_active_lease(
    service_instance_id: &str,
    existing: Option<(String, String)>,
) -> RuntimeResult<()> {
    if let Some((run_id, status)) = existing {
        return Err(RuntimeError::new(
            ErrorCode::LeaseConflict,
            format!(
                "service instance {service_instance_id} has active run lease {run_id} with status {status}"
            ),
        ));
    }
    Ok(())
}

fn ensure_no_active_port_transaction(
    transaction: &Transaction<'_>,
    address: &str,
    port: u16,
) -> RuntimeResult<()> {
    let existing = transaction
        .query_row(
            "
            SELECT endpoint_key, status FROM ports
            WHERE address = ?1 AND port = ?2
            ORDER BY endpoint_key
            LIMIT 1
            ",
            params![address, port],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    if let Some((endpoint_key, status)) = existing
        && matches!(status.as_str(), "reserved" | "binding" | "bound" | "active")
    {
        return Err(RuntimeError::new(
            ErrorCode::PortConflict,
            format!("endpoint {endpoint_key} already has active port {address}:{port}"),
        ));
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
            SET status = 'released'
            WHERE service_instance_id = ?1
            ",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string())
}

fn json_error(error: serde_json::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::ModelAdmission, error.to_string())
}
