use std::collections::BTreeMap;
use std::path::Path;

use crate::execution::ServiceIdentity;
use nixfied_model::ServiceLifetime;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::redaction::Redactor;
use crate::registry::leases::lease_ttl_modifier;
use crate::registry::status::{
    self, DbStatus, PortStatus, ProcessStatus, RunLeaseStatus, RunStatus,
};
use crate::registry::{Registry, RegistryIdentity};
use crate::state::HostPlacement;

use super::{StoredProcessIdentity, TrackedProcessIdentity};

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
    pub service_lifetime: ServiceLifetime,
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

pub struct VerifiedEndpointActivation<'a> {
    pub endpoint_key: &'a str,
    pub address: &'a str,
    pub port: u16,
    pub ownership_json: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredServiceState {
    pub(crate) service_name: String,
    pub(crate) service_address_hash: String,
    pub(crate) endpoint_identity_hash: String,
    pub(crate) state_identity_hash: String,
    pub(crate) runtime_compatibility_hash: String,
    pub(crate) target_identity_hash: String,
    pub(crate) service_lifetime: ServiceLifetime,
    pub(crate) state_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredServiceProcess {
    pub(crate) process_key: String,
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) platform_start: Option<String>,
    pub(crate) tracked_processes: Vec<TrackedProcessIdentity>,
    pub(crate) start_identity_json: String,
    pub(crate) run_id: String,
    pub(crate) status: ProcessStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredServiceEndpoint {
    pub(crate) endpoint_key: String,
    pub(crate) address: String,
    pub(crate) port: u16,
    pub(crate) status: PortStatus,
    pub(crate) owner_process_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredServiceLease {
    pub(crate) run_id: String,
    pub(crate) status: RunLeaseStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceSnapshot {
    pub(crate) service: Option<StoredServiceState>,
    pub(crate) process: Option<StoredServiceProcess>,
    pub(crate) endpoints: Vec<StoredServiceEndpoint>,
    pub(crate) leases: Vec<StoredServiceLease>,
}

pub(crate) struct ServiceReuseGuard<'a> {
    pub(crate) service: &'a ServiceRecord<'a>,
    pub(crate) process: &'a StoredServiceProcess,
    pub(crate) endpoints: &'a [PortReservation<'a>],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationOutcome {
    Canceled,
    Failed,
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
/// is outcome-settled if the start later fails before `record_service_start`
/// takes ownership.
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
    let redactor = registry.redactor().clone();
    // The run heartbeat writes through a second connection once the first
    // service is ready. Acquire the writer slot before the conflict reads: a
    // deferred read transaction cannot safely upgrade after another writer has
    // changed the WAL snapshot, and SQLite may return BUSY without invoking the
    // configured busy handler.
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
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
    // PORT_OPEN); outcome settlement releases it on a failed start, while
    // reconciliation stales it with the lease after a pre-process crash.
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
        &redactor,
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
/// leaves durable evidence — including a service-less selection (a task or
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
    let redactor = registry.redactor().clone();
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
        &redactor,
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

/// Settle a reservation when no child exists, or only after a spawned child is
/// proven gone. Port release and the run/lease outcome commit together.
pub(crate) fn settle_service_reservation(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    outcome: ReservationOutcome,
) -> RuntimeResult<()> {
    let (lease_status, run_status, event_type) = match outcome {
        ReservationOutcome::Canceled => (
            RunLeaseStatus::Canceled,
            RunStatus::Canceled,
            "service.reservation-canceled",
        ),
        ReservationOutcome::Failed => (
            RunLeaseStatus::Failed,
            RunStatus::ServiceFailed,
            "service.reservation-failed",
        ),
    };
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
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
            params![run_id, service_instance_id, lease_status.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE runs SET status = ?2 WHERE run_id = ?1",
            params![run_id, run_status.as_str()],
        )
        .map_err(sql_error)?;
    // Release the endpoint ports reserved in `reserve_service_start` so a start
    // that fails before the process is recorded does not leak the port.
    release_service_ports(&transaction, service_instance_id)?;
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type,
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
    let redactor = registry.redactor().clone();
    let process_command_json = redactor.redact_json_str(process.command_json)?;
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
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
              runtime_compatibility_hash, target_identity_hash, service_lifetime,
              state_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                service_lifetime_as_str(service.service_lifetime),
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
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ",
            params![
                process.process_key,
                identity.environment.as_str(),
                identity.slot,
                process.pid,
                process.pgid,
                process.start_identity,
                process_command_json,
                process.run_id,
                process.service_instance_id,
                ProcessStatus::Running.as_str(),
            ],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            "
            UPDATE ports
            SET owner_process_key = ?2
            WHERE service_instance_id = ?1 AND status = ?3
            ",
            params![
                service.service_instance_id,
                process.process_key,
                PortStatus::Reserved.as_str(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        &redactor,
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
        &redactor,
        EventRecord {
            event_type: "service.starting",
            run_id: Some(run.run_id),
            service_instance_id: Some(service.service_instance_id),
            process_key: Some(process.process_key),
            computed_model_hash: Some(&run.admission.computed_model_hash),
            payload_json: &process_command_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub(crate) fn record_service_borrow(
    registry: &mut Registry,
    run: &RunRecord<'_>,
    guard: &ServiceReuseGuard<'_>,
) -> RuntimeResult<bool> {
    let generator_json = run.admission.generator_json.as_str();
    let target_json = run.admission.target_json.as_str();
    let source_json = serde_json::to_string(&run.admission.source).map_err(json_error)?;
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let snapshot = read_service_snapshot_conn(&transaction, guard.service.service_instance_id)?;
    if !reuse_snapshot_matches(&snapshot, guard) {
        return Ok(false);
    }
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
                run.run_id,
                identity.environment.as_str(),
                identity.slot,
                RunStatus::ServiceStarting.as_str(),
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
              ?7
            )
            ",
            params![
                run.run_id,
                identity.environment.as_str(),
                identity.slot,
                guard.service.service_instance_id,
                run.owner_token,
                lease_ttl_modifier(),
                RunLeaseStatus::Active.as_str(),
            ],
        )
        .map_err(sql_error)?;
    let payload_json = serde_json::json!({
        "borrowedProcessKey": guard.process.process_key,
    })
    .to_string();
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type: "service.borrowed",
            run_id: Some(run.run_id),
            service_instance_id: Some(guard.service.service_instance_id),
            process_key: Some(&guard.process.process_key),
            computed_model_hash: Some(&run.admission.computed_model_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(true)
}

pub fn release_service_borrow(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    borrowed_process_key: &str,
    computed_model_hash: &str,
    canceled: bool,
) -> RuntimeResult<()> {
    let lease_status = if canceled {
        RunLeaseStatus::Canceled
    } else {
        RunLeaseStatus::Completed
    };
    let event_type = if canceled {
        "service.borrow-canceled"
    } else {
        "service.borrow-released"
    };
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
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
            params![run_id, service_instance_id, lease_status.as_str()],
        )
        .map_err(sql_error)?;
    if canceled {
        transaction
            .execute(
                "UPDATE runs SET status = ?2 WHERE run_id = ?1",
                params![run_id, RunStatus::Canceled.as_str()],
            )
            .map_err(sql_error)?;
    } else {
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
    let payload_json = serde_json::json!({
        "borrowedProcessKey": borrowed_process_key,
    })
    .to_string();
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type,
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(borrowed_process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn mark_service_standing(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    service_lifetime: ServiceLifetime,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    let lease_status = match service_lifetime {
        ServiceLifetime::RunScoped => RunLeaseStatus::Completed,
        ServiceLifetime::UntilIdle => RunLeaseStatus::Completed,
        ServiceLifetime::PersistentUntilDown => RunLeaseStatus::Active,
    };
    if matches!(service_lifetime, ServiceLifetime::PersistentUntilDown) {
        transaction
            .execute(
                &format!(
                    "
                UPDATE run_leases
                SET status = ?3,
                    expires_at = '9999-12-31T23:59:59.999Z'
                WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
                ",
                    status::sql_in_list(status::LEASE_OPEN)
                ),
                params![run_id, service_instance_id, lease_status.as_str()],
            )
            .map_err(sql_error)?;
    } else {
        transaction
            .execute(
                &format!(
                    "
                UPDATE run_leases
                SET status = ?3,
                    expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                WHERE run_id = ?1 AND service_instance_id = ?2 AND status IN ({})
                ",
                    status::sql_in_list(status::LEASE_OPEN)
                ),
                params![run_id, service_instance_id, lease_status.as_str()],
            )
            .map_err(sql_error)?;
    }
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
    let payload_json = serde_json::json!({
        "serviceLifetime": service_lifetime_as_str(service_lifetime),
    })
    .to_string();
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type: "service.standing",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json: &payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

pub fn activate_service_ready(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    process_key: &str,
    computed_model_hash: &str,
    endpoints: &[VerifiedEndpointActivation<'_>],
    lifecycle: (&str, &str, &str),
) -> RuntimeResult<()> {
    let (operation_id, operation_class, terminal_success) = lifecycle;
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let mut statement = transaction
        .prepare(&format!(
            "
            SELECT endpoint_key, address, port, status, owner_process_key
            FROM ports
            WHERE service_instance_id = ?1 AND status IN ({})
            ORDER BY endpoint_key
            ",
            status::sql_in_list(status::PORT_OPEN)
        ))
        .map_err(sql_error)?;
    let rows = statement
        .query_map(params![service_instance_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u16>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(sql_error)?;
    let mut stored = BTreeMap::new();
    for row in rows {
        let (endpoint_key, address, port, port_status, owner_process_key) =
            row.map_err(sql_error)?;
        stored.insert(
            endpoint_key,
            (address, port, port_status, owner_process_key),
        );
    }
    drop(statement);
    let expected = endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.endpoint_key.to_string(),
                (
                    endpoint.address.to_string(),
                    endpoint.port,
                    PortStatus::Reserved.as_str().to_string(),
                    Some(process_key.to_string()),
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
            &identity,
            &redactor,
            EventRecord {
                event_type: "port.owner-verified",
                run_id: Some(run_id),
                service_instance_id: Some(service_instance_id),
                process_key: Some(process_key),
                computed_model_hash: Some(computed_model_hash),
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
        &identity,
        &redactor,
        EventRecord {
            event_type: "service.probe-ready",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
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
        &identity,
        &redactor,
        EventRecord {
            event_type: "service.lifecycle.terminal",
            run_id: Some(run_id),
            service_instance_id: Some(service_instance_id),
            process_key: Some(process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json: &lifecycle_payload,
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
    let redactor = registry.redactor().clone();
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
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
    let (process_status, lease_status, event_type) = if run_was_canceling {
        (
            ProcessStatus::Canceled,
            RunLeaseStatus::Canceled,
            "service.canceled",
        )
    } else {
        (
            ProcessStatus::Stopped,
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
        &redactor,
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
    let redactor = registry.redactor().clone();
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
        &redactor,
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
    let redactor = registry.redactor().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, ProcessStatus::Canceled.as_str()],
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
        &redactor,
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
    let redactor = registry.redactor().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        &redactor,
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
    let redactor = registry.redactor().clone();
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
        &redactor,
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
    let redactor = registry.redactor().clone();
    let transaction = registry.connection_mut().transaction().map_err(sql_error)?;
    transaction
        .execute(
            "UPDATE processes SET status = ?2 WHERE process_key = ?1",
            params![process_key, ProcessStatus::Failed.as_str()],
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
        &redactor,
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
    process: &ProcessRecord<'_>,
    computed_model_hash: &str,
    platform_start: Option<&str>,
    payload_json: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let service_rows: i64 = transaction
        .query_row(
            "SELECT count(*) FROM services WHERE service_instance_id = ?1",
            params![process.service_instance_id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if service_rows != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "escape settlement found {service_rows} service rows for {}, expected 1",
                process.service_instance_id
            ),
        ));
    }
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
                process.run_id,
                process.service_instance_id,
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
    let run_pre_states = [
        RunStatus::ServiceStarting,
        RunStatus::Canceling,
        RunStatus::Canceled,
        RunStatus::Completed,
        RunStatus::TaskSucceeded,
        RunStatus::TaskFailed,
        RunStatus::ServiceFailed,
        RunStatus::Stale,
    ];
    let changed_run = transaction
        .execute(
            &format!(
                "
            UPDATE runs
            SET status = ?3
            WHERE run_id = ?1
              AND computed_model_hash = ?2
              AND status IN ({})
            ",
                status::sql_in_list(&run_pre_states)
            ),
            params![
                process.run_id,
                computed_model_hash,
                RunStatus::ProcEscaped.as_str()
            ],
        )
        .map_err(sql_error)?;
    if changed_run != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!(
                "escape settlement changed {changed_run} run rows for {}, expected 1",
                process.run_id
            ),
        ));
    }
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type: "service.proc-escape",
            run_id: Some(process.run_id),
            service_instance_id: Some(process.service_instance_id),
            process_key: Some(process.process_key),
            computed_model_hash: Some(computed_model_hash),
            payload_json,
        },
    )?;
    transaction.commit().map_err(sql_error)?;
    Ok(())
}

/// Release the durable ownership evidence of an unresolved escape after OS
/// liveness proves the recorded process containment is gone. The terminal
/// escaped process evidence is intentionally retained.
pub(crate) fn release_unresolved_escape_ports(
    registry: &mut Registry,
    process_key: &str,
    run_id: &str,
    service_instance_id: &str,
    computed_model_hash: &str,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let transaction = registry
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let unresolved: i64 = transaction
        .query_row(
            &format!(
                "
                SELECT count(*)
                FROM processes p
                JOIN services s ON s.service_instance_id = p.service_instance_id
                JOIN runs r ON r.run_id = p.run_id
                WHERE p.process_key = ?1
                  AND p.run_id = ?2
                  AND p.service_instance_id = ?3
                  AND p.status = ?4
                  AND r.computed_model_hash = ?5
                  AND EXISTS (
                    SELECT 1 FROM ports ep
                    WHERE ep.service_instance_id = p.service_instance_id
                      AND ep.owner_process_key = p.process_key
                      AND ep.status IN ({})
                  )
                ",
                status::sql_in_list(status::PORT_OPEN)
            ),
            params![
                process_key,
                run_id,
                service_instance_id,
                ProcessStatus::Escaped.as_str(),
                computed_model_hash,
            ],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if unresolved != 1 {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("unresolved escape {process_key} no longer has exact open-port evidence"),
        ));
    }
    transaction
        .execute(
            &format!(
                "
                UPDATE run_leases
                SET status = ?3
                WHERE run_id = ?1 AND service_instance_id = ?2
                  AND status IN ({})
                ",
                status::sql_in_list(status::LEASE_OPEN)
            ),
            params![run_id, service_instance_id, RunLeaseStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    transaction
        .execute(
            &format!(
                "
                UPDATE ports
                SET status = ?3
                WHERE service_instance_id = ?1
                  AND owner_process_key = ?2
                  AND status IN ({})
                ",
                status::sql_in_list(status::PORT_OPEN)
            ),
            params![service_instance_id, process_key, PortStatus::Stale.as_str()],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type: "service.escape-reconciled",
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

pub fn ensure_service_instance_probe_ready(
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

pub fn record_task_started(
    registry: &mut Registry,
    process: &TaskProcessRecord<'_>,
) -> RuntimeResult<()> {
    let identity = registry.identity().clone();
    let redactor = registry.redactor().clone();
    let command_json = redactor.redact_json_str(process.command_json)?;
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
                command_json,
                process.run_id,
                ProcessStatus::Running.as_str(),
            ],
        )
        .map_err(sql_error)?;
    insert_event(
        &transaction,
        &identity,
        &redactor,
        EventRecord {
            event_type: "task.running",
            run_id: Some(process.run_id),
            service_instance_id: None,
            process_key: Some(process.process_key),
            computed_model_hash: Some(process.computed_model_hash),
            payload_json: &command_json,
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
    let redactor = registry.redactor().clone();
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
        &redactor,
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
    redactor: &Redactor,
    event: EventRecord<'_>,
) -> RuntimeResult<()> {
    let payload_json = redactor.redact_json_str(event.payload_json)?;
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
                payload_json,
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
    let service_raw = connection
        .query_row(
            "
            SELECT service_name, service_address_hash, endpoint_identity_hash,
                   state_identity_hash, runtime_compatibility_hash,
                   target_identity_hash, service_lifetime, state_root
            FROM services
            WHERE service_instance_id = ?1
            ",
            params![service_instance_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;
    let service = service_raw
        .map(
            |(
                service_name,
                service_address_hash,
                endpoint_identity_hash,
                state_identity_hash,
                runtime_compatibility_hash,
                target_identity_hash,
                lifetime,
                state_root,
            )|
             -> RuntimeResult<StoredServiceState> {
                Ok(StoredServiceState {
                    service_name,
                    service_address_hash,
                    endpoint_identity_hash,
                    state_identity_hash,
                    runtime_compatibility_hash,
                    target_identity_hash,
                    service_lifetime: parse_service_lifetime(&lifetime)?,
                    state_root,
                })
            },
        )
        .transpose()?;

    let process_rows = {
        let mut statement = connection
            .prepare(&format!(
                "
                SELECT p.process_key, p.pid, p.pgid, p.start_identity, p.run_id, p.status
                FROM processes p
                WHERE p.service_instance_id = ?1
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
                ORDER BY p.process_key
                ",
                active_processes = status::sql_in_list(status::PROCESS_ACTIVE),
                escaped = ProcessStatus::Escaped.as_str(),
                open_ports = status::sql_in_list(status::PORT_OPEN),
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
            |(process_key, pid, pgid, start_identity_json, run_id, process_status)| -> RuntimeResult<StoredServiceProcess> {
                let start_identity = serde_json::from_str::<StoredProcessIdentity>(
                    &start_identity_json,
                )
                .map_err(|error| {
                    RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        format!("invalid start identity for process {process_key}: {error}"),
                    )
                })?;
                Ok(StoredServiceProcess {
                    process_key,
                    pid,
                    pgid,
                    platform_start: start_identity.platform_start,
                    tracked_processes: start_identity.tracked_processes,
                    start_identity_json,
                    run_id,
                    status: ProcessStatus::parse_db(&process_status)?,
                })
            },
        )
        .transpose()?;

    let endpoints = {
        let mut statement = connection
            .prepare(&format!(
                "
                SELECT endpoint_key, address, port, status, owner_process_key
                FROM ports
                WHERE service_instance_id = ?1 AND status IN ({})
                ORDER BY endpoint_key
                ",
                status::sql_in_list(status::PORT_OPEN),
            ))
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![service_instance_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u16>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        rows.into_iter()
            .map(
                |(endpoint_key, address, port, endpoint_status, owner_process_key)| {
                    Ok(StoredServiceEndpoint {
                        endpoint_key,
                        address,
                        port,
                        status: PortStatus::parse_db(&endpoint_status)?,
                        owner_process_key,
                    })
                },
            )
            .collect::<RuntimeResult<Vec<_>>>()?
    };

    let leases = {
        let mut statement = connection
            .prepare(
                "
                SELECT l.run_id, l.status
                FROM run_leases l
                WHERE l.service_instance_id = ?1
                ORDER BY l.run_id
                ",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![service_instance_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        rows.into_iter()
            .map(|(run_id, lease_status)| {
                Ok(StoredServiceLease {
                    run_id,
                    status: RunLeaseStatus::parse_db(&lease_status)?,
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?
    };
    Ok(ServiceSnapshot {
        service,
        process,
        endpoints,
        leases,
    })
}

fn reuse_snapshot_matches(snapshot: &ServiceSnapshot, guard: &ServiceReuseGuard<'_>) -> bool {
    let Some(service) = &snapshot.service else {
        return false;
    };
    if service.service_name != guard.service.service_name
        || service.service_address_hash != guard.service.service_address_hash
        || service.endpoint_identity_hash != guard.service.identity.endpoint_identity_hash
        || service.state_identity_hash != guard.service.identity.state_identity_hash
        || service.runtime_compatibility_hash != guard.service.identity.runtime_compatibility_hash
        || service.target_identity_hash != guard.service.identity.target_identity_hash
        || service.state_root != guard.service.state_root.display().to_string()
    {
        return false;
    }
    let Some(process) = &snapshot.process else {
        return false;
    };
    if process.process_key != guard.process.process_key
        || process.pid != guard.process.pid
        || process.pgid != guard.process.pgid
        || process.start_identity_json != guard.process.start_identity_json
        || process.status != ProcessStatus::Ready
    {
        return false;
    }
    expected_endpoint_map(guard.endpoints, &guard.process.process_key)
        == stored_endpoint_map(&snapshot.endpoints)
}

fn expected_endpoint_map(
    endpoints: &[PortReservation<'_>],
    process_key: &str,
) -> BTreeMap<String, (String, u16, PortStatus, Option<String>)> {
    endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.endpoint_key.to_string(),
                (
                    endpoint.address.to_string(),
                    endpoint.port,
                    PortStatus::Active,
                    Some(process_key.to_string()),
                ),
            )
        })
        .collect()
}

fn stored_endpoint_map(
    endpoints: &[StoredServiceEndpoint],
) -> BTreeMap<String, (String, u16, PortStatus, Option<String>)> {
    endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint.endpoint_key.clone(),
                (
                    endpoint.address.clone(),
                    endpoint.port,
                    endpoint.status,
                    endpoint.owner_process_key.clone(),
                ),
            )
        })
        .collect()
}

fn parse_service_lifetime(value: &str) -> RuntimeResult<ServiceLifetime> {
    match value {
        "run-scoped" => Ok(ServiceLifetime::RunScoped),
        "until-idle" => Ok(ServiceLifetime::UntilIdle),
        "persistent-until-down" => Ok(ServiceLifetime::PersistentUntilDown),
        _ => Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("unknown service lifetime {value}"),
        )),
    }
}

fn ensure_no_active_service_conn(
    conn: &Connection,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let existing = actionable_process_status(conn, service_instance_id)?;
    refuse_active_service(service_instance_id, existing)
}

fn ensure_no_active_service_transaction(
    transaction: &Transaction<'_>,
    service_instance_id: &str,
) -> RuntimeResult<()> {
    let existing = actionable_process_status(transaction, service_instance_id)?;
    refuse_active_service(service_instance_id, existing)
}

fn actionable_process_status(
    connection: &Connection,
    service_instance_id: &str,
) -> RuntimeResult<Option<String>> {
    connection
        .query_row(
            &format!(
                "
                SELECT p.status
                FROM processes p
                WHERE p.service_instance_id = ?1
                  AND (
                    p.status IN ({active})
                    OR (
                      p.status = '{escaped}'
                      AND EXISTS (
                        SELECT 1 FROM ports ep
                        WHERE ep.service_instance_id = p.service_instance_id
                          AND ep.owner_process_key = p.process_key
                          AND ep.status IN ({ports})
                      )
                    )
                  )
                ORDER BY p.process_key
                LIMIT 1
                ",
                active = status::sql_in_list(status::PROCESS_ACTIVE),
                escaped = ProcessStatus::Escaped.as_str(),
                ports = status::sql_in_list(status::PORT_OPEN),
            ),
            params![service_instance_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)
}

fn refuse_active_service(service_instance_id: &str, existing: Option<String>) -> RuntimeResult<()> {
    if let Some(status) = existing {
        Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("service instance {service_instance_id} has actionable process {status}"),
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
            ErrorCode::LeaseConflict,
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

pub fn service_lifetime_as_str(lifetime: ServiceLifetime) -> &'static str {
    match lifetime {
        ServiceLifetime::RunScoped => "run-scoped",
        ServiceLifetime::UntilIdle => "until-idle",
        ServiceLifetime::PersistentUntilDown => "persistent-until-down",
    }
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("service registry operation failed: {error}"),
    )
}

fn json_error(error: serde_json::Error) -> RuntimeError {
    RuntimeError::new(ErrorCode::ModelAdmission, error.to_string())
}
