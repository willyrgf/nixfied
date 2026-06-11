use std::collections::BTreeMap;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nixfied_model::{ContainmentRequirement, Model};
use serde::Serialize;

use crate::admission::Admission;
use crate::cancellation::{CancellationToken, canceled_error};
use crate::control::reconcile_registry;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecProbe, ExecService, OpMeta, Probe, ResolvedExec, StdinPolicy};
use crate::registry::{Registry, RunLeaseHeartbeat};
use crate::service::identity::{
    compute_service_identity, service_address_hash, service_instance_id,
};
use crate::service::ownership::{ExpectedEndpointOwner, verify_endpoint_ownership};
use crate::service::readiness::{wait_for_exec_probe, wait_for_tcp_probe};
use crate::service::registry::{
    PortReservation, ProcessRecord, RunRecord, ServiceRecord, ensure_service_start_allowed,
    mark_endpoint_owner_verified, mark_process_escape, mark_service_canceled, mark_service_failed,
    mark_service_probe_ready, mark_service_stopped, record_service_canceling,
    record_service_lifecycle_event, record_service_start, release_service_reservation,
    reserve_service_start,
};
use crate::slot::SelectedSlot;
use crate::state::{CleanupOutcome, HostPlacement, StateIdentity, clean_marked_state};

const FOREGROUND_GRACE: Duration = Duration::from_millis(100);
const MONITOR_INTERVAL: Duration = Duration::from_millis(1);

/// The child's stdin, per the exec's declared policy: a closed `/dev/null` or the
/// operator's inherited stdin.
pub(crate) fn stdin_for(policy: StdinPolicy) -> Stdio {
    match policy {
        StdinPolicy::Null => Stdio::null(),
        StdinPolicy::Inherit => Stdio::inherit(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedEndpoint {
    pub endpoint_id: String,
    pub host: String,
    pub port: u16,
}

pub struct StartedService {
    child: Child,
    monitor: ProcessMonitor,
    /// The resolved service: the executor reads lifecycle ops, the bound endpoint,
    /// and the stop signal from here, never from the raw `Model`.
    service: ExecService,
    /// The ready/health probes with exec args/env already substituted against
    /// the slot plan at start time.
    ready_probe: Probe,
    health_probe: Probe,
    /// Where probe attempt output is captured, alongside the service logs.
    logs_dir: PathBuf,
    pub run_id: String,
    pub service_instance_id: String,
    pub process_key: String,
    pub pid: u32,
    pub pgid: i32,
    pub platform_start_identity: Option<String>,
    /// The primary endpoint: the tcp probe target and the endpoint recorded as the
    /// service's address.
    pub selected_endpoint: SelectedEndpoint,
    /// Every endpoint the service binds (including the primary), each reserved and
    /// ownership-verified after readiness.
    pub selected_endpoints: Vec<SelectedEndpoint>,
    pub computed_model_hash: String,
    pub source_root: PathBuf,
    pub state_root: PathBuf,
    pub owner_token: String,
}

impl StartedService {
    pub fn wait_for_probe_ready(&mut self, registry: &mut Registry) -> RuntimeResult<()> {
        self.wait_for_probe_ready_cancellable(registry, &CancellationToken::new())
    }

    pub fn wait_for_probe_ready_cancellable(
        &mut self,
        registry: &mut Registry,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        let record = LifecycleRecord::from_meta(&self.service.ready.meta, "ready");
        let context = self.lifecycle_event_context();
        record_lifecycle_started(registry, &context, &record)?;
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        cancellation.check()?;
        if let Err(error) = self.ensure_alive_or_record_escape(registry) {
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        if let Err(error) = self.wait_ready_probe(cancellation) {
            if error.code == ErrorCode::Canceled {
                return Err(error);
            }
            if let Some(error) = self.escape_error(registry) {
                self.cleanup_after_escape();
                let _ = record_lifecycle_failure(registry, &context, &record, &error);
                return Err(error);
            }
            if let Err(error) = self.ensure_alive_or_record_escape(registry) {
                let _ = record_lifecycle_failure(registry, &context, &record, &error);
                return Err(error);
            }
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            self.cleanup_after_probe_failure(registry, &error);
            return Err(error);
        }
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        cancellation.check()?;
        if let Err(error) = self.ensure_alive_or_record_escape(registry) {
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        let ownership_json = match self.verify_endpoint_ownerships(registry) {
            Ok(payload) => payload,
            Err(error) => {
                if let Some(error) = self.escape_error(registry) {
                    self.cleanup_after_escape();
                    let _ = record_lifecycle_failure(registry, &context, &record, &error);
                    return Err(error);
                }
                if let Err(error) = self.ensure_alive_or_record_escape(registry) {
                    let _ = record_lifecycle_failure(registry, &context, &record, &error);
                    return Err(error);
                }
                let _ = record_lifecycle_failure(registry, &context, &record, &error);
                self.cleanup_after_probe_failure(registry, &error);
                return Err(error);
            }
        };
        for (key, ownership_json) in &ownership_json {
            mark_endpoint_owner_verified(
                registry,
                key,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                ownership_json,
            )?;
        }
        mark_service_probe_ready(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
        )?;
        record_lifecycle_success(registry, &context, &record)
    }

    pub fn check_health(&mut self, registry: &mut Registry) -> RuntimeResult<()> {
        self.check_health_cancellable(registry, &CancellationToken::new())
    }

    pub fn check_health_cancellable(
        &mut self,
        registry: &mut Registry,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        let record = LifecycleRecord::from_meta(&self.service.health.meta, "health");
        let context = self.lifecycle_event_context();
        record_lifecycle_started(registry, &context, &record)?;
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        cancellation.check()?;
        if let Err(error) = self.ensure_alive_or_record_escape(registry) {
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            return Err(error);
        }
        if let Err(error) = self.wait_health_probe(cancellation) {
            if error.code == ErrorCode::Canceled {
                return Err(error);
            }
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            self.cleanup_after_probe_failure(registry, &error);
            return Err(error);
        }
        if let Err(error) = self.verify_endpoint_ownerships(registry) {
            let _ = record_lifecycle_failure(registry, &context, &record, &error);
            self.cleanup_after_probe_failure(registry, &error);
            return Err(error);
        }
        record_lifecycle_success(registry, &context, &record)
    }

    fn wait_ready_probe(&self, cancellation: &CancellationToken) -> RuntimeResult<()> {
        self.wait_probe(&self.ready_probe, cancellation)
    }

    fn wait_health_probe(&self, cancellation: &CancellationToken) -> RuntimeResult<()> {
        self.wait_probe(&self.health_probe, cancellation)
    }

    fn wait_probe(&self, probe: &Probe, cancellation: &CancellationToken) -> RuntimeResult<()> {
        match probe {
            Probe::Tcp(probe) => wait_for_tcp_probe(
                probe,
                &self.selected_endpoint.host,
                self.selected_endpoint.port,
                cancellation,
            ),
            Probe::Exec(probe) => {
                wait_for_exec_probe(probe, &self.source_root, &self.logs_dir, cancellation)
            }
        }
    }

    pub fn cancel(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        reason: &str,
    ) -> RuntimeResult<()> {
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "reason": reason,
        })
        .to_string();
        record_service_canceling(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            &payload,
        )?;
        self.terminate_owned(timeout_ms)?;
        let _ = wait_for_child_exit(&mut self.child, 1000)?;
        self.monitor.stop();
        mark_service_canceled(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            &payload,
        )
    }

    pub fn stop(mut self, registry: &mut Registry, timeout_ms: u64) -> RuntimeResult<()> {
        self.stop_with_cancellation(registry, timeout_ms, None)
    }

    pub fn stop_cancellable(
        mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        self.stop_with_cancellation(registry, timeout_ms, Some(cancellation))
    }

    fn stop_with_cancellation(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        cancellation: Option<&CancellationToken>,
    ) -> RuntimeResult<()> {
        let context = self.lifecycle_event_context();
        let stop_record = LifecycleRecord::from_meta(&self.service.stop.meta, "stop");
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(error);
        }
        if let Some(cancellation) = cancellation
            && cancellation.is_canceled()
        {
            self.cancel(registry, timeout_ms, "run canceled during shutdown")?;
            return Err(canceled_error());
        }
        record_lifecycle_started(registry, &context, &stop_record)?;
        if let Some(status) = self.child.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect foreground service child: {error}"),
            )
        })? {
            let message = format!("foreground service exited before stop: {status}");
            let error = self.record_escape(registry, message, Vec::new());
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(error);
        }
        // Graceful shutdown is the model's declared stop signal escalated to
        // SIGKILL. The graceful budget is the model's stopPolicy.timeoutMs, capped
        // by the CLI timeout as an upper bound.
        let stop_timeout = (self.service.stop.timeout.as_millis() as u64).min(timeout_ms);
        let escalated = match self.stop_owned(self.service.stop.signal.libc(), stop_timeout) {
            Ok(escalated) => escalated,
            Err(error) => {
                let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
                return Err(error);
            }
        };
        self.record_stop_signaled(registry, escalated, stop_timeout);
        let _ = wait_for_child_exit(&mut self.child, 1000)?;
        if let Some(cancellation) = cancellation
            && cancellation.is_canceled()
        {
            let payload = serde_json::json!({
                "pid": self.pid,
                "pgid": self.pgid,
                "reason": "run canceled during shutdown",
            })
            .to_string();
            record_service_canceling(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                &payload,
            )?;
            self.monitor.stop();
            mark_service_canceled(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                &payload,
            )?;
            let error = canceled_error();
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(error);
        }
        if let Some(error) = self.escape_error(registry) {
            self.cleanup_after_escape();
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(error);
        }
        self.monitor.stop();
        mark_service_stopped(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
        )?;
        record_lifecycle_success(registry, &context, &stop_record)
    }

    fn escape_error(&mut self, registry: &mut Registry) -> Option<RuntimeError> {
        let escaped = self.monitor.escaped_descendants();
        if escaped.is_empty() {
            return None;
        }
        let escaped_pids = escaped
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        let message = format!(
            "service process {} has escaped descendants outside pgid {}: {:?}",
            self.pid, self.pgid, escaped_pids
        );
        Some(self.record_escape(registry, message, escaped_pids))
    }

    fn record_escape(
        &mut self,
        registry: &mut Registry,
        message: String,
        escaped: Vec<u32>,
    ) -> RuntimeError {
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "escapedDescendants": escaped,
            "reason": message,
        })
        .to_string();
        let _ = mark_process_escape(
            registry,
            &self.process_key,
            &self.run_id,
            &self.service_instance_id,
            &self.computed_model_hash,
            &payload,
        );
        RuntimeError::new(ErrorCode::ProcEscape, message)
    }

    fn ensure_alive_or_record_escape(&mut self, registry: &mut Registry) -> RuntimeResult<()> {
        match ensure_foreground_child_alive(self) {
            Ok(()) => Ok(()),
            Err(error) => {
                let recorded = self.record_escape(registry, error.message, Vec::new());
                self.cleanup_after_escape();
                Err(recorded)
            }
        }
    }

    fn cleanup_after_escape(&mut self) {
        let mut descendants = self
            .monitor
            .known_descendants()
            .into_iter()
            .map(|process| (process.pid, process))
            .collect::<BTreeMap<_, _>>();
        if let Ok(current) = descendant_pids(self.pid) {
            for pid in current {
                descendants
                    .entry(pid)
                    .or_insert_with(|| monitored_process(pid));
            }
        }
        best_effort_kill_processes(&descendants.into_values().collect::<Vec<_>>());
        let _ = self.terminate_owned(1000);
        let _ = self.child.wait();
        self.monitor.stop();
    }

    fn cleanup_after_probe_failure(&mut self, registry: &mut Registry, error: &RuntimeError) {
        let _ = self.terminate_owned(1000);
        let _ = wait_for_child_exit(&mut self.child, 1000);
        self.monitor.stop();
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "errorCode": error.code,
            "message": error.message.as_str(),
        })
        .to_string();
        let _ = mark_service_failed(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            &payload,
        );
    }

    /// Record honest evidence of the stop mechanism that actually ran — the
    /// signal sent and whether it escalated to SIGKILL — rather than a fabricated
    /// exec terminal. Best-effort; failure to record does not fail the stop.
    fn record_stop_signaled(&self, registry: &mut Registry, escalated: bool, timeout_ms: u64) {
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "signal": self.service.stop.signal.name(),
            "signalNumber": self.service.stop.signal.libc(),
            "escalatedToKill": escalated,
            "timeoutMs": timeout_ms,
        })
        .to_string();
        let _ = record_service_lifecycle_event(
            registry,
            "service.stop.signaled",
            Some(self.run_id.as_str()),
            &self.service_instance_id,
            Some(self.process_key.as_str()),
            &self.computed_model_hash,
            &payload,
        );
    }

    /// Terminate the owned process(es) according to containment: a single
    /// process group, or the whole supervised process tree.
    fn terminate_owned(&self, timeout_ms: u64) -> RuntimeResult<()> {
        match self.service.containment {
            ContainmentRequirement::ProcessGroup => terminate_process_group(self.pgid, timeout_ms),
            ContainmentRequirement::ProcessTree => {
                terminate_process_tree(self.pid, self.pgid, timeout_ms)
            }
        }
    }

    /// Graceful shutdown: signal the owned process(es) with the model's declared
    /// stop signal, then escalate to SIGKILL after the budget. Returns `true` if
    /// escalation to SIGKILL was required.
    fn stop_owned(&self, signal: i32, timeout_ms: u64) -> RuntimeResult<bool> {
        match self.service.containment {
            ContainmentRequirement::ProcessGroup => {
                terminate_process_group_signal(self.pgid, signal, timeout_ms)
            }
            ContainmentRequirement::ProcessTree => {
                terminate_process_tree_signal(self.pid, self.pgid, signal, timeout_ms)
            }
        }
    }

    /// The service name this instance was started from.
    pub fn service_name(&self) -> &str {
        self.service.name.as_str()
    }

    fn lifecycle_event_context(&self) -> LifecycleEventContext {
        LifecycleEventContext {
            run_id: Some(self.run_id.clone()),
            service_instance_id: self.service_instance_id.clone(),
            process_key: Some(self.process_key.clone()),
            computed_model_hash: self.computed_model_hash.clone(),
        }
    }

    /// Verify that every modelled endpoint's listener is owned by this service's
    /// process group, returning each endpoint's registry key and ownership JSON so
    /// the caller can mark them. Every declared listener is proven — not just the
    /// primary — so a service cannot pass readiness while a modelled port is held
    /// by someone else. On any failure an escape is recorded first if present.
    fn verify_endpoint_ownerships(
        &mut self,
        registry: &mut Registry,
    ) -> RuntimeResult<Vec<(String, String)>> {
        let endpoints = self.selected_endpoints.clone();
        let mut verified = Vec::with_capacity(endpoints.len());
        for endpoint in &endpoints {
            let ownership = verify_endpoint_ownership(
                &endpoint.endpoint_id,
                &endpoint.host,
                endpoint.port,
                &ExpectedEndpointOwner {
                    pid: self.pid,
                    pgid: self.pgid,
                    process_key: &self.process_key,
                    platform_start_identity: self.platform_start_identity.as_deref(),
                },
            )
            .map_err(|error| {
                if let Some(error) = self.escape_error(registry) {
                    self.cleanup_after_escape();
                    return error;
                }
                error
            })?;
            let json = serde_json::to_string(&ownership)
                .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
            verified.push((
                endpoint_key(&self.service_instance_id, &endpoint.endpoint_id),
                json,
            ));
        }
        Ok(verified)
    }
}

impl Drop for StartedService {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = signal_process_group(self.pgid, libc::SIGTERM);
            if wait_for_child_exit(&mut self.child, 100).ok() != Some(true) {
                let _ = signal_process_group(self.pgid, libc::SIGKILL);
                let _ = wait_for_child_exit(&mut self.child, 1000);
            }
        }
        self.monitor.stop();
    }
}

/// One service's slice of the slot plan: its name, the planned port for each of
/// its endpoints (keyed by endpointId), and the cross-service slot endpoint map
/// `${port:<serviceId>}` resolves against.
pub struct ServiceSelection<'a> {
    pub service_name: &'a str,
    pub endpoint_ports: &'a BTreeMap<String, u16>,
    pub slot_endpoints: &'a SlotEndpoints,
}

/// Start a declared foreground service from the lowered model: run prepare,
/// spawn-and-own the start exec, and track the process. The service is read from
/// the admission's `ExecutionModel`, never the raw `Model`.
pub fn start_service_for_slot(
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_slot: &SelectedSlot<'_>,
    selection: &ServiceSelection<'_>,
) -> RuntimeResult<StartedService> {
    let run_id = run_id.into();
    let source = admission.require_source()?;
    let ServiceSelection {
        service_name,
        endpoint_ports,
        slot_endpoints,
    } = *selection;
    let service = admission
        .execution_model
        .services
        .get(service_name)
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("service {service_name} is missing"),
            )
        })?;
    // Bind every modelled endpoint to its planned port. The map is keyed by
    // endpointId, the scope `${port:<endpointId>}` resolves against.
    let mut own_endpoints: BTreeMap<String, SelectedEndpoint> = BTreeMap::new();
    for (endpoint_id, endpoint) in &service.endpoints {
        let port = endpoint_ports.get(endpoint_id).copied().ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("service {service_name} endpoint {endpoint_id} has no planned port"),
            )
        })?;
        own_endpoints.insert(
            endpoint_id.clone(),
            SelectedEndpoint {
                endpoint_id: endpoint.endpoint_id.clone(),
                host: endpoint.host.to_string(),
                port,
            },
        );
    }
    let selected_endpoint = own_endpoints
        .get(&service.primary_endpoint)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!(
                    "service {service_name} primary endpoint {} is missing",
                    service.primary_endpoint
                ),
            )
        })?;
    let selected_endpoints: Vec<SelectedEndpoint> = own_endpoints.values().cloned().collect();
    // The named substitution scope is the declared connectsTo set; lowering
    // proved every cross-service placeholder references a member of it.
    let named: SlotEndpoints = slot_endpoints
        .iter()
        .filter(|(id, _)| service.connects_to.contains(id))
        .map(|(id, endpoint)| (id.clone(), endpoint.clone()))
        .collect();
    let substitution = ExecSubstitution {
        own_primary: Some(&selected_endpoint),
        own_endpoints: &own_endpoints,
        named: &named,
        state_root: &placement.state_root,
    };
    // Exec probe args/env are substituted once here, with the same scope as the
    // start exec, so probe attempts later need no endpoint context.
    let ready_probe = substituted_probe(&service.ready.probe, &substitution)?;
    let health_probe = substituted_probe(&service.health.probe, &substitution)?;
    let address_hash = service_address_hash(
        &admission.project_id,
        selected_slot.environment,
        selected_slot.slot,
        service_name,
    );
    let service_instance_id = service_instance_id(&address_hash, &service.identity);
    // Stable backing storage for the per-endpoint reservation keys.
    let reservation_keys: Vec<(String, String, u16)> = selected_endpoints
        .iter()
        .map(|endpoint| {
            (
                endpoint_key(&service_instance_id, &endpoint.endpoint_id),
                endpoint.host.clone(),
                endpoint.port,
            )
        })
        .collect();
    let reservations: Vec<PortReservation<'_>> = reservation_keys
        .iter()
        .map(|(key, address, port)| PortReservation {
            endpoint_key: key,
            address,
            port: *port,
        })
        .collect();
    reconcile_registry(registry)?;
    ensure_service_start_allowed(registry, &run_id, &service_instance_id)?;
    let owner_token = run_owner_token(&run_id);
    // Reserve the service instance (run row + active lease) AND every endpoint port
    // under the start conflict gates before running any state-mutating lifecycle
    // work, so a second runtime racing the same slot — or any modelled port already
    // held by another active service — is refused before prepare (e.g. initdb) and
    // spawn run, never discovered afterward. Released on failure below.
    reserve_service_start(
        registry,
        &RunRecord {
            run_id: &run_id,
            owner_token: &owner_token,
            admission,
            placement,
        },
        &service_instance_id,
        &reservations,
    )?;
    let lifecycle_context = LifecycleEventContext {
        run_id: Some(run_id.clone()),
        service_instance_id: service_instance_id.clone(),
        process_key: None,
        computed_model_hash: admission.computed_model_hash.clone(),
    };
    let prepare_record = LifecycleRecord::from_meta(&service.prepare.meta, "prepare");
    record_lifecycle_started(registry, &lifecycle_context, &prepare_record)?;
    if let Some(prepare_exec) = &service.prepare.exec {
        // The run-level heartbeat only starts once this function returns, and
        // the prepare process is not recorded in the registry, so a prepare
        // longer than the lease TTL would let another runtime stale the lease
        // and run a concurrent prepare against the same state. Keep the
        // just-created lease alive with a scoped heartbeat for the duration.
        let prepare_heartbeat = RunLeaseHeartbeat::start(
            placement.registry_path().to_path_buf(),
            registry.identity().clone(),
            run_id.clone(),
            owner_token.clone(),
        );
        let prepare_result = run_resolved_exec(
            prepare_exec,
            &source.observed_root,
            &placement.logs_dir,
            service.prepare.meta.operation_id.as_str(),
            &substitution,
            &CancellationToken::new(),
        );
        let heartbeat_result = prepare_heartbeat.stop();
        if let Err(error) = prepare_result.and(heartbeat_result) {
            let _ = record_lifecycle_failure(registry, &lifecycle_context, &prepare_record, &error);
            let _ = release_service_reservation(registry, &run_id, &service_instance_id);
            return Err(error);
        }
    }
    record_lifecycle_success(registry, &lifecycle_context, &prepare_record)?;
    let start_record = LifecycleRecord::from_meta(&service.start.meta, "start");
    record_lifecycle_started(registry, &lifecycle_context, &start_record)?;
    let exec = &service.start.exec;
    let command_cwd = resolve_exec_cwd(&source.observed_root, &exec.cwd)?;
    let args = substitution.args(&exec.args)?;
    let env = substitution.env(&exec.env)?;
    let stdout_path = placement
        .logs_dir
        .join(format!("service.{service_name}.stdout.log"));
    let stderr_path = placement
        .logs_dir
        .join(format!("service.{service_name}.stderr.log"));
    let command_json = serde_json::to_string(&CommandRecord {
        executable: exec.executable.as_str(),
        args: &args,
        cwd: command_cwd.as_path(),
        stdout_path: stdout_path.as_path(),
        stderr_path: stderr_path.as_path(),
    })
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    let mut command = Command::new(&exec.executable);
    command
        .args(&args)
        .current_dir(&command_cwd)
        .envs(&env)
        .stdin(stdin_for(exec.stdin))
        .stdout(Stdio::from(create_log_file(&stdout_path)?))
        .stderr(Stdio::from(create_log_file(&stderr_path)?));
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let error = RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to spawn service {service_name}: {error}"),
            );
            let _ = record_lifecycle_failure(registry, &lifecycle_context, &start_record, &error);
            let _ = release_service_reservation(registry, &run_id, &service_instance_id);
            return Err(error);
        }
    };
    let pid = child.id();
    let pgid = match get_process_group(pid) {
        Ok(pgid) => pgid,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = record_lifecycle_failure(registry, &lifecycle_context, &start_record, &error);
            let _ = release_service_reservation(registry, &run_id, &service_instance_id);
            return Err(error);
        }
    };
    let platform_start = platform_start_identity(pid);
    let start_identity = process_start_identity(pid, pgid, platform_start.as_deref());
    let process_key = format!("process-{run_id}-{pid}-{pgid}");
    let started_context = LifecycleEventContext {
        run_id: Some(run_id.clone()),
        service_instance_id: service_instance_id.clone(),
        process_key: Some(process_key.clone()),
        computed_model_hash: admission.computed_model_hash.clone(),
    };
    let endpoint_json = serde_json::to_string(&selected_endpoint)
        .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    if let Err(error) = record_service_start(
        registry,
        &RunRecord {
            run_id: &run_id,
            owner_token: &owner_token,
            admission,
            placement,
        },
        &ServiceRecord {
            service_instance_id: &service_instance_id,
            service_name,
            service_address_hash: &address_hash,
            identity: &service.identity,
            endpoint_json: &endpoint_json,
            state_root: &placement.state_root,
        },
        &ProcessRecord {
            process_key: &process_key,
            pid,
            pgid,
            start_identity: &start_identity,
            command_json: &command_json,
            run_id: &run_id,
            service_instance_id: &service_instance_id,
        },
    ) {
        let _ = terminate_process_group(pgid, 1000);
        let _ = child.wait();
        let _ = record_lifecycle_failure(registry, &started_context, &start_record, &error);
        let _ = release_service_reservation(registry, &run_id, &service_instance_id);
        return Err(error);
    }
    let strict_process_group = matches!(service.containment, ContainmentRequirement::ProcessGroup);
    let monitor = spawn_process_monitor(pid, pgid, strict_process_group);
    let mut started = StartedService {
        child,
        monitor,
        service: service.clone(),
        ready_probe,
        health_probe,
        logs_dir: placement.logs_dir.clone(),
        run_id,
        service_instance_id,
        process_key,
        pid,
        pgid,
        platform_start_identity: platform_start,
        selected_endpoint,
        selected_endpoints,
        computed_model_hash: admission.computed_model_hash.clone(),
        source_root: source.observed_root.clone(),
        state_root: placement.state_root.clone(),
        owner_token,
    };
    if let Err(error) = ensure_foreground_child_alive(&mut started) {
        let payload = serde_json::json!({
            "pid": started.pid,
            "pgid": started.pgid,
            "reason": error.message,
        })
        .to_string();
        let _ = mark_process_escape(
            registry,
            &started.process_key,
            &started.run_id,
            &started.service_instance_id,
            &started.computed_model_hash,
            &payload,
        );
        started.cleanup_after_escape();
        let _ = record_lifecycle_failure(registry, &started_context, &start_record, &error);
        return Err(error);
    }
    record_lifecycle_success(registry, &started_context, &start_record)?;
    Ok(started)
}

/// Clean every service declared in the selected slot's environment, then clean
/// the marker-owned slot state once. Each service's clean lifecycle operation is
/// a marker-gated runtime cleanup primitive (no exec).
pub fn run_slot_clean(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
) -> RuntimeResult<CleanupOutcome> {
    if let Some(env) = model.environments.get(selected_slot.environment) {
        for service_name in &env.services {
            record_service_clean(
                model,
                admission,
                registry,
                selected_slot,
                service_name.as_str(),
            )?;
        }
    }
    clean_marked_slot_state(model, admission, placement, registry, selected_slot)
}

/// Record the marker-gated clean lifecycle operation for one service.
fn record_service_clean(
    model: &Model,
    admission: &Admission,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
    service_name: &str,
) -> RuntimeResult<()> {
    let service = model.services.get(service_name).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("service {service_name} is missing"),
        )
    })?;
    let record = LifecycleRecord::from_clean(&service.lifecycle.clean);
    let address_hash = service_address_hash(
        &model.project.project_id,
        selected_slot.environment,
        selected_slot.slot,
        service_name,
    );
    // Recompute the same identity the lowering derived for this service so the
    // clean path keys on the exact registry instance the start path created.
    let identity = compute_service_identity(service, &model.execs, &model.state, &model.target);
    let service_instance_id = service_instance_id(&address_hash, &identity);
    let lifecycle_context = LifecycleEventContext {
        run_id: None,
        service_instance_id,
        process_key: None,
        computed_model_hash: admission.computed_model_hash.clone(),
    };
    record_lifecycle_started(registry, &lifecycle_context, &record)?;
    record_lifecycle_success(registry, &lifecycle_context, &record)
}

/// Clean the marker-owned state root for the selected slot.
fn clean_marked_slot_state(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
) -> RuntimeResult<CleanupOutcome> {
    let identity = StateIdentity::from_selected_slot(model, admission, selected_slot);
    // Reconcile first so rows left active by a crashed runtime (no live OS
    // process) are marked stale instead of tripping the active-refs refusal,
    // sparing the operator a manual ps/down before clean can proceed.
    reconcile_registry(registry)?;
    clean_marked_state(
        &placement.state_base,
        &placement.state_root,
        &identity,
        registry,
    )
}

fn run_owner_token(run_id: &str) -> String {
    format!("{run_id}:runtime-pid-{}", std::process::id())
}

/// The slot plan's endpoint map: every service selected for the run, resolved
/// to its deterministic host/port before anything spawns. Named placeholder
/// substitution addresses this map by service id.
pub type SlotEndpoints = std::collections::BTreeMap<nixfied_model::ServiceId, SelectedEndpoint>;

/// Placeholder substitution shared by lifecycle and task exec args/env values.
/// Bare `${port}`/`${host}` resolve to `own_primary` (the exec's own primary
/// endpoint for a service, the primary dependency for a task). `${port:<name>}` /
/// `${host:<name>}` resolve `<name>` first against `own_endpoints` (the service's
/// own endpoints by id), then against `named` (the connectsTo/dependency slot-plan
/// endpoints by service id). `${stateDir}` resolves to the host-materialised slot
/// state root. Lowering already proved every named reference is declared, so a
/// leftover named placeholder here is a leak — fail closed rather than hand the
/// literal string to the child.
pub(crate) struct ExecSubstitution<'a> {
    pub own_primary: Option<&'a SelectedEndpoint>,
    pub own_endpoints: &'a BTreeMap<String, SelectedEndpoint>,
    pub named: &'a SlotEndpoints,
    pub state_root: &'a Path,
}

impl ExecSubstitution<'_> {
    pub(crate) fn value(&self, value: &str) -> RuntimeResult<String> {
        let mut out = value.to_string();
        // Own endpoints win the `${port:<name>}` namespace; validation proved no
        // own endpointId collides with a connectsTo serviceId, so order is moot
        // for correctness, but resolving own first keeps the intent explicit.
        for (endpoint_id, endpoint) in self.own_endpoints {
            out = out
                .replace(
                    &format!("${{port:{endpoint_id}}}"),
                    &endpoint.port.to_string(),
                )
                .replace(&format!("${{host:{endpoint_id}}}"), &endpoint.host);
        }
        for (service, endpoint) in self.named {
            out = out
                .replace(
                    &format!("${{port:{}}}", service.as_str()),
                    &endpoint.port.to_string(),
                )
                .replace(&format!("${{host:{}}}", service.as_str()), &endpoint.host);
        }
        if let Some(own) = self.own_primary {
            out = out
                .replace("${port}", &own.port.to_string())
                .replace("${host}", &own.host);
        }
        out = out.replace("${stateDir}", &self.state_root.to_string_lossy());
        if out.contains("${port:") || out.contains("${host:") {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("unresolved endpoint placeholder in exec value: {value}"),
            ));
        }
        Ok(out)
    }

    pub(crate) fn args(&self, args: &[String]) -> RuntimeResult<Vec<String>> {
        args.iter().map(|arg| self.value(arg)).collect()
    }

    pub(crate) fn env(
        &self,
        env: &BTreeMap<String, String>,
    ) -> RuntimeResult<BTreeMap<String, String>> {
        env.iter()
            .map(|(key, value)| Ok((key.clone(), self.value(value)?)))
            .collect()
    }
}

pub(crate) fn resolve_exec_cwd(source_root: &Path, exec_cwd: &str) -> RuntimeResult<PathBuf> {
    let relative = Path::new(exec_cwd);
    if exec_cwd.is_empty()
        || relative.is_absolute()
        || relative.components().any(disallowed_component)
    {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("exec cwd must be a confined relative path: {exec_cwd}"),
        ));
    }
    let source_root = source_root.canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "failed to canonicalize admitted source root {}: {error}",
                source_root.display()
            ),
        )
    })?;
    let cwd = source_root.join(relative).canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("failed to resolve exec cwd {exec_cwd}: {error}"),
        )
    })?;
    if !cwd.is_dir() || !cwd.starts_with(&source_root) {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("exec cwd {} escaped admitted source root", cwd.display()),
        ));
    }
    Ok(cwd)
}

/// Clone a probe with its exec args/env substituted against the slot plan. A
/// tcp probe carries no substitutable values and passes through unchanged.
fn substituted_probe(probe: &Probe, substitution: &ExecSubstitution<'_>) -> RuntimeResult<Probe> {
    match probe {
        Probe::Tcp(tcp) => Ok(Probe::Tcp(tcp.clone())),
        Probe::Exec(exec_probe) => {
            let mut exec = exec_probe.exec.clone();
            exec.args = substitution.args(&exec.args)?;
            exec.env = substitution.env(&exec.env)?;
            Ok(Probe::Exec(ExecProbe {
                exec,
                ..exec_probe.clone()
            }))
        }
    }
}

/// A fully-substituted short-lived command with its own kill-after deadline and
/// capture paths: the shared spawn/wait core of lifecycle execs and exec probe
/// attempts.
pub(crate) struct BoundedExec<'a> {
    pub executable: &'a str,
    pub args: &'a [String],
    pub env: &'a BTreeMap<String, String>,
    pub cwd: &'a Path,
    pub stdin: StdinPolicy,
    pub timeout: Duration,
    pub stdout_path: &'a Path,
    pub stderr_path: &'a Path,
    /// Names the operation in spawn/inspect failures.
    pub label: &'a str,
}

pub(crate) enum BoundedExecOutcome {
    Exited(std::process::ExitStatus),
    TimedOut,
}

/// Spawn the bounded exec in its own process group, wait for exit or deadline
/// (killing the group on timeout or cancellation), and report how it ended.
/// Exit-status policy is the caller's: a lifecycle exec treats non-zero as
/// failure, a probe attempt treats it as retry.
pub(crate) fn run_bounded_exec(
    spec: &BoundedExec<'_>,
    cancellation: &CancellationToken,
) -> RuntimeResult<BoundedExecOutcome> {
    let label = spec.label;
    let mut command = Command::new(spec.executable);
    command
        .args(spec.args)
        .current_dir(spec.cwd)
        .envs(spec.env)
        .stdin(stdin_for(spec.stdin))
        .stdout(Stdio::from(create_log_file(spec.stdout_path)?))
        .stderr(Stdio::from(create_log_file(spec.stderr_path)?));
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = command.spawn().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to spawn lifecycle operation {label}: {error}"),
        )
    })?;
    let pgid = match get_process_group(child.id()) {
        Ok(pgid) => pgid,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let deadline = Instant::now() + spec.timeout;
    loop {
        if cancellation.is_canceled() {
            let _ = terminate_process_group(pgid, 1000);
            let _ = wait_for_child_exit(&mut child, 1000);
            return Err(canceled_error());
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect lifecycle operation {label}: {error}"),
            )
        })? {
            return Ok(BoundedExecOutcome::Exited(status));
        }
        if Instant::now() >= deadline {
            let _ = terminate_process_group(pgid, 1000);
            let _ = wait_for_child_exit(&mut child, 1000);
            return Ok(BoundedExecOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Run a resolved lifecycle exec (prepare) to completion in its own process
/// group, capturing output to the logs dir keyed by `label` so a failed prepare
/// (e.g. initdb) leaves a recoverable trail. Honors cancellation and the exec
/// timeout.
fn run_resolved_exec(
    exec: &ResolvedExec,
    source_root: &Path,
    logs_dir: &Path,
    label: &str,
    substitution: &ExecSubstitution<'_>,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    let command_cwd = resolve_exec_cwd(source_root, &exec.cwd)?;
    let args = substitution.args(&exec.args)?;
    let env = substitution.env(&exec.env)?;
    let stdout_path = logs_dir.join(format!("lifecycle.{label}.stdout.log"));
    let stderr_path = logs_dir.join(format!("lifecycle.{label}.stderr.log"));
    let outcome = run_bounded_exec(
        &BoundedExec {
            executable: &exec.executable,
            args: &args,
            env: &env,
            cwd: &command_cwd,
            stdin: exec.stdin,
            timeout: exec.timeout,
            stdout_path: &stdout_path,
            stderr_path: &stderr_path,
            label,
        },
        cancellation,
    )?;
    match outcome {
        BoundedExecOutcome::Exited(status) if status.success() => Ok(()),
        BoundedExecOutcome::Exited(status) => Err(RuntimeError::new(
            ErrorCode::LifecycleFailed,
            format!(
                "service lifecycle operation {label} exited with code {}",
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
        )),
        BoundedExecOutcome::TimedOut => Err(RuntimeError::new(
            ErrorCode::LifecycleFailed,
            format!(
                "service lifecycle operation {label} timed out after {}ms",
                exec.timeout.as_millis()
            ),
        )),
    }
}

fn disallowed_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::ParentDir | Component::RootDir | Component::Prefix(_)
    )
}

fn endpoint_key(service_instance_id: &str, endpoint_id: &str) -> String {
    format!("{service_instance_id}:{endpoint_id}")
}

struct LifecycleEventContext {
    run_id: Option<String>,
    service_instance_id: String,
    process_key: Option<String>,
    computed_model_hash: String,
}

/// A lifecycle operation's identity and terminal semantics for durable event
/// recording, owned so it does not borrow the `StartedService` across the
/// `&mut self` calls in the lifecycle methods.
struct LifecycleRecord {
    operation_id: String,
    class: &'static str,
    terminal_success: String,
    terminal_failure: String,
}

impl LifecycleRecord {
    fn from_meta(meta: &OpMeta, class: &'static str) -> Self {
        Self {
            operation_id: meta.operation_id.as_str().to_string(),
            class,
            terminal_success: meta.terminal_success.clone(),
            terminal_failure: meta.terminal_failure.clone(),
        }
    }

    fn from_clean(clean: &nixfied_model::CleanSpec) -> Self {
        Self {
            operation_id: clean.operation_id.as_str().to_string(),
            class: "clean",
            terminal_success: clean.terminal.success.clone(),
            terminal_failure: clean.terminal.failure.clone(),
        }
    }
}

fn record_lifecycle_started(
    registry: &mut Registry,
    context: &LifecycleEventContext,
    record: &LifecycleRecord,
) -> RuntimeResult<()> {
    let payload_json = serde_json::to_string(&serde_json::json!({
        "operationId": record.operation_id,
        "class": record.class,
    }))
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    record_service_lifecycle_event(
        registry,
        "service.lifecycle.started",
        context.run_id.as_deref(),
        &context.service_instance_id,
        context.process_key.as_deref(),
        &context.computed_model_hash,
        &payload_json,
    )
}

fn record_lifecycle_success(
    registry: &mut Registry,
    context: &LifecycleEventContext,
    record: &LifecycleRecord,
) -> RuntimeResult<()> {
    record_lifecycle_terminal(registry, context, record, &record.terminal_success, None)
}

fn record_lifecycle_failure(
    registry: &mut Registry,
    context: &LifecycleEventContext,
    record: &LifecycleRecord,
    error: &RuntimeError,
) -> RuntimeResult<()> {
    record_lifecycle_terminal(
        registry,
        context,
        record,
        &record.terminal_failure,
        Some(error),
    )
}

fn record_lifecycle_terminal(
    registry: &mut Registry,
    context: &LifecycleEventContext,
    record: &LifecycleRecord,
    terminal_result: &str,
    error: Option<&RuntimeError>,
) -> RuntimeResult<()> {
    let payload_json = serde_json::to_string(&serde_json::json!({
        "operationId": record.operation_id,
        "class": record.class,
        "terminalResult": terminal_result,
        "errorCode": error.map(|error| error.code),
        "message": error.map(|error| error.message.as_str()),
    }))
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    record_service_lifecycle_event(
        registry,
        "service.lifecycle.terminal",
        context.run_id.as_deref(),
        &context.service_instance_id,
        context.process_key.as_deref(),
        &context.computed_model_hash,
        &payload_json,
    )
}

fn create_log_file(path: &Path) -> RuntimeResult<File> {
    File::create(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to create log file {}: {error}", path.display()),
        )
    })
}

fn get_process_group(pid: u32) -> RuntimeResult<i32> {
    process_group(pid)?.ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("process {pid} no longer exists"),
        )
    })
}

pub(crate) fn process_group(pid: u32) -> RuntimeResult<Option<i32>> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect process group for pid {pid}: {}", error),
        ))
    } else {
        Ok(Some(pgid))
    }
}

pub(crate) fn process_is_live_with_identity(
    pid: u32,
    pgid: i32,
    platform_start: Option<&str>,
) -> RuntimeResult<bool> {
    let Some(current_pgid) = process_group(pid)? else {
        return Ok(false);
    };
    if current_pgid != pgid {
        return Ok(false);
    }
    if let Some(expected) = platform_start
        && platform_start_identity(pid).as_deref() != Some(expected)
    {
        return Ok(false);
    }
    Ok(!process_is_zombie(pid))
}

pub(crate) fn process_group_has_live_member(pgid: i32) -> RuntimeResult<bool> {
    process_group_has_live_member_impl(pgid)
}

fn ensure_foreground_child_alive(service: &mut StartedService) -> RuntimeResult<()> {
    thread::sleep(FOREGROUND_GRACE);
    match service.child.try_wait().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect service child: {error}"),
        )
    })? {
        None => Ok(()),
        Some(status) => Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("foreground service exited before handoff: {status}"),
        )),
    }?;
    let current_pgid = get_process_group(service.pid)?;
    if current_pgid != service.pgid {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "foreground service process {} moved from pgid {} to pgid {current_pgid}",
                service.pid, service.pgid
            ),
        ));
    }
    // Under process-tree containment the supervisor's children legitimately form
    // their own process groups, so only strict process-group services are held to
    // the single-group invariant here.
    if matches!(
        service.service.containment,
        ContainmentRequirement::ProcessGroup
    ) {
        let escaped = escaped_descendants(service.pid, service.pgid)?;
        if !escaped.is_empty() {
            return Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!(
                    "service process {} has descendants outside pgid {}: {:?}",
                    service.pid, service.pgid, escaped
                ),
            ));
        }
    }
    Ok(())
}

/// Terminate a process tree rooted at a supervisor whose children may live in
/// their own process groups. SIGTERM the supervisor's group first (a well-behaved
/// supervisor shuts its tree down), then escalate to SIGKILL across the tree.
pub(crate) fn terminate_process_tree(pid: u32, pgid: i32, timeout_ms: u64) -> RuntimeResult<()> {
    terminate_process_tree_signal(pid, pgid, libc::SIGTERM, timeout_ms).map(|_| ())
}

/// Signal the supervisor's group with `signal`, wait for the tree to drain, then
/// escalate to SIGKILL across the tree. Returns `true` if escalation was required.
pub(crate) fn terminate_process_tree_signal(
    pid: u32,
    pgid: i32,
    signal: i32,
    timeout_ms: u64,
) -> RuntimeResult<bool> {
    // Snapshot owned descendants with their start identities BEFORE signaling. A
    // process-tree child may reparent to init and move to its own group after the
    // supervisor exits, making it invisible to a descendant/pgid scan; the
    // snapshot keeps it tracked, and the identity makes the tracking pid-reuse
    // safe (a recycled pid has a different start identity).
    let snapshot: Vec<(u32, Option<String>)> = descendant_pids(pid)
        .unwrap_or_default()
        .into_iter()
        .map(|child| (child, platform_start_identity(child)))
        .collect();
    signal_process_group(pgid, signal)?;
    if wait_until_process_tree_empty(pid, pgid, &snapshot, timeout_ms)? {
        return Ok(false);
    }
    kill_snapshot_survivors(&snapshot);
    for descendant in descendant_pids(pid).unwrap_or_default() {
        unsafe {
            libc::kill(descendant as libc::pid_t, libc::SIGKILL);
        }
    }
    signal_process_group(pgid, libc::SIGKILL)?;
    if wait_until_process_tree_empty(pid, pgid, &snapshot, 1000)? {
        Ok(true)
    } else {
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to terminate owned process tree rooted at {pid}"),
        ))
    }
}

/// `true` if the snapshotted pid is still the *same* live process — including one
/// that escaped to its own process group. A dead, zombie, or pid-reused entry
/// (start identity no longer matches) is treated as gone.
fn snapshot_member_alive(pid: u32, expected: &Option<String>) -> bool {
    match expected {
        Some(identity) => {
            !process_is_zombie(pid)
                && platform_start_identity(pid).as_deref() == Some(identity.as_str())
        }
        None => false,
    }
}

/// SIGKILL every snapshotted descendant still running as its original process,
/// and the group it escaped into, so an owned child cannot outlive `stop`/`down`.
fn kill_snapshot_survivors(snapshot: &[(u32, Option<String>)]) {
    for (pid, identity) in snapshot {
        if !snapshot_member_alive(*pid, identity) {
            continue;
        }
        if let Ok(Some(group)) = process_group(*pid) {
            let _ = signal_process_group(group, libc::SIGKILL);
        }
        unsafe {
            libc::kill(*pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

fn wait_until_process_tree_empty(
    pid: u32,
    pgid: i32,
    snapshot: &[(u32, Option<String>)],
    timeout_ms: u64,
) -> RuntimeResult<bool> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let root_alive = process_group(pid)?.is_some() && !process_is_zombie(pid);
        let descendants_alive = descendant_pids(pid)
            .unwrap_or_default()
            .into_iter()
            .any(|child| {
                process_group(child)
                    .ok()
                    .flatten()
                    .is_some_and(|_| !process_is_zombie(child))
            });
        // A child that escaped to its own group after reparenting is neither a
        // current descendant of `pid` nor in the supervisor's group, so the
        // snapshot is the only thing that still sees it.
        let escapee_alive = snapshot
            .iter()
            .any(|(child, identity)| snapshot_member_alive(*child, identity));
        if !root_alive
            && !descendants_alive
            && !escapee_alive
            && !process_group_has_live_member(pgid)?
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn terminate_process_group(pgid: i32, timeout_ms: u64) -> RuntimeResult<()> {
    terminate_process_group_signal(pgid, libc::SIGTERM, timeout_ms).map(|_| ())
}

/// Signal the owned process group with `signal`, wait up to `timeout_ms` for it to
/// empty, then escalate to SIGKILL. Returns `true` if escalation was required.
pub(crate) fn terminate_process_group_signal(
    pgid: i32,
    signal: i32,
    timeout_ms: u64,
) -> RuntimeResult<bool> {
    signal_process_group(pgid, signal)?;
    if wait_until_process_group_empty(pgid, timeout_ms)? {
        return Ok(false);
    }
    signal_process_group(pgid, libc::SIGKILL)?;
    if wait_until_process_group_empty(pgid, 1000)? {
        Ok(true)
    } else {
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to terminate owned process group {pgid}"),
        ))
    }
}

pub(crate) fn wait_until_process_group_empty(pgid: i32, timeout_ms: u64) -> RuntimeResult<bool> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if !process_group_has_live_member(pgid)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn wait_for_child_exit(child: &mut Child, timeout_ms: u64) -> RuntimeResult<bool> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|error| RuntimeError::new(ErrorCode::ProcEscape, error.to_string()))?
            .is_some()
        {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(25));
    }
    Ok(false)
}

pub(crate) fn signal_process_group(pgid: i32, signal: i32) -> RuntimeResult<()> {
    let result = unsafe { libc::kill(-pgid, signal) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to signal process group {pgid}: {}",
                std::io::Error::last_os_error()
            ),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MonitoredProcess {
    pid: u32,
    platform_start: Option<String>,
}

#[derive(Default)]
struct ProcessMonitorState {
    known_descendants: BTreeMap<u32, MonitoredProcess>,
    escaped_descendants: BTreeMap<u32, MonitoredProcess>,
}

struct ProcessMonitor {
    state: Arc<Mutex<ProcessMonitorState>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProcessMonitor {
    fn escaped_descendants(&self) -> Vec<MonitoredProcess> {
        self.state
            .lock()
            .map(|state| state.escaped_descendants.values().cloned().collect())
            .unwrap_or_default()
    }

    fn known_descendants(&self) -> Vec<MonitoredProcess> {
        self.state
            .lock()
            .map(|state| state.known_descendants.values().cloned().collect())
            .unwrap_or_default()
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ProcessMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn_process_monitor(
    pid: u32,
    expected_pgid: i32,
    strict_process_group: bool,
) -> ProcessMonitor {
    let state = Arc::new(Mutex::new(ProcessMonitorState::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let thread_state = Arc::clone(&state);
    let thread_stop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            collect_process_tree(pid, expected_pgid, strict_process_group, &thread_state);
            thread::sleep(MONITOR_INTERVAL);
        }
        collect_process_tree(pid, expected_pgid, strict_process_group, &thread_state);
    });
    ProcessMonitor {
        state,
        stop,
        handle: Some(handle),
    }
}

fn collect_process_tree(
    pid: u32,
    expected_pgid: i32,
    strict_process_group: bool,
    state: &Arc<Mutex<ProcessMonitorState>>,
) {
    let Ok(descendants) = descendant_pids(pid) else {
        return;
    };
    // Under process-tree containment, supervised children may form their own
    // process groups; that is not an escape. Strict process-group services still
    // flag any descendant that leaves the owned group.
    let mut escaped = Vec::new();
    if strict_process_group {
        for descendant in &descendants {
            if let Ok(Some(pgid)) = process_group(*descendant)
                && pgid != expected_pgid
            {
                escaped.push(monitored_process(*descendant));
            }
        }
    }
    if let Ok(mut state) = state.lock() {
        for descendant in descendants {
            state
                .known_descendants
                .entry(descendant)
                .or_insert_with(|| monitored_process(descendant));
        }
        for process in escaped {
            state.escaped_descendants.insert(process.pid, process);
        }
    }
}

fn monitored_process(pid: u32) -> MonitoredProcess {
    MonitoredProcess {
        pid,
        platform_start: platform_start_identity(pid),
    }
}

fn best_effort_kill_processes(processes: &[MonitoredProcess]) {
    for process in processes {
        if same_process_identity(process) {
            unsafe {
                libc::kill(process.pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }
    thread::sleep(Duration::from_millis(25));
    for process in processes {
        if same_process_identity(process) {
            unsafe {
                libc::kill(process.pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}

fn same_process_identity(process: &MonitoredProcess) -> bool {
    let Some(expected) = process.platform_start.as_deref() else {
        return false;
    };
    platform_start_identity(process.pid).as_deref() == Some(expected)
}

fn escaped_descendants(pid: u32, expected_pgid: i32) -> RuntimeResult<Vec<u32>> {
    let mut escaped = Vec::new();
    for descendant in descendant_pids(pid)? {
        if let Some(pgid) = process_group(descendant)?
            && pgid != expected_pgid
        {
            escaped.push(descendant);
        }
    }
    Ok(escaped)
}

#[cfg(target_os = "linux")]
fn descendant_pids(pid: u32) -> RuntimeResult<Vec<u32>> {
    let mut descendants = Vec::new();
    let mut queue = vec![pid];
    while let Some(parent) = queue.pop() {
        for child in direct_child_pids_linux(parent)? {
            queue.push(child);
            descendants.push(child);
        }
    }
    Ok(descendants)
}

#[cfg(target_os = "linux")]
fn direct_child_pids_linux(parent: u32) -> RuntimeResult<Vec<u32>> {
    let mut children = Vec::new();
    let entries = std::fs::read_dir("/proc").map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect /proc for process descendants: {error}"),
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| RuntimeError::new(ErrorCode::ProcEscape, error.to_string()))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let status = std::fs::read_to_string(entry.path().join("status"));
        let Ok(status) = status else {
            continue;
        };
        let Some(ppid) = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .and_then(|value| value.trim().parse::<u32>().ok())
        else {
            continue;
        };
        if ppid == parent {
            children.push(pid);
        }
    }
    Ok(children)
}

#[cfg(target_os = "macos")]
fn descendant_pids(pid: u32) -> RuntimeResult<Vec<u32>> {
    let mut descendants = Vec::new();
    let mut queue = vec![pid];
    while let Some(parent) = queue.pop() {
        for child in direct_child_pids_macos(parent)? {
            queue.push(child);
            descendants.push(child);
        }
    }
    Ok(descendants)
}

#[cfg(target_os = "macos")]
fn direct_child_pids_macos(parent: u32) -> RuntimeResult<Vec<u32>> {
    let capacity = 8192usize;
    let mut buffer = vec![0 as libc::pid_t; capacity];
    let count = unsafe {
        libc::proc_listallpids(
            buffer.as_mut_ptr().cast(),
            (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    if count < 0 {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to list pids while inspecting descendants for pid {parent}: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let mut children = Vec::new();
    for pid in buffer.into_iter().take(count as usize) {
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let Some(info) = process_bsd_info(pid) else {
            continue;
        };
        if info.pbi_ppid == parent {
            children.push(pid);
        }
    }
    Ok(children)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn descendant_pids(_pid: u32) -> RuntimeResult<Vec<u32>> {
    Ok(Vec::new())
}

fn process_start_identity(pid: u32, pgid: i32, platform_start: Option<&str>) -> String {
    let observed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    serde_json::json!({
        "pid": pid,
        "pgid": pgid,
        "platformStart": platform_start,
        "observedAtNanos": observed_at,
    })
    .to_string()
}

#[cfg(target_os = "linux")]
pub(crate) fn platform_start_identity(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields_after_comm = stat.rsplit_once(") ")?.1;
    let start_time_ticks = fields_after_comm.split_whitespace().nth(19)?;
    Some(format!("linux-start-ticks:{start_time_ticks}"))
}

#[cfg(target_os = "linux")]
fn process_is_zombie(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat.rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().next())
        == Some("Z")
}

#[cfg(target_os = "linux")]
fn process_group_has_live_member_impl(pgid: i32) -> RuntimeResult<bool> {
    let entries = std::fs::read_dir("/proc").map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to inspect /proc while checking process group {pgid}: {error}"),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect /proc while checking process group {pgid}: {error}"),
            )
        })?;
        let Some(pid) = entry.file_name().to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        if process_group(pid)? == Some(pgid) && !process_is_zombie(pid) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_start_identity(pid: u32) -> Option<String> {
    let info = process_bsd_info(pid)?;
    Some(format!(
        "macos-start-time:{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "macos")]
fn process_is_zombie(pid: u32) -> bool {
    process_bsd_info(pid)
        .map(|info| info.pbi_status == libc::SZOMB)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn process_group_has_live_member_impl(pgid: i32) -> RuntimeResult<bool> {
    let capacity = 8192usize;
    let mut buffer = vec![0 as libc::pid_t; capacity];
    let count = unsafe {
        libc::proc_listallpids(
            buffer.as_mut_ptr().cast(),
            (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    if count < 0 {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "failed to list pids while checking process group {pgid}: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    for pid in buffer.into_iter().take(count as usize) {
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        if process_group(pid)? == Some(pgid) && !process_is_zombie(pid) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
fn process_bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let result = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if result != size {
        return None;
    }
    Some(unsafe { info.assume_init() })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn platform_start_identity(_pid: u32) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_is_zombie(_pid: u32) -> bool {
    false
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_group_has_live_member_impl(_pgid: i32) -> RuntimeResult<bool> {
    Ok(false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandRecord<'a> {
    executable: &'a str,
    args: &'a [String],
    cwd: &'a Path,
    stdout_path: &'a Path,
    stderr_path: &'a Path,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nixfied_model::ServiceId;

    fn endpoint(host: &str, port: u16) -> SelectedEndpoint {
        SelectedEndpoint {
            endpoint_id: format!("{host}:{port}"),
            host: host.to_string(),
            port,
        }
    }

    #[test]
    fn substitutes_bare_named_and_state_placeholders() {
        let own = endpoint("127.0.0.1", 23080);
        let named: SlotEndpoints = [(ServiceId::new("postgres"), endpoint("::1", 23081))]
            .into_iter()
            .collect();
        let substitution = ExecSubstitution {
            own_primary: Some(&own),
            own_endpoints: &BTreeMap::new(),
            named: &named,
            state_root: Path::new("/state"),
        };
        let value = substitution
            .value("--listen ${host}:${port} --db ${host:postgres}:${port:postgres} --data ${stateDir}")
            .expect("declared placeholders substitute");
        assert_eq!(
            value,
            "--listen 127.0.0.1:23080 --db ::1:23081 --data /state"
        );
    }

    #[test]
    fn substitutes_own_endpoints_by_id() {
        let http = endpoint("127.0.0.1", 25080);
        let own_endpoints: BTreeMap<String, SelectedEndpoint> = [
            ("http".to_string(), http.clone()),
            ("ws".to_string(), endpoint("127.0.0.1", 25081)),
            ("authrpc".to_string(), endpoint("127.0.0.1", 25082)),
        ]
        .into_iter()
        .collect();
        let substitution = ExecSubstitution {
            own_primary: Some(&http),
            own_endpoints: &own_endpoints,
            named: &SlotEndpoints::new(),
            state_root: Path::new("/state"),
        };
        let value = substitution
            .value("--http ${port} --ws ${port:ws} --auth ${port:authrpc}")
            .expect("own endpoint placeholders substitute");
        assert_eq!(value, "--http 25080 --ws 25081 --auth 25082");
    }

    #[test]
    fn env_values_are_substituted() {
        let named: SlotEndpoints = [(ServiceId::new("db"), endpoint("127.0.0.1", 23081))]
            .into_iter()
            .collect();
        let substitution = ExecSubstitution {
            own_primary: None,
            own_endpoints: &BTreeMap::new(),
            named: &named,
            state_root: Path::new("/state"),
        };
        let env: BTreeMap<String, String> = [(
            "DB_URL".to_string(),
            "tcp://${host:db}:${port:db}".to_string(),
        )]
        .into_iter()
        .collect();
        let env = substitution.env(&env).expect("env substitutes");
        assert_eq!(env["DB_URL"], "tcp://127.0.0.1:23081");
    }

    #[test]
    fn unresolved_named_placeholder_fails_closed() {
        let substitution = ExecSubstitution {
            own_primary: None,
            own_endpoints: &BTreeMap::new(),
            named: &SlotEndpoints::new(),
            state_root: Path::new("/state"),
        };
        let error = substitution
            .value("--db ${port:ghost}")
            .expect_err("an undeclared named placeholder must not leak to the child");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }
}
