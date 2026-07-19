use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nixfied_model::{ContainmentRequirement, Model, ServiceLifetime};
use rusqlite::params;
use serde::Serialize;

use crate::admission::Admission;
use crate::admission::secrets::{ResolvedSecrets, has_unclosed_secret_ref, secret_refs};
use crate::cancellation::{CancellationToken, canceled_error, sleep_cancellable};
use crate::control::reconcile_registry;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecProbe, ExecService, OpMeta, Probe, StdinPolicy};
use crate::redaction::{RedactedLogRelays, Redactor, child_output};
use crate::registry::status::{self, DbStatus, PortStatus, ServiceStatus};
use crate::registry::{Registry, RunLeaseHeartbeat};
use crate::service::endpoint::{
    EndpointFailure, EndpointLockGuards, EndpointOwnership, ExpectedOwner, ListenerRecord,
    OwnershipObservation, PlannedEndpoint, acquire_startup_locks, observe_ownership,
    observe_ownership_after_primary_exit, preflight,
};
use crate::service::identity::{
    compute_service_identity, service_address_hash, service_instance_id,
};
use crate::service::readiness::{ProbeAttempt, exec_probe_attempt, tcp_probe_attempt};
use crate::service::registry::{
    PortReservation, ProcessRecord, ReservationOutcome, RunRecord, ServiceRecord,
    ServiceReuseGuard, StoredServiceProcess, VerifiedEndpointActivation, activate_service_ready,
    ensure_service_start_allowed, guarded_claim_service_repair, mark_process_escape,
    mark_repair_target_stale, mark_service_canceled, mark_service_failed, mark_service_standing,
    mark_service_stopped, open_port_service_instances, read_service_snapshot,
    record_service_borrow, record_service_canceling, record_service_lifecycle_event,
    record_service_start, refuse_unexpired_repair_leases, release_service_borrow,
    release_unresolved_escape_ports, reserve_service_start, settle_registry_only_evidence,
    settle_service_reservation,
};
use crate::slot::SelectedSlot;
use crate::state::{CleanupMode, CleanupOutcome, HostPlacement, StateIdentity, clean_marked_state};

use super::TrackedProcessIdentity;

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
    child: Option<Child>,
    borrowed: bool,
    monitor: Option<ProcessMonitor>,
    startup_guards: Option<EndpointLockGuards>,
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
    /// The primary endpoint: the tcp probe target and the endpoint recorded as
    /// the service's address; `None` for an endpoint-less service (which can
    /// only carry invocation probes).
    pub selected_endpoint: Option<SelectedEndpoint>,
    /// Every endpoint the service binds (including the primary), each reserved and
    /// ownership-verified after readiness.
    pub selected_endpoints: Vec<SelectedEndpoint>,
    planned_endpoints: Vec<PlannedEndpoint>,
    pub computed_model_hash: String,
    pub source_root: PathBuf,
    pub state_root: PathBuf,
    pub secrets: ResolvedSecrets,
    pub redactor: Redactor,
    pub owner_token: String,
    pub service_lifetime: ServiceLifetime,
    log_relays: Option<RedactedLogRelays>,
}

impl StartedService {
    pub fn is_borrowed(&self) -> bool {
        self.borrowed
    }

    fn child_mut(&mut self) -> RuntimeResult<&mut Child> {
        self.child.as_mut().ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                "borrowed service does not own a child process",
            )
        })
    }

    pub fn wait_for_probe_ready(&mut self, registry: &mut Registry) -> RuntimeResult<()> {
        self.wait_for_probe_ready_cancellable(registry, &CancellationToken::new())
    }

    pub fn wait_for_probe_ready_cancellable(
        &mut self,
        registry: &mut Registry,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        if self.is_borrowed() {
            return Ok(());
        }
        let record = LifecycleRecord::from_meta(&self.service.ready.meta, "ready");
        let context = self.lifecycle_event_context();
        record_lifecycle_started(registry, &context, &record)?;
        let probe = self.ready_probe.clone();
        match self.wait_probe_with_ownership(registry, &probe, cancellation, Some(&record)) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = record_lifecycle_failure(registry, &context, &record, &error);
                Err(error)
            }
        }
    }

    pub fn check_health(&mut self, registry: &mut Registry) -> RuntimeResult<()> {
        self.check_health_cancellable(registry, &CancellationToken::new())
    }

    pub fn check_health_cancellable(
        &mut self,
        registry: &mut Registry,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        if self.is_borrowed() {
            return Ok(());
        }
        let record = LifecycleRecord::from_meta(&self.service.health.meta, "health");
        let context = self.lifecycle_event_context();
        record_lifecycle_started(registry, &context, &record)?;
        let probe = self.health_probe.clone();
        match self.wait_probe_with_ownership(registry, &probe, cancellation, None) {
            Ok(()) => record_lifecycle_success(registry, &context, &record),
            Err(error) => {
                let _ = record_lifecycle_failure(registry, &context, &record, &error);
                Err(error)
            }
        }
    }

    fn wait_probe_with_ownership(
        &mut self,
        registry: &mut Registry,
        probe: &Probe,
        cancellation: &CancellationToken,
        ready_record: Option<&LifecycleRecord>,
    ) -> RuntimeResult<()> {
        let (attempts, retry_interval, label) = probe_policy(probe);
        let mut last_pending = format!("probe {label} made no attempt");
        for attempt in 0..attempts {
            cancellation.check()?;
            if let Err(error) = self.ensure_start_process_live() {
                return self.override_after_primary_exit_with_endpoint_evidence(registry, error);
            }
            let probe_attempt = self.probe_attempt(probe, cancellation)?;
            cancellation.check()?;
            let observation = self.observe_endpoint_ownership();
            match classify_endpoint_observation(observation) {
                EndpointDecision::Complete(ownership) => {
                    if matches!(probe_attempt, ProbeAttempt::Succeeded) {
                        if let Some(record) = ready_record {
                            self.commit_ready(registry, &ownership, record)?;
                            self.startup_guards
                                .take()
                                .ok_or_else(|| {
                                    RuntimeError::new(
                                        ErrorCode::RegistryCorrupt,
                                        "newly started service lost its startup guards before the ready commit",
                                    )
                                })?
                                .release();
                        }
                        return Ok(());
                    }
                    if let ProbeAttempt::Failed(message) = probe_attempt {
                        last_pending = message;
                    }
                }
                EndpointDecision::Pending(endpoint) => {
                    last_pending = match &probe_attempt {
                        ProbeAttempt::Failed(message) => message.clone(),
                        ProbeAttempt::Succeeded => format!(
                            "endpoint {} has no exact listener at {}:{}",
                            endpoint.endpoint_id, endpoint.address, endpoint.port
                        ),
                    };
                }
                EndpointDecision::Conflict {
                    endpoint,
                    listeners,
                } => {
                    return Err(port_conflict_error(
                        "listener-occupied",
                        self.computed_project_id(registry),
                        &endpoint,
                        proven_nixfied_owner(registry, &endpoint, &listeners, &self.service)?
                            .as_ref(),
                    ));
                }
                EndpointDecision::Unverifiable { endpoint, message } => {
                    return Err(port_unverifiable_error(endpoint.as_ref(), message));
                }
                EndpointDecision::ContainmentUnconfirmed(message) => {
                    return Err(RuntimeError::new(ErrorCode::ProcEscape, message));
                }
            }
            if attempt + 1 < attempts {
                sleep_cancellable(retry_interval, cancellation)?;
            }
        }
        let timeout = RuntimeError::new(
            ErrorCode::ReadinessTimeout,
            format!(
                "readiness probe {label} did not reach probe-plus-ownership readiness: {last_pending}"
            ),
        );
        self.override_with_endpoint_evidence(registry, timeout)
    }

    fn probe_attempt(
        &self,
        probe: &Probe,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<ProbeAttempt> {
        match probe {
            Probe::Tcp(probe) => {
                let endpoint = self.selected_endpoint.as_ref().ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        "tcp probe on a service with no selected endpoint",
                    )
                })?;
                tcp_probe_attempt(probe, &endpoint.host, endpoint.port, cancellation)
            }
            Probe::Exec(probe) => exec_probe_attempt(
                probe,
                &self.source_root,
                &self.logs_dir,
                &self.redactor,
                cancellation,
            ),
        }
    }

    fn ensure_start_process_live(&mut self) -> RuntimeResult<()> {
        if let Some(error) = self.escape_error_unrecorded() {
            return Err(error);
        }
        if let Some(status) = self.child_mut()?.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect foreground service child: {error}"),
            )
        })? {
            return Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("foreground service exited before readiness: {status}"),
            ));
        }
        if !process_is_live_with_identity(
            self.pid,
            self.pgid,
            self.platform_start_identity.as_deref(),
        )? {
            return Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!(
                    "service process {} no longer matches its recorded containment identity",
                    self.pid
                ),
            ));
        }
        Ok(())
    }

    fn observe_endpoint_ownership(&self) -> OwnershipObservation {
        observe_ownership(
            &self.planned_endpoints,
            &ExpectedOwner {
                pid: self.pid,
                pgid: self.pgid,
                platform_start: self.platform_start_identity.as_deref(),
                containment: self.service.containment.clone(),
                tracked_processes: &[],
            },
        )
    }

    fn override_with_endpoint_evidence(
        &self,
        registry: &Registry,
        fallback: RuntimeError,
    ) -> RuntimeResult<()> {
        self.override_with_observation(registry, fallback, self.observe_endpoint_ownership())
    }

    fn override_after_primary_exit_with_endpoint_evidence(
        &self,
        registry: &Registry,
        fallback: RuntimeError,
    ) -> RuntimeResult<()> {
        let tracked_processes = self
            .monitor
            .as_ref()
            .map(ProcessMonitor::known_descendants)
            .unwrap_or_default();
        self.override_with_observation(
            registry,
            fallback,
            observe_ownership_after_primary_exit(
                &self.planned_endpoints,
                &ExpectedOwner {
                    pid: self.pid,
                    pgid: self.pgid,
                    platform_start: self.platform_start_identity.as_deref(),
                    containment: self.service.containment.clone(),
                    tracked_processes: &tracked_processes,
                },
            ),
        )
    }

    fn override_with_observation(
        &self,
        registry: &Registry,
        fallback: RuntimeError,
        observation: OwnershipObservation,
    ) -> RuntimeResult<()> {
        match classify_endpoint_observation(observation) {
            EndpointDecision::Conflict {
                endpoint,
                listeners,
            } => Err(port_conflict_error(
                "listener-occupied",
                self.computed_project_id(registry),
                &endpoint,
                proven_nixfied_owner(registry, &endpoint, &listeners, &self.service)?.as_ref(),
            )),
            EndpointDecision::Unverifiable { endpoint, message } => {
                Err(port_unverifiable_error(endpoint.as_ref(), message))
            }
            EndpointDecision::ContainmentUnconfirmed(message) => {
                Err(RuntimeError::new(ErrorCode::ProcEscape, message))
            }
            EndpointDecision::Complete(_) | EndpointDecision::Pending(_) => Err(fallback),
        }
    }

    fn commit_ready(
        &self,
        registry: &mut Registry,
        ownership: &[EndpointOwnership],
        record: &LifecycleRecord,
    ) -> RuntimeResult<()> {
        let payloads = ownership
            .iter()
            .map(|ownership| {
                serde_json::to_string(ownership)
                    .map(|payload| {
                        (
                            endpoint_key(&self.service_instance_id, &ownership.endpoint_id),
                            ownership.address.to_string(),
                            ownership.port,
                            payload,
                        )
                    })
                    .map_err(|error| {
                        RuntimeError::new(ErrorCode::ModelAdmission, error.to_string())
                    })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        let activations = payloads
            .iter()
            .map(|(key, address, port, payload)| VerifiedEndpointActivation {
                endpoint_key: key,
                address,
                port: *port,
                ownership_json: payload,
            })
            .collect::<Vec<_>>();
        activate_service_ready(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            &activations,
            (&record.operation_id, record.class, &record.terminal_success),
        )
    }

    fn computed_project_id<'a>(&self, registry: &'a Registry) -> &'a str {
        registry.identity().project_id.as_str()
    }

    pub fn finalize_failed_start(
        mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        error: RuntimeError,
    ) -> RuntimeError {
        if self.is_borrowed() {
            return match release_service_borrow(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                error.code == ErrorCode::Canceled,
            ) {
                Ok(()) => error,
                Err(settlement_error) => settlement_error,
            };
        }
        self.settle_failed_service(registry, timeout_ms, error)
    }

    fn settle_failed_service(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        error: RuntimeError,
    ) -> RuntimeError {
        if let Err(termination_error) = self.terminate_after_failure(timeout_ms) {
            let start_identity = self.escape_start_identity();
            let payload = serde_json::json!({
                "pid": self.pid,
                "pgid": self.pgid,
                "errorCode": error.code,
                "message": error.message.as_str(),
                "terminationError": termination_error.message.as_str(),
            })
            .to_string();
            let _ = mark_process_escape(
                registry,
                &self.process_key,
                &self.run_id,
                &self.service_instance_id,
                &self.computed_model_hash,
                &start_identity,
                &payload,
            );
            self.startup_guards.take();
            return RuntimeError::new(
                ErrorCode::ProcEscape,
                format!(
                    "failed to prove termination of service process tree rooted at {}: {}",
                    self.pid, termination_error.message
                ),
            );
        }
        if let Some(child) = &mut self.child {
            let _ = wait_for_child_exit(child, 1000);
        }
        if let Some(monitor) = &mut self.monitor {
            monitor.stop();
        }
        let _ = self.join_log_relays();
        let payload = serde_json::json!({
            "pid": self.pid,
            "pgid": self.pgid,
            "errorCode": error.code,
            "message": error.message.as_str(),
        })
        .to_string();
        let settlement = if error.code == ErrorCode::Canceled {
            mark_service_canceled(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                &payload,
            )
        } else {
            mark_service_failed(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                &payload,
            )
        };
        self.startup_guards.take();
        self.child = None;
        match settlement {
            Ok(()) => error,
            Err(settlement_error) => settlement_error,
        }
    }

    pub fn cancel(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        reason: &str,
    ) -> RuntimeResult<()> {
        if self.is_borrowed() {
            return release_service_borrow(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                true,
            );
        }
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
        let _ = wait_for_child_exit(self.child_mut()?, 1000)?;
        if let Some(monitor) = &mut self.monitor {
            monitor.stop();
        }
        self.join_log_relays()?;
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

    pub fn stand(mut self, registry: &mut Registry) -> RuntimeResult<()> {
        if self.is_borrowed() {
            return release_service_borrow(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                false,
            );
        }
        mark_service_standing(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
            self.service_lifetime,
        )?;
        if let Some(monitor) = &mut self.monitor {
            monitor.stop();
        }
        self.child = None;
        self.monitor = None;
        self.log_relays = None;
        Ok(())
    }

    fn stop_with_cancellation(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        cancellation: Option<&CancellationToken>,
    ) -> RuntimeResult<()> {
        if self.is_borrowed() {
            let canceled = cancellation.is_some_and(|token| token.is_canceled());
            release_service_borrow(
                registry,
                &self.run_id,
                &self.service_instance_id,
                &self.process_key,
                &self.computed_model_hash,
                canceled,
            )?;
            return if canceled {
                Err(canceled_error())
            } else {
                Ok(())
            };
        }
        let context = self.lifecycle_event_context();
        let stop_record = LifecycleRecord::from_meta(&self.service.stop.meta, "stop");
        if let Some(error) = self.escape_error() {
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(self.settle_failed_service(registry, timeout_ms, error));
        }
        if let Some(cancellation) = cancellation
            && cancellation.is_canceled()
        {
            self.cancel(registry, timeout_ms, "run canceled during shutdown")?;
            return Err(canceled_error());
        }
        record_lifecycle_started(registry, &context, &stop_record)?;
        if let Some(status) = self.child_mut()?.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect foreground service child: {error}"),
            )
        })? {
            let message = format!("foreground service exited before stop: {status}");
            let error = RuntimeError::new(ErrorCode::ProcEscape, message);
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(self.settle_failed_service(registry, timeout_ms, error));
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
        let _ = wait_for_child_exit(self.child_mut()?, 1000)?;
        self.join_log_relays()?;
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
            if let Some(monitor) = &mut self.monitor {
                monitor.stop();
            }
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
        if let Some(error) = self.escape_error() {
            let _ = record_lifecycle_failure(registry, &context, &stop_record, &error);
            return Err(self.settle_failed_service(registry, timeout_ms, error));
        }
        if let Some(monitor) = &mut self.monitor {
            monitor.stop();
        }
        mark_service_stopped(
            registry,
            &self.run_id,
            &self.service_instance_id,
            &self.process_key,
            &self.computed_model_hash,
        )?;
        record_lifecycle_success(registry, &context, &stop_record)
    }

    fn escape_error(&self) -> Option<RuntimeError> {
        let Some(monitor) = &self.monitor else {
            return None;
        };
        let escaped = monitor.escaped_descendants();
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
        Some(RuntimeError::new(ErrorCode::ProcEscape, message))
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

    /// Failure cleanup is stronger than the declared steady-state containment:
    /// once a strict process-group service has demonstrated an escape, every
    /// descendant captured by the monitor must also be killed and identity-
    /// checked before ports can be released.
    fn terminate_after_failure(&self, timeout_ms: u64) -> RuntimeResult<()> {
        let monitored = self
            .monitor
            .as_ref()
            .map(ProcessMonitor::known_descendants)
            .unwrap_or_default();
        terminate_process_tree_with_snapshot(
            self.pid,
            self.pgid,
            libc::SIGTERM,
            timeout_ms,
            &monitored,
        )
        .map(|_| ())
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

    fn join_log_relays(&mut self) -> RuntimeResult<()> {
        match self.log_relays.take() {
            Some(relays) => relays.join(),
            None => Ok(()),
        }
    }

    fn lifecycle_event_context(&self) -> LifecycleEventContext {
        LifecycleEventContext {
            run_id: Some(self.run_id.clone()),
            service_instance_id: self.service_instance_id.clone(),
            process_key: Some(self.process_key.clone()),
            computed_model_hash: self.computed_model_hash.clone(),
        }
    }

    fn escape_error_unrecorded(&self) -> Option<RuntimeError> {
        let monitor = self.monitor.as_ref()?;
        let escaped = monitor.escaped_descendants();
        if escaped.is_empty() {
            return None;
        }
        let escaped_pids = escaped
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        Some(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!(
                "service process {} has escaped descendants outside pgid {}: {:?}",
                self.pid, self.pgid, escaped_pids
            ),
        ))
    }

    fn escape_start_identity(&mut self) -> String {
        if let Some(monitor) = &mut self.monitor {
            monitor.stop();
        }
        let known = self
            .monitor
            .as_ref()
            .map(ProcessMonitor::known_descendants)
            .unwrap_or_default();
        let tracked = tracked_process_snapshot(self.pid, &known);
        process_start_identity(
            self.pid,
            self.pgid,
            self.platform_start_identity.as_deref(),
            &tracked,
        )
    }
}

fn probe_policy(probe: &Probe) -> (u32, Duration, &str) {
    match probe {
        Probe::Tcp(probe) => (
            probe.max_attempts.max(1),
            probe.retry_interval,
            probe.label.as_str(),
        ),
        Probe::Exec(probe) => (
            probe.max_attempts.max(1),
            probe.retry_interval,
            probe.label.as_str(),
        ),
    }
}

fn planned_endpoints(
    endpoints: &[SelectedEndpoint],
) -> Result<Vec<PlannedEndpoint>, EndpointFailure> {
    endpoints
        .iter()
        .map(|endpoint| {
            PlannedEndpoint::parse(&endpoint.endpoint_id, &endpoint.host, endpoint.port)
        })
        .collect()
}

enum EndpointDecision {
    Complete(Vec<EndpointOwnership>),
    Pending(PlannedEndpoint),
    Conflict {
        endpoint: PlannedEndpoint,
        listeners: Vec<ListenerRecord>,
    },
    Unverifiable {
        endpoint: Option<PlannedEndpoint>,
        message: String,
    },
    ContainmentUnconfirmed(String),
}

fn classify_endpoint_observation(observation: OwnershipObservation) -> EndpointDecision {
    match observation {
        OwnershipObservation::Complete(ownership) => EndpointDecision::Complete(ownership),
        OwnershipObservation::Missing(endpoint) => EndpointDecision::Pending(endpoint),
        OwnershipObservation::Outside {
            endpoint,
            listeners,
        } => EndpointDecision::Conflict {
            endpoint,
            listeners,
        },
        OwnershipObservation::Unverifiable { endpoint, message } => {
            EndpointDecision::Unverifiable { endpoint, message }
        }
        OwnershipObservation::ContainmentUnconfirmed { message } => {
            EndpointDecision::ContainmentUnconfirmed(message)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortConflictEndpoint<'a> {
    transport: &'static str,
    family: &'static str,
    address: String,
    port: u16,
    endpoint_id: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NixfiedOwner {
    project_id: String,
    environment: String,
    slot: u32,
    run_id: String,
    service_id: String,
    service_instance_id: String,
    process_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortConflictDetails<'a> {
    reason: &'a str,
    project_id: &'a str,
    endpoint: PortConflictEndpoint<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nixfied_owner: Option<&'a NixfiedOwner>,
}

fn port_conflict_error(
    reason: &str,
    project_id: &str,
    endpoint: &PlannedEndpoint,
    owner: Option<&NixfiedOwner>,
) -> RuntimeError {
    let details = PortConflictDetails {
        reason,
        project_id,
        endpoint: PortConflictEndpoint {
            transport: "tcp",
            family: endpoint.family.as_str(),
            address: endpoint.canonical_address(),
            port: endpoint.port,
            endpoint_id: &endpoint.endpoint_id,
        },
        nixfied_owner: owner,
    };
    RuntimeError::new(
        ErrorCode::PortConflict,
        format!(
            "endpoint {} is unavailable at {}:{} ({reason})",
            endpoint.endpoint_id, endpoint.address, endpoint.port
        ),
    )
    .with_detail("portConflict", details)
}

fn port_unverifiable_error(
    endpoint: Option<&PlannedEndpoint>,
    message: impl Into<String>,
) -> RuntimeError {
    let mut error = RuntimeError::new(ErrorCode::PortUnverifiable, message);
    if let Some(endpoint) = endpoint {
        error = error
            .with_detail("endpointId", &endpoint.endpoint_id)
            .with_detail("address", endpoint.canonical_address())
            .with_detail("port", endpoint.port);
    }
    error
}

fn proven_nixfied_owner(
    registry: &Registry,
    endpoint: &PlannedEndpoint,
    listeners: &[ListenerRecord],
    requested_service: &ExecService,
) -> RuntimeResult<Option<NixfiedOwner>> {
    if listeners.is_empty() {
        return Ok(None);
    }
    let candidates = {
        let mut statement = registry
            .connection()
            .prepare(
                "
                SELECT DISTINCT service_instance_id
                FROM ports
                WHERE address = ?1 AND port = ?2 AND status = ?3
                ORDER BY service_instance_id
                ",
            )
            .map_err(sql_error)?;
        statement
            .query_map(
                params![
                    endpoint.canonical_address(),
                    endpoint.port,
                    PortStatus::Active.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    for service_instance_id in candidates {
        let snapshot = read_service_snapshot(registry, &service_instance_id)?;
        let (Some(service), Some(process)) = (&snapshot.service, &snapshot.process) else {
            continue;
        };
        if service.endpoint_identity_hash != requested_service.identity.endpoint_identity_hash
            || service.state_identity_hash != requested_service.identity.state_identity_hash
            || service.runtime_compatibility_hash
                != requested_service.identity.runtime_compatibility_hash
            || service.target_identity_hash != requested_service.identity.target_identity_hash
            || !status::PROCESS_ACTIVE.contains(&process.status)
            || !snapshot.endpoints.iter().any(|stored| {
                stored.address == endpoint.canonical_address()
                    && stored.port == endpoint.port
                    && stored.status == PortStatus::Active
                    && stored.owner_process_key.as_deref() == Some(&process.process_key)
            })
        {
            continue;
        }
        if !process_is_live_with_identity(
            process.pid,
            process.pgid,
            process.platform_start.as_deref(),
        )? {
            continue;
        }
        if !matches!(
            observe_ownership(
                std::slice::from_ref(endpoint),
                &ExpectedOwner {
                    pid: process.pid,
                    pgid: process.pgid,
                    platform_start: process.platform_start.as_deref(),
                    containment: requested_service.containment.clone(),
                    tracked_processes: &[],
                },
            ),
            OwnershipObservation::Complete(_)
        ) {
            continue;
        }
        let slot = u32::try_from(registry.identity().slot).map_err(|_| {
            RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!("registry slot {} is outside u32", registry.identity().slot),
            )
        })?;
        return Ok(Some(NixfiedOwner {
            project_id: registry.identity().project_id.clone(),
            environment: registry.identity().environment.clone(),
            slot,
            run_id: process.run_id.clone(),
            service_id: service.service_name.clone(),
            service_instance_id,
            process_key: process.process_key.clone(),
        }));
    }
    Ok(None)
}

fn endpoint_failure_error(
    registry: &Registry,
    service: &ExecService,
    failure: EndpointFailure,
) -> RuntimeResult<RuntimeError> {
    match failure {
        EndpointFailure::LockContended { endpoint } => Ok(port_conflict_error(
            "startup-lock-contended",
            &registry.identity().project_id,
            &endpoint,
            None,
        )),
        EndpointFailure::ListenerOccupied {
            endpoint,
            listeners,
        } => {
            let owner = proven_nixfied_owner(registry, &endpoint, &listeners, service)?;
            Ok(port_conflict_error(
                "listener-occupied",
                &registry.identity().project_id,
                &endpoint,
                owner.as_ref(),
            ))
        }
        EndpointFailure::Unverifiable { endpoint, message } => {
            Ok(port_unverifiable_error(endpoint.as_ref(), message))
        }
    }
}

impl Drop for StartedService {
    fn drop(&mut self) {
        let Some(child) = &mut self.child else {
            return;
        };
        if child.try_wait().ok().flatten().is_none() {
            let _ = signal_process_group(self.pgid, libc::SIGTERM);
            if wait_for_child_exit(child, 100).ok() != Some(true) {
                let _ = signal_process_group(self.pgid, libc::SIGKILL);
                let _ = wait_for_child_exit(child, 1000);
            }
        }
        if let Some(monitor) = &mut self.monitor {
            monitor.stop();
        }
        let _ = self.join_log_relays();
    }
}

/// One service's slice of the slot plan: its name, the planned port for each of
/// its endpoints (keyed by endpointId), and the cross-service slot endpoint map
/// `${port:<serviceId>}` resolves against.
pub struct ServiceSelection<'a> {
    pub service_name: &'a str,
    pub service_lifetime: ServiceLifetime,
    pub endpoint_ports: &'a BTreeMap<String, u16>,
    pub slot_endpoints: &'a SlotEndpoints,
    pub run_timeout_ms: u64,
    pub cancellation: &'a CancellationToken,
    /// Executes the service's prepare task (its flattened nodes) inside the
    /// service reservation. Supplied by the run driver, which owns the started
    /// services the prepare leaves may require; `None` when the service
    /// declares no prepare task. The runtime stays generic: this is plumbing,
    /// not vocabulary.
    pub prepare_runner: Option<PrepareRunner<'a>>,
}

/// The prepare-task executor a run driver supplies.
pub type PrepareRunner<'a> = Box<dyn FnMut(&mut Registry) -> RuntimeResult<()> + 'a>;

/// Start a declared foreground service from the lowered model: run prepare,
/// spawn-and-own the start exec, and track the process. The service is read from
/// the admission's `ExecutionModel`, never the raw `Model`.
pub fn start_service_for_slot(
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_slot: &SelectedSlot<'_>,
    mut selection: ServiceSelection<'_>,
) -> RuntimeResult<StartedService> {
    let run_id = run_id.into();
    let run_timeout_ms = selection.run_timeout_ms;
    let cancellation = selection.cancellation;
    let source = admission.require_source()?;
    let service_name = selection.service_name;
    let endpoint_ports = selection.endpoint_ports;
    let slot_endpoints = selection.slot_endpoints;
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
    // An endpoint-less service has no primary: it makes no addressability
    // claim, so there is no selected endpoint to record or probe over tcp.
    let selected_endpoint = match &service.primary_endpoint {
        Some(primary) => Some(own_endpoints.get(primary).cloned().ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("service {service_name} primary endpoint {primary} is missing"),
            )
        })?),
        None => None,
    };
    let selected_endpoints: Vec<SelectedEndpoint> = own_endpoints.values().cloned().collect();
    // The named substitution scope is the declared connectsTo set; lowering
    // proved every cross-service placeholder references a member of it.
    let named: SlotEndpoints = slot_endpoints
        .iter()
        .filter(|(id, _)| service.connects_to.contains(id))
        .map(|(id, endpoint)| (id.clone(), endpoint.clone()))
        .collect();
    let substitution = ExecSubstitution {
        own_primary: selected_endpoint.as_ref(),
        own_endpoints: &own_endpoints,
        named: &named,
        state_root: &placement.state_root,
        secrets: &admission.secrets,
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
    let planned_endpoints = match planned_endpoints(&selected_endpoints) {
        Ok(endpoints) => endpoints,
        Err(failure) => return Err(endpoint_failure_error(registry, service, failure)?),
    };
    reconcile_registry(registry)?;
    let owner_token = run_owner_token(&run_id);
    if let Some(started) = borrow_reusable_service(
        registry,
        BorrowServiceRequest {
            admission,
            placement,
            run_id: &run_id,
            owner_token: &owner_token,
            service,
            service_name,
            service_lifetime: selection.service_lifetime,
            address_hash: &address_hash,
            service_instance_id: &service_instance_id,
            selected_endpoint: selected_endpoint.clone(),
            selected_endpoints: selected_endpoints.clone(),
            ready_probe: ready_probe.clone(),
            health_probe: health_probe.clone(),
            reservations: &reservations,
            recover_starting: false,
        },
    )? {
        return Ok(started);
    }
    let initial_snapshot = read_service_snapshot(registry, &service_instance_id)?;
    refuse_unexpired_repair_leases(&initial_snapshot)?;
    cancellation.check()?;
    let startup_guards = match acquire_startup_locks(&planned_endpoints) {
        Ok(guards) => guards,
        Err(failure) => return Err(endpoint_failure_error(registry, service, failure)?),
    };
    cancellation.check()?;
    reconcile_registry(registry)?;
    if let Some(started) = borrow_reusable_service(
        registry,
        BorrowServiceRequest {
            admission,
            placement,
            run_id: &run_id,
            owner_token: &owner_token,
            service,
            service_name,
            service_lifetime: selection.service_lifetime,
            address_hash: &address_hash,
            service_instance_id: &service_instance_id,
            selected_endpoint: selected_endpoint.clone(),
            selected_endpoints: selected_endpoints.clone(),
            ready_probe: ready_probe.clone(),
            health_probe: health_probe.clone(),
            reservations: &reservations,
            recover_starting: true,
        },
    )? {
        startup_guards.release();
        return Ok(started);
    }
    let service_record = ServiceRecord {
        service_instance_id: &service_instance_id,
        service_name,
        service_address_hash: &address_hash,
        identity: &service.identity,
        service_lifetime: selection.service_lifetime,
        endpoint_json: "null",
        state_root: &placement.state_root,
    };
    match repair_requested_service(
        registry,
        &service_record,
        service,
        &selected_endpoints,
        &run_id,
        &admission.computed_model_hash,
        run_timeout_ms,
    )? {
        RepairOutcome::Cleared => {}
        RepairOutcome::Reusable => {
            if let Some(started) = borrow_reusable_service(
                registry,
                BorrowServiceRequest {
                    admission,
                    placement,
                    run_id: &run_id,
                    owner_token: &owner_token,
                    service,
                    service_name,
                    service_lifetime: selection.service_lifetime,
                    address_hash: &address_hash,
                    service_instance_id: &service_instance_id,
                    selected_endpoint: selected_endpoint.clone(),
                    selected_endpoints: selected_endpoints.clone(),
                    ready_probe: ready_probe.clone(),
                    health_probe: health_probe.clone(),
                    reservations: &reservations,
                    recover_starting: true,
                },
            )? {
                startup_guards.release();
                return Ok(started);
            }
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!("service {service_name} changed after exact Starting recovery proof"),
            ));
        }
    }
    inspect_unrelated_endpoint_evidence(admission, registry, &reservations, &service_instance_id)?;
    if let Err(failure) = preflight(&planned_endpoints) {
        return Err(endpoint_failure_error(registry, service, failure)?);
    }
    ensure_service_start_allowed(registry, &run_id, &service_instance_id)?;
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
    if let Err(error) = cancellation.check() {
        return Err(settle_reserved_failure(
            registry,
            &run_id,
            &service_instance_id,
            error,
        ));
    }
    let lifecycle_context = LifecycleEventContext {
        run_id: Some(run_id.clone()),
        service_instance_id: service_instance_id.clone(),
        process_key: None,
        computed_model_hash: admission.computed_model_hash.clone(),
    };
    // prepare is a task reference: the caller supplies a runner that executes
    // the referenced task's flattened nodes (ordinary task evidence — logs,
    // summaries, registry rows keyed by step path). It runs INSIDE the
    // service's reservation, so concurrent runtimes cannot double-prepare the
    // same state, with the just-created lease kept alive for the duration.
    if let Some(prepare_task) = &service.prepare {
        let prepare_record = LifecycleRecord::from_meta(
            &OpMeta {
                operation_id: nixfied_model::OperationId::new(prepare_task.as_str()),
                terminal_success: "initialized".to_string(),
                terminal_failure: "failed".to_string(),
            },
            "prepare",
        );
        record_lifecycle_started(registry, &lifecycle_context, &prepare_record).map_err(
            |error| settle_reserved_failure(registry, &run_id, &service_instance_id, error),
        )?;
        let prepare_heartbeat = RunLeaseHeartbeat::start(
            placement.registry_path().to_path_buf(),
            registry.identity().clone(),
            run_id.clone(),
            owner_token.clone(),
        );
        let prepare_result = match selection.prepare_runner {
            Some(ref mut runner) => runner(registry),
            None => Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!(
                    "service {service_name} declares prepare task {prepare_task} but the caller supplied no prepare runner"
                ),
            )),
        };
        let heartbeat_result = prepare_heartbeat.stop();
        if let Err(error) = prepare_result.and(heartbeat_result) {
            let _ = record_lifecycle_failure(registry, &lifecycle_context, &prepare_record, &error);
            return Err(settle_reserved_failure(
                registry,
                &run_id,
                &service_instance_id,
                error,
            ));
        }
        record_lifecycle_success(registry, &lifecycle_context, &prepare_record).map_err(
            |error| settle_reserved_failure(registry, &run_id, &service_instance_id, error),
        )?;
        if let Err(error) = cancellation.check() {
            return Err(settle_reserved_failure(
                registry,
                &run_id,
                &service_instance_id,
                error,
            ));
        }
    }
    let start_record = LifecycleRecord::from_meta(&service.start.meta, "start");
    record_lifecycle_started(registry, &lifecycle_context, &start_record)
        .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    let exec = &service.start.exec;
    let command_cwd = resolve_exec_cwd(&source.observed_root, &exec.cwd)
        .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    let args = substitution
        .args(&exec.args)
        .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    let declared_env = substitution
        .env(&exec.env)
        .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    let env = exec.env_with_path(declared_env);
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
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))
    .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    // `null` for an endpoint-less service: durable evidence records the absence
    // of an addressability claim, not a fabricated endpoint.
    let endpoint_json = serde_json::to_string(&selected_endpoint)
        .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))
        .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    let redactor = Redactor::from_secrets(&admission.secrets);
    let (stdout, stderr, mut log_relays) =
        if matches!(selection.service_lifetime, ServiceLifetime::RunScoped) {
            let output = child_output(&stdout_path, &stderr_path, &redactor).map_err(|error| {
                settle_reserved_failure(registry, &run_id, &service_instance_id, error)
            })?;
            (output.stdout, output.stderr, Some(output.relays))
        } else {
            (Stdio::null(), Stdio::null(), None)
        };
    let mut command = Command::new(&exec.executable);
    // Hermetic child environment: declared env + runtime-owned variables only
    // (PATH from the tool roots); nothing inherited from the runtime's own
    // environment.
    command
        .env_clear()
        .args(&args)
        .current_dir(&command_cwd)
        .envs(&env)
        .stdin(stdin_for(exec.stdin))
        .stdout(stdout)
        .stderr(stderr);
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    if let Err(error) = cancellation.check() {
        let _ = record_lifecycle_failure(registry, &lifecycle_context, &start_record, &error);
        return Err(settle_reserved_failure(
            registry,
            &run_id,
            &service_instance_id,
            error,
        ));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let error = RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to spawn service {service_name}: {error}"),
            );
            let _ = record_lifecycle_failure(registry, &lifecycle_context, &start_record, &error);
            return Err(settle_reserved_failure(
                registry,
                &run_id,
                &service_instance_id,
                error,
            ));
        }
    };
    let pid = child.id();
    let pgid = match get_process_group(pid) {
        Ok(pgid) => pgid,
        Err(error) => {
            let _ = record_lifecycle_failure(registry, &lifecycle_context, &start_record, &error);
            if let Err(termination_error) =
                terminate_unrecorded_child(&mut child, None, service, run_timeout_ms)
            {
                return Err(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!(
                        "failed to prove termination of unrecorded service process {pid}: {}",
                        termination_error.message
                    ),
                ));
            }
            if let Some(relays) = log_relays.take() {
                let _ = relays.join();
            }
            return Err(settle_reserved_failure(
                registry,
                &run_id,
                &service_instance_id,
                error,
            ));
        }
    };
    let platform_start = platform_start_identity(pid);
    let start_identity = process_start_identity(pid, pgid, platform_start.as_deref(), &[]);
    let process_key = format!("process-{run_id}-{pid}-{pgid}");
    let started_context = LifecycleEventContext {
        run_id: Some(run_id.clone()),
        service_instance_id: service_instance_id.clone(),
        process_key: Some(process_key.clone()),
        computed_model_hash: admission.computed_model_hash.clone(),
    };
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
            service_lifetime: selection.service_lifetime,
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
        let _ = record_lifecycle_failure(registry, &started_context, &start_record, &error);
        if let Err(termination_error) =
            terminate_unrecorded_child(&mut child, Some(pgid), service, run_timeout_ms)
        {
            return Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!(
                    "failed to prove termination of unrecorded service process group {pgid}: {}",
                    termination_error.message
                ),
            ));
        }
        if let Some(relays) = log_relays.take() {
            let _ = relays.join();
        }
        return Err(settle_reserved_failure(
            registry,
            &run_id,
            &service_instance_id,
            error,
        ));
    }
    let strict_process_group = matches!(service.containment, ContainmentRequirement::ProcessGroup);
    let monitor = spawn_process_monitor(pid, pgid, strict_process_group);
    let mut started = StartedService {
        child: Some(child),
        borrowed: false,
        monitor: Some(monitor),
        startup_guards: Some(startup_guards),
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
        planned_endpoints,
        computed_model_hash: admission.computed_model_hash.clone(),
        source_root: source.observed_root.clone(),
        state_root: placement.state_root.clone(),
        secrets: admission.secrets.clone(),
        redactor,
        owner_token,
        service_lifetime: selection.service_lifetime,
        log_relays,
    };
    if let Err(error) = ensure_foreground_child_alive(&mut started) {
        let error =
            match started.override_after_primary_exit_with_endpoint_evidence(registry, error) {
                Ok(()) => RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    "endpoint failure classification unexpectedly returned success",
                ),
                Err(error) => error,
            };
        let _ = record_lifecycle_failure(registry, &started_context, &start_record, &error);
        return Err(started.finalize_failed_start(registry, run_timeout_ms, error));
    }
    if let Err(error) = record_lifecycle_success(registry, &started_context, &start_record) {
        return Err(started.finalize_failed_start(registry, run_timeout_ms, error));
    }
    if let Err(error) = cancellation.check() {
        let finalized = started.finalize_failed_start(registry, run_timeout_ms, error);
        return Err(finalized);
    }
    Ok(started)
}

struct BorrowServiceRequest<'a> {
    admission: &'a Admission,
    placement: &'a HostPlacement,
    run_id: &'a str,
    owner_token: &'a str,
    service: &'a ExecService,
    service_name: &'a str,
    service_lifetime: ServiceLifetime,
    address_hash: &'a str,
    service_instance_id: &'a str,
    selected_endpoint: Option<SelectedEndpoint>,
    selected_endpoints: Vec<SelectedEndpoint>,
    ready_probe: Probe,
    health_probe: Probe,
    reservations: &'a [PortReservation<'a>],
    recover_starting: bool,
}

enum RepairOutcome {
    Cleared,
    Reusable,
}

fn repair_requested_service(
    registry: &mut Registry,
    requested_record: &ServiceRecord<'_>,
    requested_service: &ExecService,
    requested_endpoints: &[SelectedEndpoint],
    requesting_run_id: &str,
    computed_model_hash: &str,
    run_timeout_ms: u64,
) -> RuntimeResult<RepairOutcome> {
    for _ in 0..3 {
        let snapshot = read_service_snapshot(registry, requested_record.service_instance_id)?;
        if !snapshot.has_evidence() {
            return Ok(RepairOutcome::Cleared);
        }
        let Some(service) = snapshot.service.as_ref() else {
            if settle_registry_only_evidence(registry, requested_record.service_instance_id)? {
                return Ok(RepairOutcome::Cleared);
            }
            continue;
        };
        if service.service_name != requested_record.service_name
            || service.service_address_hash != requested_record.service_address_hash
            || service.endpoint_identity_hash != requested_record.identity.endpoint_identity_hash
            || service.state_identity_hash != requested_record.identity.state_identity_hash
            || service.runtime_compatibility_hash
                != requested_record.identity.runtime_compatibility_hash
            || service.target_identity_hash != requested_record.identity.target_identity_hash
            || service.state_root != requested_record.state_root.display().to_string()
        {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "service instance {} does not match its computed identity",
                    requested_record.service_instance_id
                ),
            ));
        }
        let Some(process) = snapshot.process.as_ref() else {
            if service.status.is_active()
                || snapshot
                    .endpoints
                    .iter()
                    .any(|endpoint| status::PORT_OPEN.contains(&endpoint.status))
                || snapshot
                    .leases
                    .iter()
                    .any(|lease| status::LEASE_OPEN.contains(&lease.status))
            {
                return Err(RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!(
                        "service instance {} has active evidence without an actionable process",
                        requested_record.service_instance_id
                    ),
                ));
            }
            return Ok(RepairOutcome::Cleared);
        };
        if process.status == crate::registry::status::ProcessStatus::Escaped
            || service.status == ServiceStatus::Escaped
        {
            if process.status != crate::registry::status::ProcessStatus::Escaped
                || service.status != ServiceStatus::Escaped
            {
                return Err(RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    "escaped repair candidate has inconsistent process/service status",
                ));
            }
            refuse_unexpired_repair_leases(&snapshot)?;
            if !guarded_claim_service_repair(registry, requested_record, &snapshot)? {
                continue;
            }
            let process = process.clone();
            let start_identity = stored_escape_start_identity(&process);
            if let Err(termination_error) =
                terminate_stored_service(requested_service, &process, run_timeout_ms)
            {
                let payload = serde_json::json!({
                    "pid": process.pid,
                    "pgid": process.pgid,
                    "reason": "escaped-service-repair",
                    "terminationError": termination_error.message,
                })
                .to_string();
                let _ = mark_process_escape(
                    registry,
                    &process.process_key,
                    requesting_run_id,
                    requested_record.service_instance_id,
                    computed_model_hash,
                    &start_identity,
                    &payload,
                );
                return Err(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!(
                        "failed to prove termination of escaped service process group {}: {}",
                        process.pgid, termination_error.message
                    ),
                ));
            }
            release_unresolved_escape_ports(
                registry,
                &process.process_key,
                &process.run_id,
                requested_record.service_instance_id,
                computed_model_hash,
            )?;
            return Ok(RepairOutcome::Cleared);
        }
        if !status::PROCESS_ACTIVE.contains(&process.status) {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "repair candidate process {} is not actionable",
                    process.process_key
                ),
            ));
        }
        if !process_is_live_with_identity(
            process.pid,
            process.pgid,
            process.platform_start.as_deref(),
        )? {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                "mandatory reconciliation left a dead repair candidate active",
            ));
        }
        let stored_endpoints =
            repair_candidate_endpoints(&snapshot, requested_record.service_instance_id)?;
        let stored_planned = match planned_endpoints(&stored_endpoints) {
            Ok(endpoints) => endpoints,
            Err(failure) => {
                return Err(endpoint_failure_error(
                    registry,
                    requested_service,
                    failure,
                )?);
            }
        };
        let observation = classify_endpoint_observation(observe_ownership(
            &stored_planned,
            &ExpectedOwner {
                pid: process.pid,
                pgid: process.pgid,
                platform_start: process.platform_start.as_deref(),
                containment: requested_service.containment.clone(),
                tracked_processes: &[],
            },
        ));
        match observation {
            EndpointDecision::Complete(_)
                if endpoint_map(&stored_endpoints) == endpoint_map(requested_endpoints)
                    && (service.status != ServiceStatus::Starting
                        || (!snapshot.endpoints.is_empty()
                            && snapshot
                                .endpoints
                                .iter()
                                .all(|endpoint| endpoint.status == PortStatus::Active)))
                    && matches!(
                        service.status,
                        ServiceStatus::ProbeReady
                            | ServiceStatus::Standing
                            | ServiceStatus::Borrowed
                            | ServiceStatus::Starting
                    ) =>
            {
                return Ok(RepairOutcome::Reusable);
            }
            EndpointDecision::Unverifiable { endpoint, message } => {
                return Err(port_unverifiable_error(endpoint.as_ref(), message));
            }
            EndpointDecision::ContainmentUnconfirmed(message) => {
                return Err(RuntimeError::new(ErrorCode::ProcEscape, message));
            }
            EndpointDecision::Complete(_)
            | EndpointDecision::Pending(_)
            | EndpointDecision::Conflict { .. } => {}
        }
        refuse_unexpired_repair_leases(&snapshot)?;
        if !guarded_claim_service_repair(registry, requested_record, &snapshot)? {
            continue;
        }
        let process = process.clone();
        let start_identity = stored_escape_start_identity(&process);
        if let Err(termination_error) =
            terminate_stored_service(requested_service, &process, run_timeout_ms)
        {
            let payload = serde_json::json!({
                "pid": process.pid,
                "pgid": process.pgid,
                "reason": "service-repair",
                "terminationError": termination_error.message,
            })
            .to_string();
            let _ = mark_process_escape(
                registry,
                &process.process_key,
                requesting_run_id,
                requested_record.service_instance_id,
                computed_model_hash,
                &start_identity,
                &payload,
            );
            return Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!(
                    "failed to prove termination of service process group {}: {}",
                    process.pgid, termination_error.message
                ),
            ));
        }
        mark_repair_target_stale(registry, requested_record.service_instance_id, &process)?;
        return Ok(RepairOutcome::Cleared);
    }
    Err(RuntimeError::new(
        ErrorCode::LeaseConflict,
        format!(
            "service instance {} changed during repair claim",
            requested_record.service_instance_id
        ),
    ))
}

fn repair_candidate_endpoints(
    snapshot: &crate::service::registry::ServiceSnapshot,
    service_instance_id: &str,
) -> RuntimeResult<Vec<SelectedEndpoint>> {
    let prefix = format!("{service_instance_id}:");
    snapshot
        .endpoints
        .iter()
        .map(|endpoint| {
            let endpoint_id = endpoint.endpoint_key.strip_prefix(&prefix).ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::RegistryCorrupt,
                    format!(
                        "endpoint key {} does not belong to service {service_instance_id}",
                        endpoint.endpoint_key
                    ),
                )
            })?;
            Ok(SelectedEndpoint {
                endpoint_id: endpoint_id.to_string(),
                host: endpoint.address.clone(),
                port: endpoint.port,
            })
        })
        .collect()
}

fn inspect_unrelated_endpoint_evidence(
    admission: &Admission,
    registry: &mut Registry,
    requested: &[PortReservation<'_>],
    own_service_instance_id: &str,
) -> RuntimeResult<()> {
    for other_instance in open_port_service_instances(registry, requested, own_service_instance_id)?
    {
        let snapshot = read_service_snapshot(registry, &other_instance)?;
        let Some(process) = snapshot.process.as_ref() else {
            refuse_unexpired_repair_leases(&snapshot)?;
            if snapshot.service.is_none()
                && settle_registry_only_evidence(registry, &other_instance)?
            {
                continue;
            }
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "unrelated service instance {other_instance} owns an open port without an actionable process or lease"
                ),
            ));
        };
        if !process_is_live_with_identity(
            process.pid,
            process.pgid,
            process.platform_start.as_deref(),
        )? {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "mandatory reconciliation left unrelated process {} active",
                    process.process_key
                ),
            ));
        }
        let Some(stored_service) = snapshot.service.as_ref() else {
            return Err(RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "unrelated process {} has no service row",
                    process.process_key
                ),
            ));
        };
        let Some(service) = admission
            .execution_model
            .services
            .get(stored_service.service_name.as_str())
        else {
            return Err(RuntimeError::new(
                ErrorCode::PortUnverifiable,
                format!(
                    "live unrelated service {} is not present in the admitted model",
                    stored_service.service_name
                ),
            ));
        };
        if stored_service.endpoint_identity_hash != service.identity.endpoint_identity_hash
            || stored_service.state_identity_hash != service.identity.state_identity_hash
            || stored_service.runtime_compatibility_hash
                != service.identity.runtime_compatibility_hash
            || stored_service.target_identity_hash != service.identity.target_identity_hash
        {
            return Err(RuntimeError::new(
                ErrorCode::PortUnverifiable,
                format!(
                    "live unrelated service {} does not match the admitted identity",
                    stored_service.service_name
                ),
            ));
        }
        for requested_endpoint in requested {
            let Some(stored_endpoint) = snapshot.endpoints.iter().find(|endpoint| {
                endpoint.address == requested_endpoint.address
                    && endpoint.port == requested_endpoint.port
            }) else {
                continue;
            };
            let endpoint_id = stored_endpoint
                .endpoint_key
                .strip_prefix(&format!("{other_instance}:"))
                .ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        format!(
                            "endpoint key {} does not belong to service {other_instance}",
                            stored_endpoint.endpoint_key
                        ),
                    )
                })?;
            let planned = match PlannedEndpoint::parse(
                endpoint_id,
                &stored_endpoint.address,
                stored_endpoint.port,
            ) {
                Ok(endpoint) => endpoint,
                Err(failure) => {
                    return Err(endpoint_failure_error(registry, service, failure)?);
                }
            };
            match classify_endpoint_observation(observe_ownership(
                std::slice::from_ref(&planned),
                &ExpectedOwner {
                    pid: process.pid,
                    pgid: process.pgid,
                    platform_start: process.platform_start.as_deref(),
                    containment: service.containment.clone(),
                    tracked_processes: &[],
                },
            )) {
                EndpointDecision::Complete(ownership) => {
                    let listeners = ownership
                        .into_iter()
                        .next()
                        .map(|ownership| ownership.listeners)
                        .unwrap_or_default();
                    let owner = proven_nixfied_owner(registry, &planned, &listeners, service)?;
                    return Err(port_conflict_error(
                        "listener-occupied",
                        &admission.project_id,
                        &planned,
                        owner.as_ref(),
                    ));
                }
                EndpointDecision::Conflict {
                    endpoint,
                    listeners,
                } => {
                    let owner = proven_nixfied_owner(registry, &endpoint, &listeners, service)?;
                    return Err(port_conflict_error(
                        "listener-occupied",
                        &admission.project_id,
                        &endpoint,
                        owner.as_ref(),
                    ));
                }
                EndpointDecision::Pending(endpoint) => {
                    return Err(port_unverifiable_error(
                        Some(&endpoint),
                        format!(
                            "live unrelated process {} has no exact listener for its open port",
                            process.process_key
                        ),
                    ));
                }
                EndpointDecision::Unverifiable { endpoint, message } => {
                    return Err(port_unverifiable_error(endpoint.as_ref(), message));
                }
                EndpointDecision::ContainmentUnconfirmed(message) => {
                    return Err(RuntimeError::new(ErrorCode::PortUnverifiable, message));
                }
            }
        }
    }
    Ok(())
}

fn terminate_stored_service(
    service: &ExecService,
    process: &StoredServiceProcess,
    run_timeout_ms: u64,
) -> RuntimeResult<()> {
    let timeout_ms =
        (service.stop.timeout.as_millis().min(u128::from(u64::MAX)) as u64).min(run_timeout_ms);
    match service.containment {
        ContainmentRequirement::ProcessGroup => {
            terminate_process_group_signal(process.pgid, service.stop.signal.libc(), timeout_ms)?;
        }
        ContainmentRequirement::ProcessTree => {
            terminate_process_tree_signal(
                process.pid,
                process.pgid,
                service.stop.signal.libc(),
                timeout_ms,
            )?;
        }
    }
    Ok(())
}

fn stored_escape_start_identity(process: &StoredServiceProcess) -> String {
    let tracked = tracked_process_snapshot(process.pid, &process.tracked_processes);
    process_start_identity(
        process.pid,
        process.pgid,
        process.platform_start.as_deref(),
        &tracked,
    )
}

fn borrow_reusable_service(
    registry: &mut Registry,
    request: BorrowServiceRequest<'_>,
) -> RuntimeResult<Option<StartedService>> {
    let snapshot = read_service_snapshot(registry, request.service_instance_id)?;
    let Some(service_row) = &snapshot.service else {
        return Ok(None);
    };
    if !(matches!(
        service_row.status,
        ServiceStatus::ProbeReady | ServiceStatus::Standing | ServiceStatus::Borrowed
    ) || request.recover_starting && service_row.status == ServiceStatus::Starting)
    {
        return Ok(None);
    }
    if service_row.service_name != request.service_name
        || service_row.service_address_hash != request.address_hash
        || service_row.endpoint_identity_hash != request.service.identity.endpoint_identity_hash
        || service_row.state_identity_hash != request.service.identity.state_identity_hash
        || service_row.runtime_compatibility_hash
            != request.service.identity.runtime_compatibility_hash
        || service_row.target_identity_hash != request.service.identity.target_identity_hash
        || service_row.state_root != request.placement.state_root.display().to_string()
    {
        return Ok(None);
    }
    let Some(process_row) = snapshot.process.as_ref() else {
        return Ok(None);
    };
    if !status::PROCESS_ACTIVE.contains(&process_row.status) {
        return Ok(None);
    }
    if !process_is_live_with_identity(
        process_row.pid,
        process_row.pgid,
        process_row.platform_start.as_deref(),
    )? {
        return Ok(None);
    }
    let registry_endpoints = selected_endpoints_from_snapshot(
        &snapshot,
        request.service_instance_id,
        &process_row.process_key,
    )?;
    if endpoint_map(&registry_endpoints) != endpoint_map(&request.selected_endpoints) {
        return Ok(None);
    }
    let planned = match planned_endpoints(&request.selected_endpoints) {
        Ok(endpoints) => endpoints,
        Err(failure) => {
            return Err(endpoint_failure_error(registry, request.service, failure)?);
        }
    };
    match classify_endpoint_observation(observe_ownership(
        &planned,
        &ExpectedOwner {
            pid: process_row.pid,
            pgid: process_row.pgid,
            platform_start: process_row.platform_start.as_deref(),
            containment: request.service.containment.clone(),
            tracked_processes: &[],
        },
    )) {
        EndpointDecision::Complete(_) => {}
        EndpointDecision::Pending(_) => return Ok(None),
        EndpointDecision::Conflict {
            endpoint: _,
            listeners: _,
        } => return Ok(None),
        EndpointDecision::Unverifiable { endpoint, message } => {
            return Err(port_unverifiable_error(endpoint.as_ref(), message));
        }
        EndpointDecision::ContainmentUnconfirmed(message) => {
            return Err(RuntimeError::new(ErrorCode::ProcEscape, message));
        }
    }
    let source = request.admission.require_source()?;
    let service_record = ServiceRecord {
        service_instance_id: request.service_instance_id,
        service_name: request.service_name,
        service_address_hash: request.address_hash,
        identity: &request.service.identity,
        service_lifetime: service_row.service_lifetime,
        endpoint_json: "null",
        state_root: &request.placement.state_root,
    };
    let borrowed = record_service_borrow(
        registry,
        &RunRecord {
            run_id: request.run_id,
            owner_token: request.owner_token,
            admission: request.admission,
            placement: request.placement,
        },
        &ServiceReuseGuard {
            service: &service_record,
            process: process_row,
            endpoints: request.reservations,
        },
        request.recover_starting,
    )?;
    if !borrowed {
        return Ok(None);
    }
    let process_row = process_row.clone();
    Ok(Some(StartedService {
        child: None,
        borrowed: true,
        monitor: None,
        startup_guards: None,
        service: request.service.clone(),
        ready_probe: request.ready_probe,
        health_probe: request.health_probe,
        logs_dir: request.placement.logs_dir.clone(),
        run_id: request.run_id.to_string(),
        service_instance_id: request.service_instance_id.to_string(),
        process_key: process_row.process_key,
        pid: process_row.pid,
        pgid: process_row.pgid,
        platform_start_identity: process_row.platform_start,
        selected_endpoint: request.selected_endpoint,
        selected_endpoints: registry_endpoints,
        planned_endpoints: planned,
        computed_model_hash: request.admission.computed_model_hash.clone(),
        source_root: source.observed_root.clone(),
        state_root: request.placement.state_root.clone(),
        secrets: request.admission.secrets.clone(),
        redactor: Redactor::from_secrets(&request.admission.secrets),
        owner_token: request.owner_token.to_string(),
        service_lifetime: request.service_lifetime,
        log_relays: None,
    }))
}

fn endpoint_map(endpoints: &[SelectedEndpoint]) -> BTreeMap<String, SelectedEndpoint> {
    endpoints
        .iter()
        .map(|endpoint| (endpoint.endpoint_id.clone(), endpoint.clone()))
        .collect()
}

fn selected_endpoints_from_snapshot(
    snapshot: &crate::service::registry::ServiceSnapshot,
    service_instance_id: &str,
    process_key: &str,
) -> RuntimeResult<Vec<SelectedEndpoint>> {
    let mut endpoints = Vec::new();
    for endpoint in &snapshot.endpoints {
        if endpoint.status != PortStatus::Active
            || endpoint.owner_process_key.as_deref() != Some(process_key)
        {
            return Ok(Vec::new());
        }
        let prefix = format!("{service_instance_id}:");
        let endpoint_id = endpoint.endpoint_key.strip_prefix(&prefix).ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!(
                    "endpoint key {} does not belong to service {service_instance_id}",
                    endpoint.endpoint_key
                ),
            )
        })?;
        endpoints.push(SelectedEndpoint {
            endpoint_id: endpoint_id.to_string(),
            host: endpoint.address.clone(),
            port: endpoint.port,
        });
    }
    Ok(endpoints)
}

/// Clean every declared service of the slot, then clean the marker-owned slot
/// state once. Membership does not exist; every declared service may have left
/// slot evidence, so each one's clean lifecycle operation is recorded. Each is
/// a marker-gated runtime cleanup primitive (no exec).
pub fn run_slot_clean(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    for service_name in model.services.keys() {
        record_service_clean(model, admission, registry, selected_slot, service_name)?;
    }
    clean_marked_slot_state(model, admission, placement, registry, selected_slot, mode)
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
    let identity = compute_service_identity(service, &model.state, &model.target);
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
    mode: CleanupMode,
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
        mode,
    )
}

fn run_owner_token(run_id: &str) -> String {
    format!("{run_id}:runtime-pid-{}", std::process::id())
}

fn reservation_outcome(error: &RuntimeError) -> ReservationOutcome {
    if error.code == ErrorCode::Canceled {
        ReservationOutcome::Canceled
    } else {
        ReservationOutcome::Failed
    }
}

fn settle_reserved_failure(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    error: RuntimeError,
) -> RuntimeError {
    match settle_service_reservation(
        registry,
        run_id,
        service_instance_id,
        reservation_outcome(&error),
    ) {
        Ok(()) => error,
        Err(settlement_error) => settlement_error,
    }
}

fn terminate_unrecorded_child(
    child: &mut Child,
    observed_pgid: Option<i32>,
    service: &ExecService,
    run_timeout_ms: u64,
) -> RuntimeResult<()> {
    let pid = child.id();
    let pgid = observed_pgid.unwrap_or(i32::try_from(pid).map_err(|_| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("spawned pid {pid} is outside the process-group id range"),
        )
    })?);
    let timeout_ms =
        (service.stop.timeout.as_millis().min(u128::from(u64::MAX)) as u64).min(run_timeout_ms);
    match service.containment {
        ContainmentRequirement::ProcessGroup => terminate_process_group(pgid, timeout_ms)?,
        ContainmentRequirement::ProcessTree => {
            terminate_process_tree(pid, pgid, timeout_ms)?;
        }
    }
    if !wait_for_child_exit(child, 1000)? {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("spawned service child {pid} did not become waitable after termination"),
        ));
    }
    Ok(())
}

fn sql_error(error: rusqlite::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("endpoint attribution registry operation failed: {error}"),
    )
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
    pub secrets: &'a ResolvedSecrets,
}

impl ExecSubstitution<'_> {
    fn endpoint_value(&self, value: &str) -> RuntimeResult<String> {
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

    pub(crate) fn value(&self, value: &str) -> RuntimeResult<String> {
        let out = self.endpoint_value(value)?;
        if out.contains("${secret:") {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                "secret placeholders are only allowed in invocation.env values",
            ));
        }
        Ok(out)
    }

    fn env_value(&self, value: &str) -> RuntimeResult<String> {
        if has_unclosed_secret_ref(value) {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("malformed secret placeholder in invocation env value: {value}"),
            ));
        }
        let mut out = self.endpoint_value(value)?;
        for reference in secret_refs(value) {
            let secret = self.secrets.get(reference).ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    format!("secret placeholder references unresolved secret {reference}"),
                )
            })?;
            out = out.replace(&format!("${{secret:{reference}}}"), secret);
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
            .map(|(key, value)| Ok((key.clone(), self.env_value(value)?)))
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
    pub redactor: &'a Redactor,
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
    let output = child_output(spec.stdout_path, spec.stderr_path, spec.redactor)?;
    let mut command = Command::new(spec.executable);
    // Hermetic child environment, same as the long-lived spawn paths.
    command
        .env_clear()
        .args(spec.args)
        .current_dir(spec.cwd)
        .envs(spec.env)
        .stdin(stdin_for(spec.stdin))
        .stdout(output.stdout)
        .stderr(output.stderr);
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
            output.relays.join()?;
            return Err(canceled_error());
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect lifecycle operation {label}: {error}"),
            )
        })? {
            output.relays.join()?;
            return Ok(BoundedExecOutcome::Exited(status));
        }
        if Instant::now() >= deadline {
            let _ = terminate_process_group(pgid, 1000);
            let _ = wait_for_child_exit(&mut child, 1000);
            output.relays.join()?;
            return Ok(BoundedExecOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
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

pub(crate) fn process_is_live_with_start_identity(
    pid: u32,
    platform_start: Option<&str>,
) -> RuntimeResult<bool> {
    if process_group(pid)?.is_none() {
        return Ok(false);
    }
    let Some(expected) = platform_start else {
        return Ok(false);
    };
    if platform_start_identity(pid).as_deref() != Some(expected) {
        return Ok(false);
    }
    Ok(!process_is_zombie(pid))
}

pub(crate) fn process_is_in_containment(
    root_pid: u32,
    root_pgid: i32,
    containment: &ContainmentRequirement,
    candidate_pid: u32,
    candidate_pgid: i32,
) -> RuntimeResult<bool> {
    match containment {
        ContainmentRequirement::ProcessGroup => Ok(candidate_pgid == root_pgid),
        ContainmentRequirement::ProcessTree => {
            if candidate_pid == root_pid {
                return Ok(true);
            }
            Ok(descendant_pids(root_pid)?.contains(&candidate_pid))
        }
    }
}

pub(crate) fn process_group_has_live_member(pgid: i32) -> RuntimeResult<bool> {
    process_group_has_live_member_impl(pgid)
}

fn ensure_foreground_child_alive(service: &mut StartedService) -> RuntimeResult<()> {
    thread::sleep(FOREGROUND_GRACE);
    match service.child_mut()?.try_wait().map_err(|error| {
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
    terminate_process_tree_with_snapshot(pid, pgid, signal, timeout_ms, &[])
}

pub(crate) fn terminate_process_tree_with_snapshot(
    pid: u32,
    pgid: i32,
    signal: i32,
    timeout_ms: u64,
    monitored: &[TrackedProcessIdentity],
) -> RuntimeResult<bool> {
    // Snapshot owned descendants with their start identities BEFORE signaling. A
    // process-tree child may reparent to init and move to its own group after the
    // supervisor exits, making it invisible to a descendant/pgid scan; the
    // snapshot keeps it tracked, and the identity makes the tracking pid-reuse
    // safe (a recycled pid has a different start identity). The monitor's
    // earlier snapshot also covers a strict-group escape that has already been
    // reparented and is no longer discoverable below the foreground child.
    let mut snapshot_by_pid = descendant_pids(pid)
        .unwrap_or_default()
        .into_iter()
        .map(|child| (child, platform_start_identity(child)))
        .collect::<BTreeMap<_, _>>();
    for process in monitored {
        snapshot_by_pid
            .entry(process.pid)
            .or_insert_with(|| process.platform_start.clone());
    }
    let snapshot = snapshot_by_pid.into_iter().collect::<Vec<_>>();
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

#[derive(Default)]
struct ProcessMonitorState {
    known_descendants: BTreeMap<u32, TrackedProcessIdentity>,
    escaped_descendants: BTreeMap<u32, TrackedProcessIdentity>,
}

struct ProcessMonitor {
    state: Arc<Mutex<ProcessMonitorState>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProcessMonitor {
    fn escaped_descendants(&self) -> Vec<TrackedProcessIdentity> {
        self.state
            .lock()
            .map(|state| state.escaped_descendants.values().cloned().collect())
            .unwrap_or_default()
    }

    fn known_descendants(&self) -> Vec<TrackedProcessIdentity> {
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

fn monitored_process(pid: u32) -> TrackedProcessIdentity {
    TrackedProcessIdentity {
        pid,
        platform_start: platform_start_identity(pid),
    }
}

fn tracked_process_snapshot(
    root_pid: u32,
    existing: &[TrackedProcessIdentity],
) -> Vec<TrackedProcessIdentity> {
    let mut tracked = existing
        .iter()
        .cloned()
        .map(|process| (process.pid, process))
        .collect::<BTreeMap<_, _>>();
    for pid in descendant_pids(root_pid).unwrap_or_default() {
        let observed = monitored_process(pid);
        tracked
            .entry(pid)
            .and_modify(|stored| {
                if stored.platform_start.is_none() {
                    stored.platform_start.clone_from(&observed.platform_start);
                }
            })
            .or_insert(observed);
    }
    tracked.into_values().collect()
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
    let pids = macos_process_ids().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to list pids while inspecting descendants for pid {parent}: {error}"),
        )
    })?;
    let mut children = Vec::new();
    for pid in pids {
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

fn process_start_identity(
    pid: u32,
    pgid: i32,
    platform_start: Option<&str>,
    tracked_processes: &[TrackedProcessIdentity],
) -> String {
    let observed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    serde_json::json!({
        "pid": pid,
        "pgid": pgid,
        "platformStart": platform_start,
        "trackedProcesses": tracked_processes,
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
    let pids = macos_process_ids().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to list pids while checking process group {pgid}: {error}"),
        )
    })?;
    for pid in pids {
        if process_group(pid)? == Some(pgid) && !process_is_zombie(pid) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_process_ids() -> std::io::Result<Vec<u32>> {
    let required = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if required < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut capacity = usize::try_from(required)
        .map_err(|_| std::io::Error::other("macOS process-list size was negative"))?
        .checked_add(32)
        .ok_or_else(|| std::io::Error::other("macOS process-list capacity overflow"))?
        .max(32);

    loop {
        let byte_capacity = capacity
            .checked_mul(std::mem::size_of::<libc::pid_t>())
            .ok_or_else(|| std::io::Error::other("macOS process-list byte size overflow"))?;
        let byte_capacity = libc::c_int::try_from(byte_capacity).map_err(|_| {
            std::io::Error::other("macOS process-list byte size exceeds proc_listallpids limits")
        })?;
        let mut pids = Vec::new();
        pids.try_reserve_exact(capacity).map_err(|error| {
            std::io::Error::other(format!("failed to allocate macOS process list: {error}"))
        })?;
        pids.resize(capacity, 0 as libc::pid_t);
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), byte_capacity) };
        if count < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let count = usize::try_from(count)
            .map_err(|_| std::io::Error::other("macOS process count was negative"))?;
        if count >= pids.len() {
            capacity = capacity.checked_mul(2).ok_or_else(|| {
                std::io::Error::other("macOS process-list capacity growth overflow")
            })?;
            continue;
        }
        pids.truncate(count);
        let mut result = pids
            .into_iter()
            .filter_map(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
            .collect::<Vec<_>>();
        result.sort_unstable();
        result.dedup();
        return Ok(result);
    }
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
    use crate::admission::secrets::ResolvedSecrets;
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
            secrets: &ResolvedSecrets::empty(),
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
            secrets: &ResolvedSecrets::empty(),
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
            secrets: &ResolvedSecrets::empty(),
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
    fn secret_placeholders_are_env_only() {
        let secrets = ResolvedSecrets::from_values(BTreeMap::from([(
            "api-token".to_string(),
            "secret-value".to_string(),
        )]));
        let substitution = ExecSubstitution {
            own_primary: None,
            own_endpoints: &BTreeMap::new(),
            named: &SlotEndpoints::new(),
            state_root: Path::new("/state"),
            secrets: &secrets,
        };
        let env: BTreeMap<String, String> = [(
            "TOKEN".to_string(),
            "bearer:${secret:api-token}".to_string(),
        )]
        .into_iter()
        .collect();

        let env = substitution.env(&env).expect("secret env substitutes");
        assert_eq!(env["TOKEN"], "bearer:secret-value");
        assert_eq!(
            substitution
                .value("--token=${secret:api-token}")
                .unwrap_err()
                .code,
            ErrorCode::ModelAdmission
        );
    }

    #[test]
    fn unresolved_named_placeholder_fails_closed() {
        let substitution = ExecSubstitution {
            own_primary: None,
            own_endpoints: &BTreeMap::new(),
            named: &SlotEndpoints::new(),
            state_root: Path::new("/state"),
            secrets: &ResolvedSecrets::empty(),
        };
        let error = substitution
            .value("--db ${port:ghost}")
            .expect_err("an undeclared named placeholder must not leak to the child");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }

    #[test]
    fn endpoint_evidence_classifier_preserves_closed_error_precedence() {
        let planned = PlannedEndpoint::parse("api", "127.0.0.1", 23080).unwrap();
        assert!(matches!(
            classify_endpoint_observation(OwnershipObservation::Complete(Vec::new())),
            EndpointDecision::Complete(_)
        ));
        assert!(matches!(
            classify_endpoint_observation(OwnershipObservation::Missing(planned.clone())),
            EndpointDecision::Pending(endpoint) if endpoint == planned
        ));
        assert!(matches!(
            classify_endpoint_observation(OwnershipObservation::Outside {
                endpoint: planned.clone(),
                listeners: Vec::new(),
            }),
            EndpointDecision::Conflict { endpoint, .. } if endpoint == planned
        ));
        assert!(matches!(
            classify_endpoint_observation(OwnershipObservation::Unverifiable {
                endpoint: Some(planned.clone()),
                message: "incomplete fd proof".to_string(),
            }),
            EndpointDecision::Unverifiable { endpoint: Some(endpoint), message }
                if endpoint == planned && message == "incomplete fd proof"
        ));
        assert!(matches!(
            classify_endpoint_observation(OwnershipObservation::ContainmentUnconfirmed {
                message: "escaped".to_string(),
            }),
            EndpointDecision::ContainmentUnconfirmed(message) if message == "escaped"
        ));
    }
}
