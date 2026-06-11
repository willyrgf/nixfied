use std::path::Path;

use nixfied_model::ServiceIdentity;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::leases::lease_ttl_modifier;
use crate::registry::status::{
    self, DbStatus, PortStatus, ProcessStatus, RunLeaseStatus, RunStatus, ServiceStatus,
};
use crate::registry::{Registry, RegistryIdentity};
use crate::state::HostPlacement;

pub struct RunRecord<'a> {
    pub run_id: &'a str,
    pub owner_token: &'a str,
    pub admission: &'a Admission,
    pub placement: &'a HostPlacement,
}

pub struct ServiceRecord<'a> {
    pub service_instance_id: &'a str,
    pub service_name: &'a str,
    pub service_address_hash: &'a str,
    pub identity: &'a ServiceIdentity,
    pub endpoint_json: &'a str,
    pub state_root: &'a Path,
}

/// One endpoint's reservation: the registry key, bind address, and port a service
/// will own. Reserved alongside the run lease before any process starts, so a port
/// conflict is refused before prepare and spawn rather than discovered afterward.
pub struct PortReservation<'a> {
    pub endpoint_key: &'a str,
    pub address: &'a str,
    pub port: u16,
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
    /// The task exceeded its own timeout budget: an execution failure with its
    /// own event, never conflated with an operator cancellation.
    TimedOut,
    Canceled,
}

pub fn ensure_service_start_allowed(
    registry: &Registry,
    run_id: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    // The run row is created idempotently (a run may own several services), so the
    // safety gates are per service instance: no active lease and no live service.
    // The run's own reservation lease is excluded so this stays a valid recheck
    // after the slot is reserved ahead of prepare.
    ensure_no_active_lease_conn(registry.connection(), service_instance_id, run_id)?;
    ensure_no_active_service_conn(registry.connection(), service_instance_id)
}

/// Reserve a service instance for a run before any state-mutating lifecycle work
/// (e.g. prepare/initdb) runs: insert the run row and an active lease under the
/// same conflict gates as `record_service_start`, so a second runtime racing the
/// same slot is refused instead of running prepare concurrently. The reservation
/// must be released (see `release_service_reservation`) if the start later fails
/// before `record_service_start` takes ownership.
pub fn reserve_service_start(
    registry: &mut Registry,
    run: &RunRecord<'_>,
    service_instance_id: &str,
    endpoints: &[PortReservation<'_>],
) -> RuntimeResult<()> {
    let generator_json = run.admission.generator_json.as_str();
    let target_json = run.admission.target_json.as_str();
    let source_json = serde_json::to_string(&run.admission.source).map_err(json_error)?;
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    ensure_no_active_lease_transaction(&transaction, service_instance_id, run.run_id)?;
    ensure_no_active_service_transaction(&transaction, service_instance_id)?;
    transaction
        .execute(
            "
            INSERT OR IGNORE INTO runs (
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (?1, ?2, ?3, ?12, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                RunStatus::ServiceStarting.as_str(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR IGNORE INTO run_leases (
              run_id, environment, slot, service_instance_id, owner_token,
              heartbeat_at, expires_at, status
            ) VALUES (
              ?1, ?2, ?3, ?4, ?5,
              strftime('%Y-%m-%dT%H:%M:%fZ','now'),
              strftime('%Y-%m-%dT%H:%M:%fZ','now', ?6),
              ?7
            )
            ",
            params![
                run.run_id,
                identity.environment.as_str(),
                identity.slot,
                service_instance_id,
                run.owner_token,
                lease_ttl_modifier(),
                RunLeaseStatus::Active.as_str(),
            ],
        )
        .map_err(sql_error)?;
    // Reserve every endpoint port in the same transaction as the lease: a port
    // already held by another active service refuses the start here, before any
    // prepare or spawn runs. A Reserved row blocks competing reservations (it is in
    // PORT_OPEN); it is released by `release_service_reservation` on a failed start
    // and staled with the lease on a crash before the process is recorded.
    for endpoint in endpoints {
        ensure_no_active_port_transaction(&transaction, endpoint.address, endpoint.port)?;
        transaction
            .execute(
                "
                INSERT OR REPLACE INTO ports (
                  endpoint_key, environment, slot, service_instance_id, address, port,
                  status, owner_process_key
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
                ",
                params![
                    endpoint.endpoint_key,
                    identity.environment.as_str(),
                    identity.slot,
                    service_instance_id,
                    endpoint.address,
                    endpoint.port,
                    PortStatus::Reserved.as_str(),
                ],
            )
            .map_err(sql_error)?;
    }
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.reserved",
            run_id: Some(run.run_id),
            service_instance_id: Some(service_instance_id),
            process_key: None,
            computed_model_hash: Some(&run.admission.computed_model_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Record the run row up front, before any service starts, so every admitted run
/// leaves durable evidence — including a service-less selection (a workflow or
/// environment of only service-less tasks) whose service loop never runs and so
/// never reaches `reserve_service_start`. `INSERT OR IGNORE` keeps the later
/// service-path inserts idempotent no-ops, preserving their semantics exactly.
///
/// No run lease is created here: a lease keys on a `service_instance_id`
/// (`run_leases PRIMARY KEY (run_id, service_instance_id)`) and guards cross-run
/// service/port/state contention, which a service-less task does not create. The
/// per-service lease is still taken inside the service loop. This asymmetry is
/// deliberate, not an oversight.
pub fn record_run_created(
    registry: &mut Registry,
    run_id: &str,
    admission: &Admission,
    placement: &HostPlacement,
) -> RuntimeResult<()> {
    let source_json = serde_json::to_string(&admission.source).map_err(json_error)?;
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR IGNORE INTO runs (
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ",
            params![
                run_id,
                identity.environment.as_str(),
                identity.slot,
                RunStatus::ServiceStarting.as_str(),
                admission.model_path.display().to_string(),
                admission.computed_model_hash.as_str(),
                admission.runtime_abi.as_str(),
                admission.toolchain_id.as_str(),
                admission.generator_json.as_str(),
                admission.target_json.as_str(),
                source_json,
                placement.summary_path.display().to_string(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "run.created",
            run_id: Some(run_id),
            service_instance_id: None,
            process_key: None,
            computed_model_hash: Some(&admission.computed_model_hash),
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Settle a run that reached the end without a service- or task-driven terminal
/// status (e.g. a degenerate selection with no services and no tasks) to
/// `completed`. Guarded on `service-starting` so a task- or cancellation-derived
/// terminal status is never clobbered, mirroring the guard in
/// `mark_service_stopped`.
pub fn mark_run_completed(registry: &mut Registry, run_id: &str) -> RuntimeResult<()> {
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1 AND status = ?3",
            params![
                run_id,
                RunStatus::Completed.as_str(),
                RunStatus::ServiceStarting.as_str()
            ],
        )
        .map_err(sql_error)?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Release a reservation taken by `reserve_service_start` when the start fails
/// before `record_service_start` takes ownership, so the lease does not leak and
/// block later runs.
pub fn release_service_reservation(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            &format!(
                "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![run_id, service_instance_id, RunLeaseStatus::Failed.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1 AND status = ?3",
            params![
                run_id,
                RunStatus::ServiceFailed.as_str(),
                RunStatus::ServiceStarting.as_str()
            ],
        )
        .map_err(sql_error)?;
    // Release the endpoint ports reserved in `reserve_service_start` so a start
    // that fails before the process is recorded does not leak the port.
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type: "service.reservation-released",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: None,
            computed_model_hash: None,
            payload_json: "{}",
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn record_service_start(
    registry: &mut Registry,
    run: &RunRecord<'_>,
    service: &ServiceRecord<'_>,
    process: &ProcessRecord<'_>,
) -> RuntimeResult<()> {
    let generator_json = run.admission.generator_json.as_str();
    let target_json = run.admission.target_json.as_str();
    let source_json = serde_json::to_string(&run.admission.source).map_err(json_error)?;
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    ensure_no_active_lease_transaction(&transaction, service.service_instance_id, run.run_id)?;
    ensure_no_active_service_transaction(&transaction, service.service_instance_id)?;
    transaction
        .execute(
            "
            INSERT OR IGNORE INTO runs (
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (?1, ?2, ?3, ?12, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                RunStatus::ServiceStarting.as_str(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR IGNORE INTO run_leases (
              run_id, environment, slot, service_instance_id, owner_token,
              heartbeat_at, expires_at, status
            ) VALUES (
              ?1, ?2, ?3, ?4, ?5,
              strftime('%Y-%m-%dT%H:%M:%fZ','now'),
              strftime('%Y-%m-%dT%H:%M:%fZ','now', ?6),
              ?7
            )
            ",
            params![
                run.run_id,
                identity.environment.as_str(),
                identity.slot,
                service.service_instance_id,
                run.owner_token,
                lease_ttl_modifier(),
                RunLeaseStatus::Active.as_str(),
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
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?12, ?10, ?11)
            ",
            params![
                service.service_instance_id,
                identity.environment.as_str(),
                identity.slot,
                service.service_name,
                service.service_address_hash,
                service.identity.endpoint_identity_hash.as_str(),
                service.identity.state_identity_hash.as_str(),
                service.identity.runtime_compatibility_hash.as_str(),
                service.identity.target_identity_hash.as_str(),
                service.endpoint_json,
                service.state_root.display().to_string(),
                ServiceStatus::Starting.as_str(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            INSERT OR REPLACE INTO processes (
              process_key, environment, slot, pid, pgid, start_identity,
              command_json, run_id, service_instance_id, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
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
                ProcessStatus::Running.as_str(),
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
            SET status = ?3, owner_process_key = ?2
            WHERE endpoint_key = ?1
            ",
            params![endpoint_key, process_key, PortStatus::Active.as_str()],
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
            "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
            params![service_instance_id, ServiceStatus::ProbeReady.as_str()],
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
    let run_status = transaction
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            params![run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error)?;
    let run_was_canceling = run_status
        .as_deref()
        .and_then(RunStatus::from_db)
        .is_some_and(|status| matches!(status, RunStatus::Canceling | RunStatus::Canceled));
    let (process_status, service_status, lease_status, event_type) = if run_was_canceling {
        (
            ProcessStatus::Canceled,
            ServiceStatus::Canceled,
            RunLeaseStatus::Canceled,
            "service.canceled",
        )
    } else {
        (
            ProcessStatus::Stopped,
            ServiceStatus::Stopped,
            RunLeaseStatus::Completed,
            "service.stopped",
        )
    };
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, process_status.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
            params![service_instance_id, service_status.as_str()],
        )
        .map_err(sql_error)?;
    if run_was_canceling {
        transaction
            .execute(
                "UPDATE runs SET status = ?2 WHERE run_id = ?1",
                params![run_id, RunStatus::Canceled.as_str()],
            )
            .map_err(sql_error)?;
    } else {
        // A run whose environment declares services but no tasks finishes here:
        // move it out of 'service-starting' to a terminal status. Guarded on the
        // starting status so a task-derived terminal result is never clobbered.
        transaction
            .execute(
                "UPDATE runs SET status = ?2 WHERE run_id = ?1 AND status = ?3",
                params![
                    run_id,
                    RunStatus::Completed.as_str(),
                    RunStatus::ServiceStarting.as_str()
                ],
            )
            .map_err(sql_error)?;
    }
    transaction
        .execute(
            &format!(
                "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![run_id, service_instance_id, lease_status.as_str()],
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
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, RunStatus::Canceling.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status = ?4
            ",
            params![
                run_id,
                service_instance_id,
                RunLeaseStatus::Canceling.as_str(),
                RunLeaseStatus::Active.as_str()
            ],
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
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, ProcessStatus::Canceled.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
            params![service_instance_id, ServiceStatus::Canceled.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, RunStatus::Canceled.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            &format!(
                "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![
                run_id,
                service_instance_id,
                RunLeaseStatus::Canceled.as_str()
            ],
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

pub fn record_service_lifecycle_event(
    registry: &mut Registry,
    event_type: &str,
    run_id: Option<&str>,
    service_instance_id: &str,
    process_key: Option<&str>,
    computed_model_hash: &str,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        EventRecord {
            event_type,
            run_id,
            service_instance_id: Some(service_instance_id),
            process_key,
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
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, RunStatus::Canceling.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE run_leases
            SET status = ?2
            WHERE run_id = ?1 AND status = ?3
            ",
            params![
                run_id,
                RunLeaseStatus::Canceling.as_str(),
                RunLeaseStatus::Active.as_str()
            ],
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
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, ProcessStatus::Failed.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
            params![service_instance_id, ServiceStatus::Failed.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, RunStatus::ServiceFailed.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            &format!(
                "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![run_id, service_instance_id, RunLeaseStatus::Failed.as_str()],
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
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, ProcessStatus::Escaped.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE services SET status = ?2 WHERE service_instance_id = ?1",
            params![service_instance_id, ServiceStatus::Escaped.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, RunStatus::ProcEscaped.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            &format!(
                "
            UPDATE run_leases
            SET status = ?3
            WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![run_id, service_instance_id, RunLeaseStatus::Failed.as_str()],
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
        Some(raw) if raw == ServiceStatus::ProbeReady.as_str() => Ok(()),
        Some(status) => Err(RuntimeError::new(
            ErrorCode::DependencyUnavailable,
            format!("service {service_name} is {status}, not probe-ready"),
        )),
        None => Err(RuntimeError::new(
            ErrorCode::DependencyUnavailable,
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
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9)
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
                ProcessStatus::Running.as_str(),
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
    let (process_status, event_type, run_status, lease_status) = match terminal_status {
        TaskTerminalStatus::Succeeded => (
            ProcessStatus::Succeeded,
            "task.succeeded",
            RunStatus::TaskSucceeded,
            None,
        ),
        TaskTerminalStatus::Failed => (
            ProcessStatus::Failed,
            "task.failed",
            RunStatus::TaskFailed,
            Some(RunLeaseStatus::Failed),
        ),
        TaskTerminalStatus::TimedOut => (
            ProcessStatus::Failed,
            "task.timed-out",
            RunStatus::TaskFailed,
            Some(RunLeaseStatus::Failed),
        ),
        TaskTerminalStatus::Canceled => (
            ProcessStatus::Canceled,
            "task.canceled",
            RunStatus::Canceled,
            Some(RunLeaseStatus::Canceled),
        ),
    };
    let identity = registry.identity().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, process_status.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, run_status.as_str()],
        )
        .map_err(sql_error)?;
    if let Some(lease_status) = lease_status {
        transaction
            .execute(
                &format!(
                    "
                UPDATE run_leases
                SET status = ?2
                WHERE run_id = ?1 AND status IN ({})
                ",
                    status::sql_in_list(status::LEASE_OPEN)
                ),
                params![run_id, lease_status.as_str()],
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
        if ServiceStatus::from_db(&status).is_some_and(|status| !status.is_active()) {
            return Ok(());
        }
        Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!(
                "service reuse is unsupported; service instance {service_instance_id} is {status}"
            ),
        ))
    } else {
        Ok(())
    }
}

// The caller's own reservation lease is excluded via `own_run_id` so the start
// path can reserve the slot before prepare and still pass its own later checks.
// An empty `own_run_id` excludes nothing (run ids are non-empty).
fn ensure_no_active_lease_conn(
    conn: &Connection,
    service_instance_id: &str,
    own_run_id: &str,
) -> RuntimeResult<()> {
    let existing = conn
        .query_row(
            &format!(
                "
            SELECT run_id, status FROM run_leases
            WHERE service_instance_id = ?1 AND run_id != ?2 AND status IN ({})
            ORDER BY run_id
            LIMIT 1
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![service_instance_id, own_run_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    refuse_active_lease(service_instance_id, existing)
}

fn ensure_no_active_lease_transaction(
    transaction: &Transaction<'_>,
    service_instance_id: &str,
    own_run_id: &str,
) -> RuntimeResult<()> {
    let existing = transaction
        .query_row(
            &format!(
                "
            SELECT run_id, status FROM run_leases
            WHERE service_instance_id = ?1 AND run_id != ?2 AND status IN ({})
            ORDER BY run_id
            LIMIT 1
            ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![service_instance_id, own_run_id],
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
        && PortStatus::from_db(&status)
            .is_some_and(|port_status| status::PORT_OPEN.contains(&port_status))
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
            SET status = ?2
            WHERE service_instance_id = ?1
            ",
            params![service_instance_id, PortStatus::Released.as_str()],
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
