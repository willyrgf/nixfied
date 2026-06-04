use std::path::Path;

use nixfied_model::{Model, ServiceSpec};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::Registry;
use crate::state::HostPlacement;

pub struct RunRecord<'a> {
    pub run_id: &'a str,
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

pub fn ensure_service_start_allowed(
    registry: &Registry,
    run_id: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    ensure_no_existing_run_conn(registry.connection(), run_id)?;
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
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    ensure_no_existing_run_transaction(&transaction, run.run_id)?;
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
              run_id, status, model_path, computed_model_hash, runtime_abi,
              toolchain_id, generator_json, target_json, source_json, summary_path
            ) VALUES (?1, 'service-starting', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ",
            params![
                run.run_id,
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
            INSERT OR REPLACE INTO services (
              service_instance_id, service_name, service_address_hash,
              endpoint_identity_hash, state_identity_hash, runtime_compatibility_hash,
              target_identity_hash, status, endpoint_json, state_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'starting', ?8, ?9)
            ",
            params![
                service.service_instance_id,
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
              process_key, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running')
            ",
            params![
                process.process_key,
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
              endpoint_key, service_instance_id, address, port, status, owner_process_key
            ) VALUES (?1, ?2, ?3, ?4, 'reserved', NULL)
            ",
            params![
                service.endpoint_key,
                service.service_instance_id,
                service.endpoint_address,
                service.endpoint_port,
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        "run.admitted",
        Some(run.run_id),
        None,
        None,
        Some(&run.admission.computed_model_hash),
        "{}",
    )?;
    insert_event(
        &transaction,
        "service.starting",
        Some(run.run_id),
        Some(service.service_instance_id),
        Some(process.process_key),
        Some(&run.admission.computed_model_hash),
        process.command_json,
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
        "port.owner-verified",
        Some(run_id),
        Some(service_instance_id),
        Some(process_key),
        Some(computed_model_hash),
        payload_json,
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
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = 'probe-ready' WHERE service_instance_id = ?1",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        "service.probe-ready",
        Some(run_id),
        Some(service_instance_id),
        Some(process_key),
        Some(computed_model_hash),
        "{}",
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
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = 'stopped' WHERE process_key = ?1",
            params![process_key],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = 'stopped' WHERE service_instance_id = ?1",
            params![service_instance_id],
        )
        .map_err(sql_error)?;
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        "service.stopped",
        Some(run_id),
        Some(service_instance_id),
        Some(process_key),
        Some(computed_model_hash),
        "{}",
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
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        "service.failed",
        Some(run_id),
        Some(service_instance_id),
        Some(process_key),
        Some(computed_model_hash),
        payload_json,
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
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        "service.proc-escape",
        Some(run_id),
        Some(service_instance_id),
        Some(process_key),
        Some(computed_model_hash),
        payload_json,
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
    run_id: &str,
    process_key: &str,
    pid: u32,
    pgid: i32,
    start_identity: &str,
    command_json: &str,
    computed_model_hash: &str,
) -> RuntimeResult<()> {
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT INTO processes (
              process_key, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'running')
            ",
            params![process_key, pid, pgid, start_identity, command_json, run_id],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        "task.running",
        Some(run_id),
        None,
        Some(process_key),
        Some(computed_model_hash),
        command_json,
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_task_finished(
    registry: &mut Registry,
    run_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    success: bool,
    payload_json: &str,
) -> RuntimeResult<()> {
    let status = if success { "succeeded" } else { "failed" };
    let event_type = if success {
        "task.succeeded"
    } else {
        "task.failed"
    };
    let run_status = if success {
        "task-succeeded"
    } else {
        "task-failed"
    };
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
    insert_event(
        &transaction,
        event_type,
        Some(run_id),
        None,
        Some(process_key),
        Some(computed_model_hash),
        payload_json,
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

fn insert_event(
    transaction: &rusqlite::Transaction<'_>,
    event_type: &str,
    run_id: Option<&str>,
    service_instance_id: Option<&str>,
    process_key: Option<&str>,
    computed_model_hash: Option<&str>,
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
        if matches!(status.as_str(), "stopped" | "escaped" | "failed") {
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
    if let Some((endpoint_key, status)) = existing {
        if matches!(status.as_str(), "reserved" | "binding" | "bound" | "active") {
            return Err(RuntimeError::new(
                ErrorCode::PortConflict,
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
