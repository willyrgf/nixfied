use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nixfied_manifest::{ContainmentRequirement, LoopbackHost, StopSignal};
use rusqlite::params;
use serde::Serialize;

use crate::admission::secrets::ResolvedSecrets;
use crate::admission::{ControlAdmission, RunAdmission};
use crate::cancellation::{CancellationToken, canceled_error};
use crate::child::OwnedChild;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{
    ExecService, OpMeta, Probe, ProbePolicy, RelativeCwd, ResolvedInvocation, StdinPolicy,
};
use crate::launch::Refusal;
use crate::redaction::{
    CAPTURE_SHUTDOWN_TIMEOUT, CaptureOutcome, LogFileMode, RedactedLogRelays, Redactor,
    child_output,
};
use crate::registry::Registry;
use crate::registry::status::{self, DbStatus, PortStatus};
use crate::service::endpoint::{
    EndpointFailure, EndpointLockGuards, EndpointOwnership, ExpectedOwner, ListenerRecord,
    LockRoot, OwnershipObservation, acquire_startup_locks, observe_ownership,
    observe_ownership_after_primary_exit, observe_single_ownership, preflight,
};
use crate::service::identity::service_instance_id;
use crate::service::readiness::{ExecProbe, ProbeAttempt, exec_probe_attempt, tcp_probe_attempt};
use crate::service::registry::{
    EndpointRecord, InvocationIdentity, InvocationOwner, InvocationProcessRecord, Ownership,
    ProcessRecord, ServiceRecord, ServiceSettlement, ServiceStartOutcome, StopPolicy,
    TaskTerminalStatus, VerifiedEndpointActivation, activate_service_ready,
    mark_invocation_finished, mark_process_escape, mark_service_canceled, mark_service_failed,
    mark_service_stopped, read_service_snapshot, record_capture_outcome,
    record_invocation_canceling, record_invocation_observed, record_invocation_started,
    record_service_canceling, record_service_lifecycle_event, record_service_start,
    record_service_start_intent, settle_service_start,
};
use crate::slot::SelectedSlot;
use crate::state::ownership::SlotGuard;
use crate::state::{CleanupMode, CleanupOutcome, HostPlacement, StateIdentity, clean_marked_state};
use crate::template::{EndpointSelector, Piece, Template};

use super::TrackedProcessIdentity;

const STARTUP_GRACE: Duration = Duration::from_millis(100);

fn stop_signal_number(signal: StopSignal) -> i32 {
    match signal {
        StopSignal::Term => libc::SIGTERM,
        StopSignal::Int => libc::SIGINT,
        StopSignal::Quit => libc::SIGQUIT,
        StopSignal::Hup => libc::SIGHUP,
    }
}

/// The child's stdin, per the exec's declared policy: a closed `/dev/null` or the
/// operator's inherited stdin.
pub(crate) fn stdin_for(policy: StdinPolicy) -> Stdio {
    match policy {
        StdinPolicy::Null => Stdio::null(),
        StdinPolicy::Inherit => Stdio::inherit(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedEndpoint {
    pub endpoint_id: String,
    pub host: LoopbackHost,
    pub port: u16,
}

const PORT_CONFLICT_KEY: &str = "portConflict";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PortConflictEndpoint<'a> {
    transport: &'a str,
    family: &'a str,
    #[serde(rename = "address")]
    host: &'a LoopbackHost,
    port: u16,
    endpoint_id: &'a str,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PortConflictDetails<'a> {
    reason: PortConflictReason,
    project_id: &'a str,
    endpoint: PortConflictEndpoint<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nixfied_owner: Option<&'a NixfiedOwner>,
}

#[derive(Debug, serde::Serialize)]
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

include!("../generated/process.rs");

/// Runtime evidence of an owned service. Callers receive an
/// immutable view; resource capabilities remain in the enclosing private payload.
#[derive(Debug)]
pub struct ServiceInfo {
    pub run_id: String,
    pub service_instance_id: String,
    pub process_key: String,
    pub pid: u32,
    pub pgid: i32,
    pub platform_start_identity: Option<String>,
    pub computed_manifest_hash: String,
    service_name: String,
    primary_endpoint: Option<String>,
    selected_endpoints: BTreeMap<String, SelectedEndpoint>,
}

impl ServiceInfo {
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
    pub fn selected_endpoint(&self) -> Option<&SelectedEndpoint> {
        self.primary_endpoint
            .as_ref()
            .and_then(|id| self.selected_endpoints.get(id))
    }
}

pub struct StartingService {
    owned: Box<OwnedService>,
    startup_guards: EndpointLockGuards,
}
pub struct ReadyService {
    owned: Box<OwnedService>,
}

/// A failed readiness transition retains the starting owner and its guards.
pub struct ReadinessFailure {
    service: StartingService,
    error: Box<RuntimeError>,
}
impl std::fmt::Debug for ReadinessFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadinessFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl ReadinessFailure {
    pub fn into_parts(self) -> (StartingService, RuntimeError) {
        (self.service, *self.error)
    }
}

impl StartingService {
    pub fn info(&self) -> &ServiceInfo {
        &self.owned.info
    }
    pub fn selected_endpoint(&self) -> Option<&SelectedEndpoint> {
        self.info().selected_endpoint()
    }
    /// Stop a started service before readiness with its declared stop policy.
    pub fn stop(self, registry: &mut Registry, timeout_ms: u64) -> RuntimeResult<()> {
        let Self {
            mut owned,
            startup_guards,
        } = self;
        let result = owned.teardown(registry, timeout_ms, Teardown::Stop(None));
        drop(owned);
        startup_guards.release();
        result
    }
    pub fn ready(
        mut self,
        registry: &mut Registry,
        cancellation: &CancellationToken,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> Result<ReadyService, ReadinessFailure> {
        if let Err(error) =
            self.owned
                .run_probe(registry, ProbePhase::Ready, cancellation, checkpoint)
        {
            return Err(ReadinessFailure {
                service: self,
                error: Box::new(error),
            });
        }
        // The ready commit completed while this handle still owned the guards.
        self.startup_guards.release();
        Ok(ReadyService { owned: self.owned })
    }
    pub fn finalize_failed_start(
        mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        error: RuntimeError,
    ) -> RuntimeError {
        let error = self
            .owned
            .settle_failed_service(registry, timeout_ms, error);
        // Drop's best-effort containment also runs while endpoint exclusion is held.
        drop(self.owned);
        self.startup_guards.release();
        error
    }
}

impl ReadyService {
    pub fn info(&self) -> &ServiceInfo {
        &self.owned.info
    }
    pub fn service_name(&self) -> &str {
        self.info().service_name()
    }
    pub fn selected_endpoint(&self) -> Option<&SelectedEndpoint> {
        self.info().selected_endpoint()
    }
    pub fn check_liveness(&self) -> RuntimeResult<()> {
        self.owned.check_liveness()
    }
    /// Non-blocking: the service leader exited on its own, or its state is
    /// unobservable. Its own stop then settles it as a failure.
    pub fn exited(&self) -> bool {
        !matches!(self.owned.child.observe(), Ok(None))
    }
    pub fn check_health(
        &mut self,
        registry: &mut Registry,
        cancellation: &CancellationToken,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        self.owned
            .run_probe(registry, ProbePhase::Health, cancellation, checkpoint)
    }
    pub fn finalize_failed_start(
        mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        error: RuntimeError,
    ) -> RuntimeError {
        self.owned
            .settle_failed_service(registry, timeout_ms, error)
    }
    pub fn cancel(
        mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        reason: &str,
    ) -> RuntimeResult<()> {
        self.owned
            .teardown(registry, timeout_ms, Teardown::Cancel(reason))
    }
    pub fn stop(mut self, registry: &mut Registry, timeout_ms: u64) -> RuntimeResult<()> {
        self.owned
            .teardown(registry, timeout_ms, Teardown::Stop(None))
    }
    /// Stop gracefully; a cancellation that arrives while it stops records
    /// the service as canceled.
    pub fn stop_observing(
        mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<()> {
        self.owned
            .teardown(registry, timeout_ms, Teardown::Stop(Some(cancellation)))
    }
}
/// A session checkpoint across started services: the first departure fails it.
pub fn check_services_live<'a>(
    services: impl IntoIterator<Item = &'a ReadyService>,
) -> RuntimeResult<()> {
    services
        .into_iter()
        .try_for_each(ReadyService::check_liveness)
}

impl std::fmt::Debug for ReadyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadyService")
            .field("info", self.info())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum ProbePhase {
    Ready,
    Health,
}

/// How an owned service's teardown is chosen, recorded, and settled.
enum Teardown<'a> {
    /// The declared stop signal after durable stop intent; a clean stop
    /// records `stopped`, or `canceled` when the given cancellation arrived
    /// while it stopped.
    Stop(Option<&'a CancellationToken>),
    /// Cancellation with its reason, recorded as `canceling` before any signal.
    Cancel(&'a str),
    /// A failure already decided the outcome: `canceled` for `CANCELED`,
    /// otherwise `failed`.
    Fail(RuntimeError),
}

enum Terminal {
    Canceled,
    Failed,
}

/// One observation of an owned service; callers map it to their phase's error.
enum Observation {
    Live,
    Escaped(RuntimeError),
    Exited(std::process::ExitStatus),
    Unobservable(std::io::Error),
    /// The leader no longer matches its recorded group or start identity.
    Moved,
}

/// Only this payload owns child resources. Optional fields exist solely for
/// move-out during consuming teardown.
struct OwnedService {
    info: ServiceInfo,
    child: OwnedChild,
    descendants: DescendantTracker,
    service: ExecService,
    ready_probe: PreparedProbe,
    health_probe: PreparedProbe,
    logs_dir: PathBuf,
    source_root: PathBuf,
    launcher: PathBuf,
    next_probe_occurrence: u64,
    redactor: Redactor,
    log_relays: Option<RedactedLogRelays>,
    // Set once when capture settles; read by terminal settlement.
    capture_outcome: Option<CaptureOutcome>,
}

impl OwnedService {
    /// One observation of the owned service: escape evidence refreshed at
    /// this checkpoint first, then the leader itself.
    fn observe(&self) -> RuntimeResult<Observation> {
        if let Some(error) = self.escape_error() {
            return Ok(Observation::Escaped(error));
        }
        let info = &self.info;
        Ok(match self.child.observe() {
            Err(error) => Observation::Unobservable(error),
            Ok(Some(status)) => Observation::Exited(status),
            Ok(None)
                if !process_is_live_with_identity(
                    info.pid,
                    info.pgid,
                    info.platform_start_identity.as_deref(),
                )? =>
            {
                Observation::Moved
            }
            Ok(None) => Observation::Live,
        })
    }

    /// A session checkpoint: any departure of a started service is a
    /// dependency failure, except escape or unobservability.
    fn check_liveness(&self) -> RuntimeResult<()> {
        if let Some(capture) = &self.log_relays {
            capture.check()?;
        }
        match self.observe()? {
            Observation::Live => Ok(()),
            Observation::Escaped(error) => Err(error),
            Observation::Unobservable(error) => Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to observe service child: {error}"),
            )),
            Observation::Exited(_) | Observation::Moved => Err(RuntimeError::new(
                ErrorCode::DependencyUnavailable,
                format!(
                    "service {} exited while session work was running",
                    self.info.service_name()
                ),
            )),
        }
    }

    /// Startup and teardown: every departure from a live, contained leader
    /// before `phase` is a containment failure.
    fn require_contained(&self, phase: &str) -> RuntimeResult<()> {
        match self.observe()? {
            Observation::Live => Ok(()),
            Observation::Escaped(error) => Err(error),
            Observation::Unobservable(error) => Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect service child: {error}"),
            )),
            Observation::Exited(status) => Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("service exited before {phase}: {status}"),
            )),
            Observation::Moved => Err(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!(
                    "service process {} no longer matches its recorded containment identity",
                    self.info.pid
                ),
            )),
        }
    }

    /// Run the readiness or health probe with its lifecycle evidence. Only
    /// readiness commits endpoint ownership; its ready commit is its success.
    fn run_probe(
        &mut self,
        registry: &mut Registry,
        phase: ProbePhase,
        cancellation: &CancellationToken,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let (record, probe) = match phase {
            ProbePhase::Ready => (
                LifecycleRecord::from_meta(&self.service.ready.meta, "ready"),
                self.ready_probe.clone(),
            ),
            ProbePhase::Health => (
                LifecycleRecord::from_meta(&self.service.health.meta, "health"),
                self.health_probe.clone(),
            ),
        };
        let context = self.lifecycle_event_context();
        record_lifecycle_started(registry, &context, &record)?;
        let ready_record = matches!(phase, ProbePhase::Ready).then_some(&record);
        match self.wait_probe_with_ownership(
            registry,
            &probe,
            cancellation,
            ready_record,
            checkpoint,
        ) {
            Ok(()) if ready_record.is_some() => Ok(()),
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
        probe: &PreparedProbe,
        cancellation: &CancellationToken,
        ready_record: Option<&LifecycleRecord>,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let (attempts, retry_interval, label) = probe_policy(probe);
        let mut last_pending = format!("probe {label} made no attempt");
        for attempt in 0..attempts {
            cancellation.check()?;
            self.check_probe_liveness(registry, checkpoint)?;
            let probe_attempt = self.probe_attempt(registry, probe, cancellation, checkpoint);
            if let Err(error) = self.check_owned_probe_liveness(registry) {
                return Err(match probe_attempt {
                    Err(probe_error) => error.with_cause(probe_error),
                    Ok(_) => error,
                });
            }
            let probe_attempt = probe_attempt?;
            checkpoint()?;
            cancellation.check()?;
            let observation = self.observe_endpoint_ownership();
            match observation {
                OwnershipObservation::Complete(ownership) => {
                    if matches!(probe_attempt, ProbeAttempt::Succeeded) {
                        if let Some(record) = ready_record {
                            self.commit_ready(registry, &ownership, record)?;
                        }
                        return Ok(());
                    }
                    if let ProbeAttempt::Failed(message) = probe_attempt {
                        last_pending = message;
                    }
                }
                OwnershipObservation::Missing(endpoint) => {
                    last_pending = match &probe_attempt {
                        ProbeAttempt::Failed(message) => message.clone(),
                        ProbeAttempt::Succeeded => format!(
                            "endpoint {} has no exact listener at {}:{}",
                            endpoint.endpoint_id, endpoint.host, endpoint.port
                        ),
                    };
                }
                OwnershipObservation::Outside {
                    endpoint,
                    listeners,
                } => {
                    return Err(port_conflict_error(
                        PortConflictReason::ListenerOccupied,
                        &registry.identity().project_id,
                        endpoint,
                        proven_nixfied_owner(registry, endpoint, &listeners, &self.service)?
                            .as_ref(),
                    ));
                }
                OwnershipObservation::Unverifiable { endpoint, message } => {
                    return Err(port_unverifiable_error(endpoint, message));
                }
                OwnershipObservation::ContainmentUnconfirmed { message } => {
                    return Err(RuntimeError::new(ErrorCode::ProcEscape, message));
                }
            }
            if attempt + 1 < attempts {
                let waiting = Instant::now();
                while waiting.elapsed() < retry_interval {
                    cancellation.check()?;
                    self.check_probe_liveness(registry, checkpoint)?;
                    thread::sleep(
                        retry_interval
                            .saturating_sub(waiting.elapsed())
                            .min(super::OBSERVATION_INTERVAL),
                    );
                }
            }
        }
        let timeout = RuntimeError::new(
            ErrorCode::ReadinessTimeout,
            format!(
                "readiness probe {label} did not reach probe-plus-ownership readiness: {last_pending}"
            ),
        );
        Err(self.override_with_endpoint_evidence(registry, timeout))
    }

    fn check_probe_liveness(
        &mut self,
        registry: &Registry,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        checkpoint()?;
        self.check_owned_probe_liveness(registry)
    }

    fn check_owned_probe_liveness(&mut self, registry: &Registry) -> RuntimeResult<()> {
        self.require_contained("readiness").map_err(|error| {
            self.override_after_primary_exit_with_endpoint_evidence(registry, error)
        })
    }

    fn probe_attempt(
        &mut self,
        registry: &mut Registry,
        probe: &PreparedProbe,
        cancellation: &CancellationToken,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> RuntimeResult<ProbeAttempt> {
        match probe {
            PreparedProbe::Tcp(probe) => {
                let endpoint = self.selected_endpoint().ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::LifecycleFailed,
                        "tcp probe on a service with no selected endpoint",
                    )
                })?;
                tcp_probe_attempt(
                    probe,
                    endpoint.host,
                    endpoint.port,
                    cancellation,
                    &mut || {
                        checkpoint()?;
                        self.check_liveness()
                    },
                )
            }
            PreparedProbe::Exec { policy, command } => {
                let occurrence = self.next_probe_occurrence;
                self.next_probe_occurrence = occurrence.checked_add(1).ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::StateUnwritable,
                        "probe occurrence sequence exhausted",
                    )
                })?;
                exec_probe_attempt(
                    policy,
                    ExecProbe {
                        command,
                        source_root: &self.source_root,
                        logs_dir: &self.logs_dir,
                        redactor: &self.redactor,
                        registry,
                        launcher: &self.launcher,
                        run_id: &self.info.run_id,
                        service_name: &self.info.service_name,
                        manifest_hash: &self.info.computed_manifest_hash,
                        occurrence,
                    },
                    cancellation,
                    &mut || {
                        checkpoint()?;
                        self.check_liveness()
                    },
                )
            }
        }
    }

    fn observe_endpoint_ownership(&self) -> OwnershipObservation<'_> {
        observe_ownership(
            &self.info.selected_endpoints,
            &ExpectedOwner {
                pid: self.info.pid,
                pgid: self.info.pgid,
                platform_start: self.info.platform_start_identity.as_deref(),
                containment: self.service.containment,
                tracked_processes: &[],
            },
        )
    }

    fn override_with_endpoint_evidence(
        &self,
        registry: &Registry,
        fallback: RuntimeError,
    ) -> RuntimeError {
        self.override_with_observation(registry, fallback, self.observe_endpoint_ownership())
    }

    fn override_after_primary_exit_with_endpoint_evidence(
        &self,
        registry: &Registry,
        fallback: RuntimeError,
    ) -> RuntimeError {
        let tracked_processes = self.descendants.known_descendants();
        self.override_with_observation(
            registry,
            fallback,
            observe_ownership_after_primary_exit(
                &self.info.selected_endpoints,
                &ExpectedOwner {
                    pid: self.info.pid,
                    pgid: self.info.pgid,
                    platform_start: self.info.platform_start_identity.as_deref(),
                    containment: self.service.containment,
                    tracked_processes: &tracked_processes,
                },
            ),
        )
    }

    fn override_with_observation(
        &self,
        registry: &Registry,
        fallback: RuntimeError,
        observation: OwnershipObservation<'_>,
    ) -> RuntimeError {
        match observation {
            OwnershipObservation::Outside {
                endpoint,
                listeners,
            } => match proven_nixfied_owner(registry, endpoint, &listeners, &self.service) {
                Ok(owner) => port_conflict_error(
                    PortConflictReason::ListenerOccupied,
                    &registry.identity().project_id,
                    endpoint,
                    owner.as_ref(),
                ),
                Err(error) => error,
            },
            OwnershipObservation::Unverifiable { endpoint, message } => {
                port_unverifiable_error(endpoint, message)
            }
            OwnershipObservation::ContainmentUnconfirmed { message } => {
                RuntimeError::new(ErrorCode::ProcEscape, message)
            }
            OwnershipObservation::Complete(_) | OwnershipObservation::Missing(_) => fallback,
        }
    }

    fn commit_ready(
        &self,
        registry: &mut Registry,
        ownership: &[EndpointOwnership<'_>],
        record: &LifecycleRecord,
    ) -> RuntimeResult<()> {
        let payloads = ownership
            .iter()
            .map(|ownership| {
                serde_json::to_string(ownership)
                    .map(|payload| {
                        (
                            endpoint_key(
                                &self.info.service_instance_id,
                                &ownership.endpoint.endpoint_id,
                            ),
                            ownership.endpoint.host.to_string(),
                            ownership.endpoint.port,
                            payload,
                        )
                    })
                    .map_err(|error| {
                        RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string())
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
            &self.info.run_id,
            &self.info.service_instance_id,
            &self.info.process_key,
            &self.info.computed_manifest_hash,
            &activations,
            (
                record.meta.operation_id.as_str(),
                record.class,
                &record.meta.terminal_success,
            ),
        )
    }

    fn settle_failed_service(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        error: RuntimeError,
    ) -> RuntimeError {
        self.teardown(registry, timeout_ms, Teardown::Fail(error))
            .expect_err("failure settlement retains its failure")
    }

    /// The one teardown of an owned service; the caller chooses its policy
    /// once. Durable intent precedes every signal, containment, reaping, and
    /// capture shutdown always run, and the terminal record follows the
    /// contained facts: an uncontained tree records an escape, and unsettled
    /// capture keeps ownership unresolved.
    fn teardown(
        &mut self,
        registry: &mut Registry,
        timeout_ms: u64,
        policy: Teardown<'_>,
    ) -> RuntimeResult<()> {
        let context = self.lifecycle_event_context();
        let stop_record = LifecycleRecord::from_meta(&self.service.stop.meta, "stop");
        let stop_failed = |registry: &mut Registry, error: &RuntimeError| {
            let _ = record_lifecycle_failure(registry, &context, &stop_record, error);
        };
        let (policy, intent) = match policy {
            // Pending exits and escapes are observed before the stop
            // lifecycle start event, the durable stop intent.
            Teardown::Stop(late) => match self.require_contained("stop") {
                Err(error) => {
                    stop_failed(registry, &error);
                    (Teardown::Fail(error), Ok(()))
                }
                Ok(()) => match record_lifecycle_started(registry, &context, &stop_record) {
                    Ok(()) => (Teardown::Stop(late), Ok(())),
                    Err(error) => (Teardown::Fail(error), Ok(())),
                },
            },
            Teardown::Cancel(reason) => {
                let intent = record_service_canceling(
                    registry,
                    &self.info.run_id,
                    &self.info.service_instance_id,
                    &self.info.process_key,
                    &self.info.computed_manifest_hash,
                    &self.event_payload(serde_json::json!({ "reason": reason })),
                );
                (Teardown::Cancel(reason), intent)
            }
            Teardown::Fail(error) => (Teardown::Fail(error), Ok(())),
        };
        // Graceful stop uses the declared signal and budget, capped by the
        // command timeout; every other teardown terminates with SIGTERM.
        let (signal, budget) = match policy {
            Teardown::Stop(_) => (
                stop_signal_number(self.service.stop.signal),
                (self.service.stop.timeout.as_millis() as u64).min(timeout_ms),
            ),
            Teardown::Cancel(_) | Teardown::Fail(_) => (libc::SIGTERM, timeout_ms),
        };
        let containment = match (self.contain(signal, budget), &policy) {
            (Ok(escalated), Teardown::Stop(_)) => {
                self.record_stop_signaled(registry, escalated, budget);
                Ok(())
            }
            (Ok(_), _) => Ok(()),
            (Err(error), Teardown::Stop(_)) => {
                stop_failed(registry, &error);
                Err(error)
            }
            (Err(error), _) => Err(error),
        };
        let (containment, capture) = self.finish_terminal_capture(containment);
        let (terminal, operation, cause, payload) = match policy {
            Teardown::Stop(Some(cancellation))
                if containment.is_ok() && capture.is_ok() && cancellation.is_canceled() =>
            {
                // The stop signal already ran; the cancellation that arrived
                // meanwhile decides the terminal record.
                let payload = self
                    .event_payload(serde_json::json!({ "reason": "run canceled during shutdown" }));
                record_service_canceling(
                    registry,
                    &self.info.run_id,
                    &self.info.service_instance_id,
                    &self.info.process_key,
                    &self.info.computed_manifest_hash,
                    &payload,
                )?;
                mark_service_canceled(
                    registry,
                    &self.info.run_id,
                    &self.info.service_instance_id,
                    &self.info.process_key,
                    &self.info.computed_manifest_hash,
                    &payload,
                    ServiceSettlement {
                        ownership: Ownership::Settled,
                        capture: CaptureOutcome::Complete,
                    },
                )?;
                let error = canceled_error();
                stop_failed(registry, &error);
                return Err(error);
            }
            Teardown::Stop(_) if containment.is_ok() && capture.is_ok() => {
                match self.escape_error() {
                    None => {
                        mark_service_stopped(
                            registry,
                            &self.info.run_id,
                            &self.info.service_instance_id,
                            &self.info.process_key,
                            &self.info.computed_manifest_hash,
                            Some(CaptureOutcome::Complete),
                        )?;
                        return record_lifecycle_success(registry, &context, &stop_record);
                    }
                    Some(error) => {
                        stop_failed(registry, &error);
                        (Terminal::Failed, Some(error), None, None)
                    }
                }
            }
            Teardown::Stop(_) => (Terminal::Failed, None, None, None),
            Teardown::Cancel(reason) => (
                Terminal::Canceled,
                intent.err(),
                Some(RuntimeError::new(ErrorCode::Canceled, reason)),
                Some(self.event_payload(serde_json::json!({ "reason": reason }))),
            ),
            Teardown::Fail(error) if error.code == ErrorCode::Canceled => {
                (Terminal::Canceled, Some(error), None, None)
            }
            Teardown::Fail(error) => (Terminal::Failed, Some(error), None, None),
        };
        if let Err(termination) = containment {
            let escape =
                self.settle_escape(registry, operation.as_ref().or(cause.as_ref()), termination);
            let error = completion_error(Err(escape), capture, operation)
                .expect("escape remains a failure");
            return Err(match cause {
                Some(cause) => error.with_cause(cause),
                None => error,
            });
        }
        // Contained processes with unsettled capture writers remain obligations.
        let settlement = ServiceSettlement {
            ownership: if capture.is_ok() {
                Ownership::Settled
            } else {
                Ownership::Unresolved
            },
            capture: self.capture_outcome(),
        };
        let error = completion_error(Ok(()), capture, operation).map(|error| match cause {
            Some(cause) => error.with_cause(cause),
            None => error,
        });
        let payload = payload.unwrap_or_else(|| {
            let error = error
                .as_ref()
                .expect("a failed terminal retains its failure");
            self.event_payload(serde_json::json!({
                "errorCode": error.code,
                "message": error.message.as_str(),
            }))
        });
        let mark = match terminal {
            Terminal::Canceled => mark_service_canceled,
            Terminal::Failed => mark_service_failed,
        };
        let settled = mark(
            registry,
            &self.info.run_id,
            &self.info.service_instance_id,
            &self.info.process_key,
            &self.info.computed_manifest_hash,
            &payload,
            settlement,
        );
        match completion_error(settled, Ok(()), error) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Lifecycle payload fields beside the recorded leader identity.
    fn event_payload(&self, fields: serde_json::Value) -> String {
        let mut payload = serde_json::json!({ "pid": self.info.pid, "pgid": self.info.pgid });
        if let (Some(payload), serde_json::Value::Object(fields)) =
            (payload.as_object_mut(), fields)
        {
            payload.extend(fields);
        }
        payload.to_string()
    }

    fn escape_error(&self) -> Option<RuntimeError> {
        // Refresh at the decision boundary so shutdown cannot signal the
        // group before recording a child that has already escaped it.
        if let Err(error) = self.refresh_descendants() {
            return Some(error);
        }
        let escaped = self.descendants.escaped_descendants();
        if escaped.is_empty() {
            return None;
        }
        let escaped_pids = escaped
            .iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        let message = format!(
            "service process {} has escaped descendants outside pgid {}: {:?}",
            self.info.pid, self.info.pgid, escaped_pids
        );
        Some(RuntimeError::new(ErrorCode::ProcEscape, message))
    }

    fn settle_escape(
        &mut self,
        registry: &mut Registry,
        operation_error: Option<&RuntimeError>,
        mut termination_error: RuntimeError,
    ) -> RuntimeError {
        let operation_error = operation_error.unwrap_or(&termination_error);
        let start_identity = self.escape_start_identity();
        let payload = self.event_payload(serde_json::json!({
            "errorCode": operation_error.code,
            "message": operation_error.message.as_str(),
            "terminationError": termination_error.message.as_str(),
        }));
        let process = ProcessRecord {
            process_key: &self.info.process_key,
            pid: self.info.pid,
            pgid: self.info.pgid,
            start_identity: &start_identity,
            command_json: "{}",
        };
        match mark_process_escape(
            registry,
            &self.info.run_id,
            &self.info.service_instance_id,
            &process,
            &self.info.computed_manifest_hash,
            self.info.platform_start_identity.as_deref(),
            &payload,
        ) {
            Ok(()) => {
                termination_error.message = format!(
                    "failed to prove termination of service process tree rooted at {}: {}",
                    self.info.pid, termination_error.message,
                );
                match super::registry::record_capture_outcome(
                    registry,
                    &self.info.run_id,
                    &self.info.process_key,
                    self.capture_outcome(),
                ) {
                    Ok(()) => termination_error,
                    Err(recording) => termination_error.with_cause(recording),
                }
            }
            Err(settlement_error) => settlement_error.absorb(termination_error),
        }
    }

    /// Record honest evidence of the stop mechanism that actually ran — the
    /// signal sent and whether it escalated to SIGKILL — rather than a fabricated
    /// exec terminal. Best-effort; failure to record does not fail the stop.
    fn record_stop_signaled(&self, registry: &mut Registry, escalated: bool, timeout_ms: u64) {
        let payload = self.event_payload(serde_json::json!({
            "signal": self.service.stop.signal,
            "signalNumber": stop_signal_number(self.service.stop.signal),
            "escalatedToKill": escalated,
            "timeoutMs": timeout_ms,
        }));
        let _ = record_service_lifecycle_event(
            registry,
            "service.stop.signaled",
            Some(self.info.run_id.as_str()),
            Some(&self.info.service_instance_id),
            Some(self.info.process_key.as_str()),
            &self.info.computed_manifest_hash,
            &payload,
        );
    }

    /// Contain the owned tree: refresh descendant evidence at this decision
    /// boundary, then signal and escalate across the group, the leader, and
    /// every descendant this owner ever tracked, whatever the declared
    /// containment. An escapee that already left the group, or that would
    /// reparent after the first signal, stays identity-tracked. Returns
    /// whether escalation to SIGKILL was required.
    fn contain(&self, signal: i32, timeout_ms: u64) -> RuntimeResult<bool> {
        let refreshed = self.refresh_descendants();
        let tracked = self.descendants.known_descendants();
        let contained = contain(
            &Leader::Child(&self.child),
            self.info.pgid,
            signal,
            timeout_ms,
            &tracked,
        );
        crate::error::both(contained, refreshed)
    }

    fn refresh_descendants(&self) -> RuntimeResult<()> {
        self.descendants.refresh(
            self.info.pid,
            self.info.pgid,
            matches!(
                self.service.containment,
                ContainmentRequirement::ProcessGroup
            ),
        )
    }

    fn selected_endpoint(&self) -> Option<&SelectedEndpoint> {
        self.info.selected_endpoint()
    }

    /// Always attempt reap and capture shutdown, including containment failure.
    /// Keep unresolved process ownership available for escape settlement/Drop.
    fn finish_terminal_capture(
        &mut self,
        containment: RuntimeResult<()>,
    ) -> (RuntimeResult<()>, RuntimeResult<()>) {
        let containment = crate::error::both(containment, reap_owned_child(&mut self.child));
        let capture = self.shutdown_capture();
        (containment, capture)
    }

    fn shutdown_capture(&mut self) -> RuntimeResult<()> {
        let (outcome, result) = shutdown_service_capture(self.log_relays.take());
        if let Some(outcome) = outcome {
            self.capture_outcome = Some(outcome);
        }
        result
    }

    /// The checked outcome once capture settled; unknown before settlement.
    fn capture_outcome(&self) -> CaptureOutcome {
        self.capture_outcome.unwrap_or(CaptureOutcome::Unknown)
    }

    fn lifecycle_event_context(&self) -> LifecycleEventContext {
        LifecycleEventContext {
            run_id: Some(self.info.run_id.clone()),
            service_name: self.info.service_name.clone(),
            service_instance_id: Some(self.info.service_instance_id.clone()),
            process_key: Some(self.info.process_key.clone()),
            computed_manifest_hash: self.info.computed_manifest_hash.clone(),
        }
    }

    fn escape_start_identity(&mut self) -> String {
        let known = self.descendants.known_descendants();
        process_escape_start_identity(
            self.info.pid,
            self.info.pgid,
            self.info.platform_start_identity.as_deref(),
            &known,
        )
    }
}

fn probe_policy(probe: &PreparedProbe) -> (u32, Duration, &str) {
    let policy = match probe {
        PreparedProbe::Tcp(policy) | PreparedProbe::Exec { policy, .. } => policy,
    };
    (
        policy.max_attempts.get(),
        policy.retry_interval,
        policy.label.as_str(),
    )
}

fn port_conflict_error(
    reason: PortConflictReason,
    project_id: &str,
    endpoint: &SelectedEndpoint,
    owner: Option<&NixfiedOwner>,
) -> RuntimeError {
    let reason_wire = serde_json::to_value(reason).expect("closed conflict reason serializes");
    let reason_text = reason_wire
        .as_str()
        .expect("closed conflict reason is a string");
    let details = PortConflictDetails {
        reason,
        project_id,
        endpoint: PortConflictEndpoint {
            transport: "tcp",
            family: match endpoint.host.ip() {
                std::net::IpAddr::V4(_) => "ipv4",
                std::net::IpAddr::V6(_) => "ipv6",
            },
            host: &endpoint.host,
            port: endpoint.port,
            endpoint_id: &endpoint.endpoint_id,
        },
        nixfied_owner: owner,
    };
    RuntimeError::new(
        ErrorCode::PortConflict,
        format!(
            "endpoint {} is unavailable at {}:{} ({reason_text})",
            endpoint.endpoint_id, endpoint.host, endpoint.port
        ),
    )
    .with_detail(PORT_CONFLICT_KEY, details)
}

fn port_unverifiable_error(
    endpoint: Option<&SelectedEndpoint>,
    message: impl Into<String>,
) -> RuntimeError {
    let mut error = RuntimeError::new(ErrorCode::PortUnverifiable, message);
    if let Some(endpoint) = endpoint {
        error = error
            .with_detail("endpointId", &endpoint.endpoint_id)
            .with_detail("address", endpoint.host)
            .with_detail("port", endpoint.port);
    }
    error
}

fn proven_nixfied_owner(
    registry: &Registry,
    endpoint: &SelectedEndpoint,
    listeners: &[ListenerRecord],
    requested_service: &ExecService,
) -> RuntimeResult<Option<NixfiedOwner>> {
    if listeners.is_empty() {
        return Ok(None);
    }
    let address = endpoint.host.to_string();
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
                params![address, endpoint.port, PortStatus::Active.as_str()],
                |row| row.get::<_, String>(0),
            )
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    for service_instance_id in candidates {
        let snapshot = read_service_snapshot(registry, &service_instance_id)?;
        let Some(process) = &snapshot.process else {
            continue;
        };
        if !status::PROCESS_ACTIVE.contains(&process.status)
            || !snapshot.endpoints.iter().any(|stored| {
                stored.address == address
                    && stored.port == endpoint.port
                    && stored.status == PortStatus::Active
                    && stored.owner_process_key == process.process_key
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
            observe_single_ownership(
                endpoint,
                &ExpectedOwner {
                    pid: process.pid,
                    pgid: process.pgid,
                    platform_start: process.platform_start.as_deref(),
                    containment: requested_service.containment,
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
            service_id: process.service_name.clone(),
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
) -> RuntimeError {
    match failure {
        EndpointFailure::LockContended { endpoint } => port_conflict_error(
            PortConflictReason::StartupLockContended,
            &registry.identity().project_id,
            &endpoint,
            None,
        ),
        EndpointFailure::ListenerOccupied {
            endpoint,
            listeners,
        } => match proven_nixfied_owner(registry, &endpoint, &listeners, service) {
            Ok(owner) => port_conflict_error(
                PortConflictReason::ListenerOccupied,
                &registry.identity().project_id,
                &endpoint,
                owner.as_ref(),
            ),
            Err(error) => error,
        },
        EndpointFailure::Unverifiable { endpoint, message } => {
            port_unverifiable_error(endpoint.as_ref(), message)
        }
    }
}

impl Drop for OwnedService {
    /// Best-effort unwind containment of a still-running leader, including
    /// its tracked descendants.
    fn drop(&mut self) {
        if self.child.observe().ok().flatten().is_none() {
            let _ = self.contain(libc::SIGTERM, 100);
        }
        let _ = self.shutdown_capture();
    }
}

fn shutdown_service_capture(
    capture: Option<RedactedLogRelays>,
) -> (Option<CaptureOutcome>, RuntimeResult<()>) {
    match capture {
        Some(capture) => {
            let (outcome, result) = capture.shutdown(Instant::now() + CAPTURE_SHUTDOWN_TIMEOUT);
            (Some(outcome), result)
        }
        None => (None, Ok(())),
    }
}

/// One service's slice of the slot plan: its name, the planned port for each of
/// its endpoints (keyed by endpointId), and the cross-service slot endpoint map
/// `${port:<serviceId>}` resolves against.
pub struct ServiceSelection<'a> {
    pub launcher: &'a Path,
    pub service_name: &'a str,
    pub endpoint_ports: &'a BTreeMap<String, u16>,
    pub slot_endpoints: &'a SlotEndpoints,
    pub run_timeout_ms: u64,
    pub cancellation: &'a CancellationToken,
    /// Observes every service the session already started. Release and the
    /// startup grace consult it, so no workload is released after another
    /// owned service already failed.
    pub session_checkpoint: &'a dyn Fn() -> RuntimeResult<()>,
    /// Executes the service's prepare task (its flattened nodes) inside the
    /// held endpoint startup guards. Supplied by the run driver, which owns the started
    /// services the prepare leaves may require; `None` when the service
    /// declares no prepare task. The runtime stays generic: this is plumbing,
    /// not vocabulary.
    pub prepare_runner: Option<PrepareRunner<'a>>,
}

/// The prepare-task executor a run driver supplies.
pub type PrepareRunner<'a> = Box<dyn FnMut(&mut Registry) -> RuntimeResult<()> + 'a>;

/// Start a declared service from the lowered manifest: run prepare,
/// spawn-and-own the start exec, and track the process. The service is read from
/// the admission's `ExecutionManifest`, never the raw `Manifest`. The caller must have
/// recorded this exact `run_id` with [`super::record_run_created`] first.
pub fn start_service_for_slot(
    admission: &RunAdmission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_slot: &SelectedSlot<'_>,
    selection: ServiceSelection<'_>,
) -> RuntimeResult<StartingService> {
    if registry.identity().environment != selected_slot.environment
        || registry.identity().slot != i64::from(selected_slot.slot)
    {
        return Err(RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            "service selection does not match the owned slot",
        ));
    }
    start_service_with_lock_root(
        admission,
        placement,
        registry,
        run_id,
        selection,
        LockRoot::Fixed,
    )
}

pub(super) fn start_service_with_lock_root(
    admission: &RunAdmission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    mut selection: ServiceSelection<'_>,
    lock_root: LockRoot<'_>,
) -> RuntimeResult<StartingService> {
    let run_id = run_id.into();
    let run_timeout_ms = selection.run_timeout_ms;
    let cancellation = selection.cancellation;
    let session_checkpoint = selection.session_checkpoint;
    let source = admission.source();
    let service_name = selection.service_name;
    let endpoint_ports = selection.endpoint_ports;
    let slot_endpoints = selection.slot_endpoints;
    let service = admission
        .common()
        .execution_manifest()
        .services()
        .get(service_name)
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::LifecycleFailed,
                format!("service {service_name} is missing"),
            )
        })?;
    // Bind every modelled endpoint to its planned port. The map is keyed by
    // endpointId, the scope `${port:<endpointId>}` resolves against.
    let mut own_endpoints: BTreeMap<String, SelectedEndpoint> = BTreeMap::new();
    for (endpoint_id, endpoint) in &service.endpoints {
        let port = endpoint_ports.get(endpoint_id).copied().ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::LifecycleFailed,
                format!("service {service_name} endpoint {endpoint_id} has no planned port"),
            )
        })?;
        own_endpoints.insert(
            endpoint_id.clone(),
            SelectedEndpoint {
                endpoint_id: endpoint.endpoint_id.clone(),
                host: endpoint.host,
                port,
            },
        );
    }
    // An endpoint-less service has no primary: it makes no addressability
    // claim, so there is no selected endpoint to record or probe over tcp.
    let selected_endpoint = match &service.primary_endpoint {
        Some(primary) => Some(own_endpoints.get(primary).ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::LifecycleFailed,
                format!("service {service_name} primary endpoint {primary} is missing"),
            )
        })?),
        None => None,
    };
    // The named substitution scope is the declared connectsTo set; lowering
    // proved every cross-service placeholder references a member of it.
    let named: SlotEndpoints = slot_endpoints
        .iter()
        .filter(|(id, _)| service.connects_to.contains(id))
        .map(|(id, endpoint)| (id.clone(), endpoint.clone()))
        .collect();
    let substitution = ExecSubstitution {
        own_primary: selected_endpoint,
        own_endpoints: &own_endpoints,
        named: &named,
        state_root: &placement.state_root(),
        secrets: admission.secrets(),
    };
    // Exec probe args/env are substituted once here, with the same scope as the
    // start exec, so probe attempts later need no endpoint context.
    let ready_probe = prepare_probe(&service.ready.probe, &substitution)?;
    let health_probe = prepare_probe(&service.health.probe, &substitution)?;
    let service_instance_id = service_instance_id(&run_id, service_name);
    // Stable backing storage for process-bound endpoint evidence.
    let endpoint_records: Vec<(String, String, u16)> = own_endpoints
        .values()
        .map(|endpoint| {
            (
                endpoint_key(&service_instance_id, &endpoint.endpoint_id),
                endpoint.host.to_string(),
                endpoint.port,
            )
        })
        .collect();
    let endpoints_to_record: Vec<EndpointRecord<'_>> = endpoint_records
        .iter()
        .map(|(key, address, port)| EndpointRecord {
            endpoint_key: key,
            address,
            port: *port,
        })
        .collect();
    let evidence = crate::output::EvidenceSource::in_logs(
        &placement.logs_dir(),
        service_name,
        crate::output::SourcePresentation::Shown,
        &format!("service.{service_name}"),
    );
    let service_record = ServiceRecord {
        service_instance_id: &service_instance_id,
        service_name,
        source: &evidence,
        stop: StopPolicy {
            signal: stop_signal_number(service.stop.signal),
            timeout_ms: service.stop.timeout.as_millis() as u64,
            containment: service.containment,
        },
    };
    cancellation.check()?;
    let startup_guards = match acquire_startup_locks(own_endpoints.values(), lock_root) {
        Ok(guards) => guards,
        Err(failure) => return Err(endpoint_failure_error(registry, service, failure)),
    };
    cancellation.check()?;
    if let Err(failure) = preflight(own_endpoints.values()) {
        return Err(endpoint_failure_error(registry, service, failure));
    }
    // Record startup intent after endpoint preflight and before prepare.
    // The slot owner and host startup guards stay held through readiness.
    record_service_start_intent(
        registry,
        &run_id,
        admission.common().computed_manifest_hash(),
        &service_instance_id,
    )?;
    let lifecycle_context = LifecycleEventContext {
        run_id: Some(run_id.clone()),
        service_name: service_name.to_owned(),
        service_instance_id: Some(service_instance_id.clone()),
        process_key: None,
        computed_manifest_hash: admission.common().computed_manifest_hash().to_owned(),
    };
    let start_record = LifecycleRecord::from_meta(&service.start.meta, "start");
    // Until spawn succeeds, every failure can settle startup directly.
    // Once a child exists, the separate paths below must first prove containment.
    let (pending, command_json, redactor, mut log_relays) = (|| {
        cancellation.check()?;
        // prepare is a task reference: the caller supplies a runner that executes
        // the referenced task's flattened nodes (ordinary task evidence — logs,
        // summaries, registry rows keyed by step path). Slot authority and
        // endpoint startup guards remain held throughout preparation.
        if let Some(prepare_task) = &service.prepare {
            let prepare_record = LifecycleRecord::from_meta(
                &OpMeta {
                    operation_id: nixfied_manifest::OperationId::new(prepare_task.as_str()),
                    terminal_success: "initialized".to_string(),
                    terminal_failure: "failed".to_string(),
                },
                "prepare",
            );
            record_lifecycle_started(registry, &lifecycle_context, &prepare_record)?;
            let prepare_result = match selection.prepare_runner {
                Some(ref mut runner) => runner(registry),
                None => Err(RuntimeError::new(
                    ErrorCode::LifecycleFailed,
                    format!(
                        "service {service_name} declares prepare task {prepare_task} but the caller supplied no prepare runner"
                    ),
                )),
            };
            if let Err(error) = prepare_result {
                let _ = record_lifecycle_failure(registry, &lifecycle_context, &prepare_record, &error);
                return Err(error);
            }
            record_lifecycle_success(registry, &lifecycle_context, &prepare_record)?;
            cancellation.check()?;
        }
        record_lifecycle_started(registry, &lifecycle_context, &start_record)?;
        let exec = &service.start.exec;
        let command_cwd = resolve_exec_cwd(&source.observed_root, &exec.cwd)?;
        let args = substitution.args(&exec.args)?;
        let declared_env = substitution.env(&exec.env)?;
        let env = exec.env_with_path(declared_env);
        let stdout_path = &evidence.stdout;
        let stderr_path = &evidence.stderr;
        let command_json = serde_json::to_string(&CommandRecord {
            executable: exec.executable.as_str(),
            args: &args,
            cwd: command_cwd.as_path(),
            stdout_path: stdout_path.as_path(),
            stderr_path: stderr_path.as_path(),
        })
        .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
        let request = crate::launch::PreparedLaunch::new(&exec.executable, &args, &env, &command_cwd)?;
        let redactor = Redactor::from_secrets(admission.secrets());
        // Service output always passes through owned capture workers, so writer
        // closure is checked rather than inferred from the leader's exit.
        let output = child_output(stdout_path, stderr_path, &redactor, LogFileMode::Replace)?;
        let (stdout, stderr, log_relays) = (output.stdout, output.stderr, Some(output.relays));
        let spawned = cancellation.check().and_then(|()| request.spawn(
            selection.launcher, registry.authority(), stdin_for(exec.stdin), stdout, stderr,
        ));
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let error = completion_error(Ok(()), shutdown_service_capture(log_relays).1, Some(error)).unwrap();
                let _ = record_lifecycle_failure(registry, &lifecycle_context, &start_record, &error);
                return Err(error);
            }
        };
        Ok((child, command_json, redactor, log_relays))
    })()
    .map_err(|error| settle_reserved_failure(registry, &run_id, &service_instance_id, error))?;
    let pid = pending.id();
    // Before the start record commits, cleanup owns the child and capture, but
    // may settle startup only after proving containment.
    let mut fail_unrecorded = |registry: &mut Registry,
                               child: &mut OwnedChild,
                               context: &LifecycleEventContext,
                               pgid,
                               error: RuntimeError| {
        let _ = record_lifecycle_failure(registry, context, &start_record, &error);
        let containment = terminate_unrecorded_child(child, pgid, service, run_timeout_ms);
        let contained = containment.is_ok();
        let error = completion_error(
            containment,
            shutdown_service_capture(log_relays.take()).1,
            Some(error),
        )
        .unwrap();
        if contained {
            settle_reserved_failure(registry, &run_id, &service_instance_id, error)
        } else {
            error
        }
    };
    let (pgid, platform_start) = match leader_identity(pid, "service") {
        Ok((pgid, start)) => (pgid, Some(start)),
        Err(error) => {
            let mut child = OwnedChild::from(pending.abort());
            return Err(fail_unrecorded(
                registry,
                &mut child,
                &lifecycle_context,
                None,
                error,
            ));
        }
    };
    let start_identity =
        super::StoredProcessIdentity::encode(pid, pgid, platform_start.as_deref(), Some(&[]));
    let process_key = format!("process-{run_id}-{pid}-{pgid}");
    let started_context = LifecycleEventContext {
        run_id: Some(run_id.clone()),
        service_name: service_name.to_owned(),
        service_instance_id: Some(service_instance_id.clone()),
        process_key: Some(process_key.clone()),
        computed_manifest_hash: admission.common().computed_manifest_hash().to_owned(),
    };
    let descendants = DescendantTracker::default();
    let (child, launch_error) = match pending.register_and_release(
        |_| {
            record_service_start(
                registry,
                &run_id,
                admission.common().computed_manifest_hash(),
                &service_record,
                &ProcessRecord {
                    process_key: &process_key,
                    pid,
                    pgid,
                    start_identity: &start_identity,
                    command_json: &command_json,
                },
                &endpoints_to_record,
            )
        },
        || {
            cancellation.check()?;
            session_checkpoint()
        },
    ) {
        Ok(child) => (child, None),
        Err(Refusal::Registered(failure)) => (failure.child, Some(*failure.error)),
        Err(Refusal::Unregistered(failure)) => {
            let mut child = OwnedChild::from(failure.child);
            return Err(fail_unrecorded(
                registry,
                &mut child,
                &started_context,
                Some(pgid),
                *failure.error,
            ));
        }
    };
    let mut started = StartingService {
        startup_guards,
        owned: Box::new(OwnedService {
            info: ServiceInfo {
                run_id,
                service_instance_id,
                process_key,
                pid,
                pgid,
                platform_start_identity: platform_start,
                selected_endpoints: own_endpoints,
                computed_manifest_hash: admission.common().computed_manifest_hash().to_owned(),
                service_name: service.name.to_string(),
                primary_endpoint: service.primary_endpoint.clone(),
            },
            child: child.into(),
            descendants,
            service: service.clone(),
            ready_probe,
            health_probe,
            logs_dir: placement.logs_dir().clone(),
            source_root: source.observed_root.clone(),
            launcher: selection.launcher.to_owned(),
            next_probe_occurrence: 0,
            redactor,
            log_relays,
            capture_outcome: None,
        }),
    };
    if let Some(error) = launch_error {
        let _ = record_lifecycle_failure(registry, &started_context, &start_record, &error);
        return Err(started.finalize_failed_start(registry, run_timeout_ms, error));
    }
    if let Err(error) = hold_startup_grace(&mut started.owned, cancellation, session_checkpoint) {
        let error = if error.code == ErrorCode::Canceled {
            error
        } else {
            started
                .owned
                .override_after_primary_exit_with_endpoint_evidence(registry, error)
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

/// Clean every declared service of the slot, then clean the marker-owned slot
/// state once. Membership does not exist; every declared service may have left
/// slot evidence, so each one's clean lifecycle operation is recorded. Each is
/// a marker-gated runtime cleanup primitive (no exec).
pub fn run_slot_clean(
    admission: &ControlAdmission,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    for service in admission.execution_manifest().services().values() {
        record_service_clean(admission, registry, service)?;
    }
    clean_marked_slot_state(admission, registry, selected_slot, mode)
}

/// Record the marker-gated clean lifecycle operation for one service.
fn record_service_clean(
    admission: &ControlAdmission,
    registry: &mut Registry,
    service: &ExecService,
) -> RuntimeResult<()> {
    let record = LifecycleRecord::from_meta(&service.clean.meta, "clean");
    let lifecycle_context = LifecycleEventContext {
        run_id: None,
        service_name: service.name.to_string(),
        service_instance_id: None,
        process_key: None,
        computed_manifest_hash: admission.computed_manifest_hash().to_owned(),
    };
    record_lifecycle_started(registry, &lifecycle_context, &record)?;
    record_lifecycle_success(registry, &lifecycle_context, &record)
}

/// Clean the marker-owned state root for the selected slot.
fn clean_marked_slot_state(
    admission: &ControlAdmission,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
    mode: CleanupMode,
) -> RuntimeResult<CleanupOutcome> {
    let identity = StateIdentity::from_selected_slot(admission, selected_slot);
    // The caller already performed exclusive predecessor recovery.
    clean_marked_state(&identity, registry, mode)
}

fn settle_reserved_failure(
    registry: &mut Registry,
    run_id: &str,
    service_instance_id: &str,
    error: RuntimeError,
) -> RuntimeError {
    match settle_service_start(
        registry,
        run_id,
        service_instance_id,
        if error.code == ErrorCode::Canceled {
            ServiceStartOutcome::Canceled
        } else {
            ServiceStartOutcome::Failed
        },
    ) {
        Ok(()) => error,
        Err(settlement_error) => settlement_error.with_cause(error),
    }
}

fn terminate_unrecorded_child(
    child: &mut OwnedChild,
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
    let containment = contain(&Leader::Child(child), pgid, libc::SIGTERM, timeout_ms, &[]);
    crate::error::both(containment.map(|_| ()), reap_owned_child(child))
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
pub type SlotEndpoints = std::collections::BTreeMap<nixfied_manifest::ServiceId, SelectedEndpoint>;

/// Render checked references against selected runtime facts. Inserted values
/// remain opaque; endpoint selectors were resolved to their namespace at lowering.
pub(crate) struct ExecSubstitution<'a> {
    pub own_primary: Option<&'a SelectedEndpoint>,
    pub own_endpoints: &'a BTreeMap<String, SelectedEndpoint>,
    pub named: &'a SlotEndpoints,
    pub state_root: &'a Path,
    pub secrets: &'a ResolvedSecrets,
}

impl ExecSubstitution<'_> {
    fn command(&self, exec: &ResolvedInvocation) -> RuntimeResult<RenderedInvocation> {
        Ok(RenderedInvocation {
            executable: exec.executable.clone(),
            args: self.args(&exec.args)?,
            env: exec.env_with_path(self.env(&exec.env)?),
            cwd: exec.cwd.clone(),
            stdin: exec.stdin,
        })
    }

    pub(crate) fn value(&self, template: &Template) -> RuntimeResult<String> {
        let mut out = String::new();
        for piece in template.pieces() {
            match piece {
                Piece::Literal(text) => out.push_str(text),
                Piece::StateDir => out.push_str(&self.state_root.to_string_lossy()),
                Piece::Secret(id) => out.push_str(self.secrets.get(id).ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::LifecycleFailed,
                        format!("admitted secret {id} is missing from render context"),
                    )
                })?),
                Piece::Port(selector) | Piece::Host(selector) => {
                    let endpoint = match selector {
                        EndpointSelector::Primary => self.own_primary,
                        EndpointSelector::Own(id) => self.own_endpoints.get(id),
                        EndpointSelector::Service(id) => self.named.get(id),
                    }
                    .ok_or_else(|| {
                        RuntimeError::new(
                            ErrorCode::LifecycleFailed,
                            "admitted endpoint is missing from render context",
                        )
                    })?;
                    match piece {
                        Piece::Port(_) => out.push_str(&endpoint.port.to_string()),
                        Piece::Host(_) => out.push_str(&endpoint.host.to_string()),
                        _ => unreachable!(),
                    }
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn args(&self, args: &[Template]) -> RuntimeResult<Vec<String>> {
        args.iter().map(|arg| self.value(arg)).collect()
    }

    pub(crate) fn env(
        &self,
        env: &BTreeMap<String, Template>,
    ) -> RuntimeResult<BTreeMap<String, String>> {
        env.iter()
            .map(|(key, value)| Ok((key.clone(), self.value(value)?)))
            .collect()
    }
}

pub(crate) fn resolve_exec_cwd(
    source_root: &Path,
    exec_cwd: &crate::execution::RelativeCwd,
) -> RuntimeResult<PathBuf> {
    let exec_cwd = exec_cwd.as_str();
    let relative = Path::new(exec_cwd);
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

/// Concrete values are distinct from authored invocation templates. They are
/// rendered once and never parsed again by a probe attempt.
#[derive(Clone)]
pub(crate) struct RenderedInvocation {
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: RelativeCwd,
    pub stdin: StdinPolicy,
}

#[derive(Clone)]
enum PreparedProbe {
    Tcp(ProbePolicy),
    Exec {
        policy: ProbePolicy,
        command: RenderedInvocation,
    },
}

fn prepare_probe(
    probe: &Probe,
    substitution: &ExecSubstitution<'_>,
) -> RuntimeResult<PreparedProbe> {
    match probe {
        Probe::Tcp(policy) => Ok(PreparedProbe::Tcp(policy.clone())),
        Probe::Exec(probe) => Ok(PreparedProbe::Exec {
            policy: probe.policy.clone(),
            command: substitution.command(&probe.exec)?,
        }),
    }
}

/// A fully-substituted task or probe command with captured streams and an
/// optional deadline. Probe callers always supply their finite attempt limit.
pub(crate) struct CapturedExec<'a> {
    pub authority: &'a SlotGuard,
    pub executable: &'a str,
    pub args: &'a [String],
    pub env: &'a BTreeMap<String, String>,
    pub cwd: &'a Path,
    pub stdin: StdinPolicy,
    pub timeout: Option<Duration>,
    pub stdout_path: &'a Path,
    pub stderr_path: &'a Path,
    pub redactor: &'a Redactor,
    pub log_file_mode: LogFileMode,
    /// Names the operation in spawn/inspect failures.
    pub label: &'a str,
}

pub(crate) enum CapturedExecOutcome {
    Exited(std::process::ExitStatus),
    TimedOut,
    Canceled,
}

/// Capture and containment obligations survive direct-child reaping. Tasks and
/// probes both register before entering this owner; it has no registry dependency.
pub(crate) struct OwnedCapturedChild {
    child: OwnedChild,
    capture: Option<RedactedLogRelays>,
    timeout: Option<Duration>,
    label: String,
}

pub(crate) enum TerminationReason {
    Canceled,
    TimedOut,
    ObservationFailed,
}

pub(crate) enum CapturedExecTransition<'a> {
    Observed(&'a CapturedExecOutcome),
    Terminating(TerminationReason),
}

pub(crate) struct CapturedExecFailure {
    pub error: Box<RuntimeError>,
    pub outcome: Option<CapturedExecOutcome>,
    /// Containment, reaping, and capture all settled; only the operation failed.
    pub settled: bool,
    /// The checked capture outcome after writer settlement.
    pub capture: crate::redaction::CaptureOutcome,
}

/// Inert captured bootstrap. Capture and the child stay owned through failed
/// registration/delivery; normal completion only accepts an authorized child.
pub(crate) struct PendingCapturedChild {
    pending: Option<crate::launch::PendingLaunch>,
    capture: Option<RedactedLogRelays>,
    timeout: Option<Duration>,
    label: String,
}

impl PendingCapturedChild {
    pub fn pid(&self) -> u32 {
        self.pending.as_ref().expect("pending child is owned").id()
    }

    pub fn register_and_release(
        mut self,
        register: impl FnOnce(&Child) -> RuntimeResult<()>,
        checkpoint: impl FnMut() -> RuntimeResult<()>,
    ) -> Result<OwnedCapturedChild, Refusal<Box<RuntimeError>, CapturedExecFailure>> {
        let result = self
            .pending
            .take()
            .expect("pending child is owned")
            .register_and_release(register, checkpoint);
        let mut finish = |failure: crate::launch::LaunchFailure| {
            let (error, settled, capture) = self.owned(failure.child).finish(Some(*failure.error));
            let error = Box::new(error.expect("release failure retains its error"));
            (error, settled, capture)
        };
        match result {
            Ok(child) => Ok(self.owned(child)),
            Err(Refusal::Unregistered(failure)) => Err(Refusal::Unregistered(finish(failure).0)),
            Err(Refusal::Registered(failure)) => {
                let (error, settled, capture) = finish(failure);
                Err(Refusal::Registered(CapturedExecFailure {
                    error,
                    outcome: None,
                    settled,
                    capture,
                }))
            }
        }
    }

    fn owned(&mut self, child: Child) -> OwnedCapturedChild {
        OwnedCapturedChild {
            child: child.into(),
            capture: self.capture.take(),
            timeout: self.timeout,
            label: std::mem::take(&mut self.label),
        }
    }
}

impl Drop for PendingCapturedChild {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            // Unwind fallback uses the same captured-child containment path.
            drop(self.owned(pending.abort()));
        }
    }
}

pub(crate) fn spawn_gated_captured_exec(
    spec: &CapturedExec<'_>,
    launcher: &Path,
) -> RuntimeResult<PendingCapturedChild> {
    let request =
        crate::launch::PreparedLaunch::new(spec.executable, spec.args, spec.env, spec.cwd)?;
    let output = child_output(
        spec.stdout_path,
        spec.stderr_path,
        spec.redactor,
        spec.log_file_mode,
    )?;
    match request.spawn(
        launcher,
        spec.authority,
        stdin_for(spec.stdin),
        output.stdout,
        output.stderr,
    ) {
        Ok(pending) => Ok(PendingCapturedChild {
            pending: Some(pending),
            capture: Some(output.relays),
            timeout: spec.timeout,
            label: spec.label.into(),
        }),
        Err(error) => {
            let (_, capture) = output
                .relays
                .shutdown(Instant::now() + CAPTURE_SHUTDOWN_TIMEOUT);
            Err(completion_error(Ok(()), capture, Some(error)).expect("spawn failure is retained"))
        }
    }
}

impl OwnedCapturedChild {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn complete(
        mut self,
        cancellation: &CancellationToken,
        mut checkpoint: impl FnMut() -> RuntimeResult<()>,
        mut transition: impl FnMut(CapturedExecTransition<'_>) -> RuntimeResult<()>,
    ) -> Result<CapturedExecOutcome, CapturedExecFailure> {
        let started = Instant::now();
        let mut capture_failed = false;
        let observed = loop {
            if cancellation.is_canceled() {
                break Ok(CapturedExecOutcome::Canceled);
            }
            if let Err(error) = checkpoint() {
                break Err(error);
            }
            match self.child.observe() {
                Ok(Some(status)) => break Ok(CapturedExecOutcome::Exited(status)),
                Ok(None) => {}
                Err(error) => {
                    break Err(RuntimeError::new(
                        ErrorCode::ProcEscape,
                        format!("failed to inspect {}: {error}", self.label),
                    ));
                }
            }
            if let Err(error) = self.capture.as_ref().expect("capture is owned").check() {
                capture_failed = true;
                break Err(error);
            }
            if self
                .timeout
                .is_some_and(|timeout| started.elapsed() >= timeout)
            {
                break Ok(CapturedExecOutcome::TimedOut);
            }
            thread::sleep(super::OBSERVATION_INTERVAL);
        };
        let recording = match &observed {
            Ok(outcome) => transition(CapturedExecTransition::Observed(outcome)),
            Err(_) => Ok(()),
        };
        // Intent is synchronous and precedes every signal, even if recording fails.
        let intent = match &observed {
            Ok(CapturedExecOutcome::Canceled) => transition(CapturedExecTransition::Terminating(
                TerminationReason::Canceled,
            )),
            Ok(CapturedExecOutcome::TimedOut) => transition(CapturedExecTransition::Terminating(
                TerminationReason::TimedOut,
            )),
            Err(_) => transition(CapturedExecTransition::Terminating(
                TerminationReason::ObservationFailed,
            )),
            Ok(CapturedExecOutcome::Exited(_)) => Ok(()),
        };
        let intent = crate::error::both(recording, intent);
        let (outcome, operation) = match observed {
            Ok(outcome) => (Some(outcome), intent.err()),
            // Shutdown consumes the retained capture failure; do not duplicate
            // its checkpoint preview as a second operation failure.
            Err(_) if capture_failed => (None, intent.err()),
            Err(error) => (
                None,
                Some(match intent {
                    Ok(()) => error,
                    Err(intent_error) => error.with_cause(intent_error),
                }),
            ),
        };
        match self.finish(operation) {
            (Some(error), settled, capture) => Err(CapturedExecFailure {
                error: Box::new(error),
                outcome,
                settled,
                capture,
            }),
            (None, _, _) => Ok(outcome.expect("successful observation supplies an outcome")),
        }
    }

    fn finish(
        &mut self,
        operation: Option<RuntimeError>,
    ) -> (Option<RuntimeError>, bool, crate::redaction::CaptureOutcome) {
        let pgid = self.pid() as i32;
        let containment = terminate_and_reap(&mut self.child, pgid);
        let (outcome, capture) = self
            .capture
            .take()
            .expect("completion consumes capture once")
            .shutdown(Instant::now() + CAPTURE_SHUTDOWN_TIMEOUT);
        let settled = containment.is_ok() && capture.is_ok();
        (
            completion_error(containment, capture, operation),
            settled,
            outcome,
        )
    }
}

impl Drop for OwnedCapturedChild {
    fn drop(&mut self) {
        if self.capture.is_some() {
            let _ = self.finish(None);
        }
    }
}

/// One registered task or probe invocation: its owner, evidence, and how its
/// captured outcomes are recorded.
pub(crate) struct Invocation<'a> {
    pub owner: InvocationOwner<'a>,
    pub run_id: &'a str,
    pub manifest_hash: &'a str,
    pub source: &'a crate::output::EvidenceSource,
    pub command_json: &'a str,
    /// The exit code and terminal status a captured outcome records.
    pub terminal: &'a dyn Fn(&CapturedExecOutcome) -> (Option<i32>, TaskTerminalStatus),
    /// The canceling-intent payload for the owned process group.
    pub canceling: &'a dyn Fn(i32, &TerminationReason) -> String,
}

/// A failed invocation. Any registered row is already settled from the
/// contained facts, or keeps its obligation with its capture outcome recorded.
pub(crate) struct InvocationFailure {
    pub error: Box<RuntimeError>,
    /// The outcome observed before the failure, if any.
    pub outcome: Option<CapturedExecOutcome>,
}

impl Invocation<'_> {
    pub fn identity<'k>(&'k self, process_key: &'k str) -> InvocationIdentity<'k> {
        InvocationIdentity {
            run_id: self.run_id,
            process_key,
            manifest_hash: self.manifest_hash,
            owner: self.owner,
        }
    }

    /// Register the gated child's verified group and start identity, release
    /// it under `checkpoint` and cancellation, and complete it. Returns the
    /// process key with the observed outcome; the caller records the
    /// successful terminal.
    pub fn run(
        &self,
        registry: &mut Registry,
        pending: PendingCapturedChild,
        process_key: impl FnOnce(u32) -> String,
        cancellation: &CancellationToken,
        checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
    ) -> Result<(String, CapturedExecOutcome), InvocationFailure> {
        let unregistered = |error| InvocationFailure {
            error,
            outcome: None,
        };
        let pid = pending.pid();
        let process_key = process_key(pid);
        let identity = self.identity(&process_key);
        let role = match self.owner {
            InvocationOwner::Task => "task",
            InvocationOwner::Probe(_) => "probe",
        };
        let released = pending.register_and_release(
            |_| {
                let (pgid, start) = leader_identity(pid, role)?;
                let start_identity =
                    super::StoredProcessIdentity::encode(pid, pgid, Some(&start), None);
                record_invocation_started(
                    registry,
                    &InvocationProcessRecord {
                        source: self.source,
                        run_id: self.run_id,
                        process_key: &process_key,
                        pid,
                        pgid,
                        start_identity: &start_identity,
                        command_json: self.command_json,
                        computed_manifest_hash: self.manifest_hash,
                    },
                    self.owner,
                )
            },
            || {
                cancellation.check()?;
                checkpoint()
            },
        );
        let child = match released {
            Ok(child) => child,
            Err(Refusal::Unregistered(error)) => return Err(unregistered(error)),
            Err(Refusal::Registered(failure)) => {
                return Err(self.settle(registry, identity, failure));
            }
        };
        let pgid = pid as i32;
        let outcome = child
            .complete(
                cancellation,
                &mut *checkpoint,
                |transition| match transition {
                    CapturedExecTransition::Observed(outcome) => {
                        let (exit_code, terminal) = (self.terminal)(outcome);
                        record_invocation_observed(
                            registry,
                            identity,
                            terminal.execution_outcome(),
                            exit_code,
                        )
                    }
                    CapturedExecTransition::Terminating(reason) => record_invocation_canceling(
                        registry,
                        identity,
                        &(self.canceling)(pgid, &reason),
                    ),
                },
            )
            .map_err(|failure| self.settle(registry, identity, failure))?;
        Ok((process_key, outcome))
    }

    /// A settled child leaves only terminal evidence; an unsettled child keeps
    /// its obligation and records its capture outcome for recovery.
    fn settle(
        &self,
        registry: &mut Registry,
        identity: InvocationIdentity<'_>,
        failure: CapturedExecFailure,
    ) -> InvocationFailure {
        let error = *failure.error;
        let recorded = if failure.settled {
            let status = failure
                .outcome
                .as_ref()
                .map_or(TaskTerminalStatus::Canceled, |outcome| {
                    (self.terminal)(outcome).1
                });
            let payload =
                serde_json::json!({ "interrupted": true, "code": error.code }).to_string();
            mark_invocation_finished(registry, identity, status, &payload, Some(failure.capture))
        } else {
            record_capture_outcome(
                registry,
                identity.run_id,
                identity.process_key,
                failure.capture,
            )
        };
        InvocationFailure {
            error: Box::new(match recorded {
                Ok(()) => error,
                Err(recording) => error.with_cause(recording),
            }),
            outcome: failure.outcome,
        }
    }
}

/// A launched leader's group and start identity. The gate made it its own
/// group leader before exec; any other group is an escape.
fn leader_identity(pid: u32, role: &str) -> RuntimeResult<(i32, String)> {
    let pgid = pid as i32;
    if get_process_group(pid)? != pgid {
        return Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("{role} launcher process group changed"),
        ));
    }
    let start = platform_start_identity(pid).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("{role} launcher start identity is unavailable"),
        )
    })?;
    Ok((pgid, start))
}

/// Capture is ordered stdout then stderr. Preserve that order when containment
/// owns the primary error, then append any subordinate operation failure.
fn completion_error(
    containment: RuntimeResult<()>,
    capture: RuntimeResult<()>,
    operation: Option<RuntimeError>,
) -> Option<RuntimeError> {
    let mut primary = containment.err();
    if let Err(mut capture) = capture {
        primary = Some(match primary {
            Some(error) => {
                let causes = std::mem::take(&mut capture.causes);
                let mut error = error.with_cause(capture);
                error.causes.extend(*causes);
                error
            }
            None => capture,
        });
    }
    if let Some(operation) = operation {
        primary = Some(match primary {
            Some(error) => error.absorb(operation),
            None => operation,
        });
    }
    primary
}

/// Finish an owned bounded child, including descendants left after direct exit.
/// Reaping is attempted even if group containment fails; it cannot erase that failure.
pub(crate) fn terminate_and_reap(child: &mut OwnedChild, pgid: i32) -> RuntimeResult<()> {
    let containment = contain(&Leader::Child(child), pgid, libc::SIGTERM, 1000, &[]);
    crate::error::both(containment.map(|_| ()), reap_owned_child(child))
}

fn reap_owned_child(child: &mut OwnedChild) -> RuntimeResult<()> {
    let exited = || {
        child
            .observe()
            .map(|status| status.is_some())
            .map_err(|error| RuntimeError::new(ErrorCode::ProcEscape, error.to_string()))
    };
    if poll_until(Duration::from_secs(1), exited)? {
        return Ok(());
    }
    let killed = child.kill();
    if poll_until(Duration::from_secs(1), exited)? {
        return Ok(());
    }
    Err(RuntimeError::new(
        ErrorCode::ProcEscape,
        match killed {
            Ok(()) => "owned child did not exit after containment or direct kill".into(),
            Err(error) => format!("failed to kill owned child after containment: {error}"),
        },
    ))
}

fn endpoint_key(service_instance_id: &str, endpoint_id: &str) -> String {
    format!("{service_instance_id}:{endpoint_id}")
}

struct LifecycleEventContext {
    run_id: Option<String>,
    service_name: String,
    service_instance_id: Option<String>,
    process_key: Option<String>,
    computed_manifest_hash: String,
}

/// A lifecycle operation's identity and terminal semantics for durable event
/// recording, owned so it does not borrow the `ReadyService` across the
/// `&mut self` calls in the lifecycle methods.
struct LifecycleRecord {
    meta: OpMeta,
    class: &'static str,
}

impl LifecycleRecord {
    fn from_meta(meta: &OpMeta, class: &'static str) -> Self {
        Self {
            meta: meta.clone(),
            class,
        }
    }
}

fn record_lifecycle_started(
    registry: &mut Registry,
    context: &LifecycleEventContext,
    record: &LifecycleRecord,
) -> RuntimeResult<()> {
    let payload_json = serde_json::to_string(&serde_json::json!({
        "serviceId": context.service_name,
        "operationId": record.meta.operation_id,
        "class": record.class,
    }))
    .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
    record_service_lifecycle_event(
        registry,
        "service.lifecycle.started",
        context.run_id.as_deref(),
        context.service_instance_id.as_deref(),
        context.process_key.as_deref(),
        &context.computed_manifest_hash,
        &payload_json,
    )
}

fn record_lifecycle_success(
    registry: &mut Registry,
    context: &LifecycleEventContext,
    record: &LifecycleRecord,
) -> RuntimeResult<()> {
    record_lifecycle_terminal(
        registry,
        context,
        record,
        &record.meta.terminal_success,
        None,
    )
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
        &record.meta.terminal_failure,
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
        "serviceId": context.service_name,
        "operationId": record.meta.operation_id,
        "class": record.class,
        "terminalResult": terminal_result,
        "errorCode": error.map(|error| error.code),
        "message": error.map(|error| error.message.as_str()),
    }))
    .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
    record_service_lifecycle_event(
        registry,
        "service.lifecycle.terminal",
        context.run_id.as_deref(),
        context.service_instance_id.as_deref(),
        context.process_key.as_deref(),
        &context.computed_manifest_hash,
        &payload_json,
    )
}

pub(crate) fn get_process_group(pid: u32) -> RuntimeResult<i32> {
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

/// Whether any non-zombie process currently holds `pid`, whatever its identity.
pub(crate) fn process_present(pid: u32) -> RuntimeResult<bool> {
    Ok(process_group(pid)?.is_some() && !process_is_zombie(pid))
}

pub(crate) fn process_is_live_with_start_identity(
    pid: u32,
    platform_start: Option<&str>,
) -> RuntimeResult<bool> {
    Ok(process_group(pid)?.is_some() && identity_alive(pid, platform_start))
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

/// The startup grace period: poll cancellation, every started service, and
/// escape evidence, then require a live, contained leader.
fn hold_startup_grace(
    service: &mut OwnedService,
    cancellation: &CancellationToken,
    session_checkpoint: &dyn Fn() -> RuntimeResult<()>,
) -> RuntimeResult<()> {
    let deadline = Instant::now() + STARTUP_GRACE;
    while Instant::now() < deadline {
        cancellation.check()?;
        session_checkpoint()?;
        if let Some(error) = service.escape_error() {
            return Err(error);
        }
        thread::sleep(
            super::OBSERVATION_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    service.require_contained("readiness")
}

/// The leader of an owned process tree, identified so that a reused PID is
/// never mistaken for it.
pub(crate) enum Leader<'a> {
    /// An owned direct child: its PID stays ours until it is reaped.
    Child(&'a OwnedChild),
    /// A recorded leader. Its start identity distinguishes PID reuse; without
    /// one, presence stands for identity and the PID is never signaled alone.
    Recorded {
        pid: u32,
        platform_start: Option<&'a str>,
    },
}

impl Leader<'_> {
    fn pid(&self) -> u32 {
        match self {
            Self::Child(child) => child.id(),
            Self::Recorded { pid, .. } => *pid,
        }
    }

    /// Still the same live, non-zombie process. An unobservable owned child
    /// counts as live, so containment never claims it gone.
    fn alive(&self) -> bool {
        match self {
            Self::Child(child) => !matches!(child.observe(), Ok(Some(_))),
            Self::Recorded {
                pid,
                platform_start: Some(start),
            } => identity_alive(*pid, Some(start)),
            Self::Recorded {
                pid,
                platform_start: None,
            } => process_present(*pid).unwrap_or(true),
        }
    }

    /// A PID is never reused while its process group exists, so the group is
    /// ours unless the leader PID now names another process.
    fn owns_group(&self) -> bool {
        match self {
            Self::Child(_)
            | Self::Recorded {
                platform_start: None,
                ..
            } => true,
            Self::Recorded {
                pid,
                platform_start: Some(start),
            } => {
                platform_start_identity(*pid).as_deref() == Some(*start)
                    || matches!(process_group(*pid), Ok(None))
            }
        }
    }

    /// Signal the leader itself only when its identity is proven.
    fn kill(&self) {
        match self {
            Self::Child(child) => {
                let _ = child.kill();
            }
            Self::Recorded {
                pid,
                platform_start: Some(_),
            } if self.alive() => unsafe {
                libc::kill(*pid as libc::pid_t, libc::SIGKILL);
            },
            Self::Recorded { .. } => {}
        }
    }
}

/// The one containment primitive. Snapshot the leader's descendants with
/// their start identities, add every tracked identity, signal the leader's
/// group, and wait until the leader, its descendants, its group, and every
/// snapshotted process are gone; then escalate to SIGKILL across all of them.
/// A snapshotted process may reparent and leave the group after the first
/// signal; its identity keeps it tracked and makes the tracking PID-reuse
/// safe. Returns whether escalation was required.
pub(crate) fn contain(
    leader: &Leader<'_>,
    pgid: i32,
    signal: i32,
    timeout_ms: u64,
    tracked: &[TrackedProcessIdentity],
) -> RuntimeResult<bool> {
    let pid = leader.pid();
    let snapshot = tracked_snapshot(leader.alive().then_some(pid), tracked);
    let owns_group = leader.owns_group();
    if owns_group {
        signal_process_group(pgid, signal)?;
    }
    let gone = || tree_gone(leader, pgid, owns_group, &snapshot);
    if poll_until(Duration::from_millis(timeout_ms), gone)? {
        return Ok(false);
    }
    kill_snapshot_survivors(&snapshot);
    if leader.alive() {
        for descendant in descendant_pids(pid).unwrap_or_default() {
            unsafe {
                libc::kill(descendant as libc::pid_t, libc::SIGKILL);
            }
        }
        leader.kill();
    }
    if owns_group {
        signal_process_group(pgid, libc::SIGKILL)?;
    }
    if poll_until(Duration::from_secs(1), gone)? {
        Ok(true)
    } else {
        Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to terminate owned process tree rooted at {pid}"),
        ))
    }
}

fn tree_gone(
    leader: &Leader<'_>,
    pgid: i32,
    owns_group: bool,
    snapshot: &[TrackedProcessIdentity],
) -> RuntimeResult<bool> {
    if leader.alive() {
        return Ok(false);
    }
    // A child that escaped to its own group after reparenting is neither a
    // descendant of the leader nor in its group; only the snapshot sees it.
    if snapshot
        .iter()
        .any(|process| identity_alive(process.pid, process.platform_start.as_deref()))
    {
        return Ok(false);
    }
    Ok(!(owns_group && process_group_has_live_member(pgid)?))
}

/// `true` if `pid` is still the *same* live process — including one that
/// escaped to its own process group. A dead, zombie, pid-reused (start
/// identity no longer matches), or identity-unconfirmed entry is not.
fn identity_alive(pid: u32, identity: Option<&str>) -> bool {
    identity.is_some_and(|identity| {
        !process_is_zombie(pid) && platform_start_identity(pid).as_deref() == Some(identity)
    })
}

/// SIGKILL every snapshotted descendant still running as its original process,
/// and the group it escaped into, so an owned child cannot outlive `stop`/`down`.
fn kill_snapshot_survivors(snapshot: &[TrackedProcessIdentity]) {
    for TrackedProcessIdentity {
        pid,
        platform_start,
    } in snapshot
    {
        if !identity_alive(*pid, platform_start.as_deref()) {
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

/// Poll `done` until it holds or `timeout` passes; it always runs at least
/// once and once more at the deadline.
pub(crate) fn poll_until(
    timeout: Duration,
    mut done: impl FnMut() -> RuntimeResult<bool>,
) -> RuntimeResult<bool> {
    const INTERVAL: Duration = Duration::from_millis(25);
    let deadline = Instant::now() + timeout;
    loop {
        if done()? {
            return Ok(true);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        thread::sleep(remaining.min(INTERVAL));
    }
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
struct DescendantEvidence {
    known: BTreeMap<u32, TrackedProcessIdentity>,
    escaped: BTreeMap<u32, TrackedProcessIdentity>,
}

/// Execution-thread-owned evidence; no independent scanner or liveness owner.
#[derive(Default)]
struct DescendantTracker {
    evidence: RefCell<DescendantEvidence>,
}

impl DescendantTracker {
    fn refresh(
        &self,
        pid: u32,
        expected_pgid: i32,
        strict_process_group: bool,
    ) -> RuntimeResult<()> {
        let descendants = descendant_pids(pid)?;
        let mut evidence = self.evidence.borrow_mut();
        for descendant in descendants {
            let escaped = strict_process_group
                && process_group(descendant)?.is_some_and(|pgid| pgid != expected_pgid);
            let observed = monitored_process(descendant);
            evidence
                .known
                .entry(descendant)
                .or_insert_with(|| observed.clone());
            if escaped {
                evidence.escaped.insert(descendant, observed);
            }
        }
        Ok(())
    }

    fn escaped_descendants(&self) -> Vec<TrackedProcessIdentity> {
        self.evidence.borrow().escaped.values().cloned().collect()
    }

    fn known_descendants(&self) -> Vec<TrackedProcessIdentity> {
        self.evidence.borrow().known.values().cloned().collect()
    }
}

fn monitored_process(pid: u32) -> TrackedProcessIdentity {
    TrackedProcessIdentity {
        pid,
        platform_start: platform_start_identity(pid),
    }
}

/// Every tracked identity plus the live descendants of `root`, if any. A live
/// descendant's current identity replaces a stored one for the same PID.
fn tracked_snapshot(
    root: Option<u32>,
    tracked: &[TrackedProcessIdentity],
) -> Vec<TrackedProcessIdentity> {
    let mut snapshot = tracked
        .iter()
        .cloned()
        .map(|process| (process.pid, process))
        .collect::<BTreeMap<_, _>>();
    let live = match root {
        Some(root) => descendant_pids(root).unwrap_or_default(),
        None => Vec::new(),
    };
    for pid in live {
        let observed = monitored_process(pid);
        if observed.platform_start.is_some() || !snapshot.contains_key(&pid) {
            snapshot.insert(pid, observed);
        }
    }
    snapshot.into_values().collect()
}

pub(crate) fn process_escape_start_identity(
    pid: u32,
    pgid: i32,
    platform_start: Option<&str>,
    existing: &[TrackedProcessIdentity],
) -> String {
    let tracked = tracked_snapshot(Some(pid), existing);
    super::StoredProcessIdentity::encode(pid, pgid, platform_start, Some(&tracked))
}

fn descendant_pids(pid: u32) -> RuntimeResult<Vec<u32>> {
    let mut descendants = Vec::new();
    let mut queue = vec![pid];
    while let Some(parent) = queue.pop() {
        for child in direct_child_pids(parent)? {
            queue.push(child);
            descendants.push(child);
        }
    }
    Ok(descendants)
}

#[cfg(target_os = "linux")]
fn direct_child_pids(parent: u32) -> RuntimeResult<Vec<u32>> {
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
fn direct_child_pids(parent: u32) -> RuntimeResult<Vec<u32>> {
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
pub(crate) fn process_group_has_live_member(pgid: i32) -> RuntimeResult<bool> {
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
pub(crate) fn process_group_has_live_member(pgid: i32) -> RuntimeResult<bool> {
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
    use nixfied_manifest::ServiceId;

    #[test]
    fn capture_completion_error_goldens_preserve_stream_and_outcome_order() {
        for containment_failed in [false, true] {
            let capture = RuntimeError::new(
                ErrorCode::SecretLeakBlocked,
                "captured stdout did not reach EOF before shutdown deadline",
            )
            .with_cause(RuntimeError::new(
                ErrorCode::SecretLeakBlocked,
                "captured stderr did not reach EOF before shutdown deadline",
            ));
            let containment = if containment_failed {
                Err(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    "containment failed",
                ))
            } else {
                Ok(())
            };
            let error = completion_error(
                containment,
                Err(capture),
                Some(RuntimeError::new(
                    ErrorCode::TaskFailed,
                    "task smoke exited with code 7",
                )),
            )
            .unwrap();
            let actual = serde_json::to_value(error).unwrap();
            let expected = if containment_failed {
                serde_json::json!({
                    "code":"PROC_ESCAPE", "exitClass":"error", "message":"containment failed", "details":{}, "manifestPath":null, "computedManifestHash":null,
                    "causes":[
                        {"code":"SECRET_LEAK_BLOCKED", "exitClass":"error", "message":"captured stdout did not reach EOF before shutdown deadline", "details":{}},
                        {"code":"SECRET_LEAK_BLOCKED", "exitClass":"error", "message":"captured stderr did not reach EOF before shutdown deadline", "details":{}},
                        {"code":"TASK_FAILED", "exitClass":"error", "message":"cause: TASK_FAILED", "details":{}}
                    ]
                })
            } else {
                serde_json::json!({
                    "code":"SECRET_LEAK_BLOCKED", "exitClass":"error", "message":"captured stdout did not reach EOF before shutdown deadline", "details":{}, "manifestPath":null, "computedManifestHash":null,
                    "causes":[
                        {"code":"SECRET_LEAK_BLOCKED", "exitClass":"error", "message":"captured stderr did not reach EOF before shutdown deadline", "details":{}},
                        {"code":"TASK_FAILED", "exitClass":"error", "message":"cause: TASK_FAILED", "details":{}}
                    ]
                })
            };
            assert_eq!(actual, expected);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_failure_preserves_observed_exit_or_interrupts_live_child() {
        for exited in [false, true] {
            let root = crate::test_support::TestDir::new("capture-failure");
            let authority = crate::state::ownership::fixture_guard(
                &root,
                &crate::registry::RegistryIdentity::default_slot("test", "abi", "toolchain"),
            );
            let child = spawn_gated_captured_exec(
                &CapturedExec {
                    authority: &authority,
                    executable: "/bin/sh",
                    args: &[
                        "-c".into(),
                        if exited {
                            "printf x; exit 0"
                        } else {
                            "printf x; /bin/sleep 30"
                        }
                        .into(),
                    ],
                    env: &BTreeMap::new(),
                    cwd: &root,
                    stdin: StdinPolicy::Null,
                    timeout: None,
                    stdout_path: Path::new("/dev/full"),
                    stderr_path: &root.join("stderr"),
                    redactor: &Redactor::empty(),
                    log_file_mode: LogFileMode::Replace,
                    label: "capture-failure",
                },
                &crate::launch::test_launcher(),
            )
            .unwrap()
            .register_and_release(|_| Ok(()), || Ok(()))
            .unwrap_or_else(|_| panic!("the captured child should be released"));
            let pid = child.pid();
            if exited {
                let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
                assert_eq!(
                    unsafe {
                        libc::waitid(libc::P_PID, pid, &mut status, libc::WEXITED | libc::WNOWAIT)
                    },
                    0
                );
            }
            let started = Instant::now();
            let result = child.complete(&CancellationToken::new(), || Ok(()), |_| Ok(()));
            let failure = match result {
                Err(failure) => failure,
                Ok(_) => panic!("failed capture must stop execution"),
            };
            assert_eq!(failure.error.code, ErrorCode::SecretLeakBlocked);
            assert!(failure.error.message.contains("failed to write"));
            match (exited, failure.outcome) {
                (false, None) => {}
                (true, Some(CapturedExecOutcome::Exited(status))) => assert!(status.success()),
                _ => panic!("capture failure must preserve an already observed exit"),
            }
            assert!(started.elapsed() < Duration::from_secs(3));
            assert!(!process_group_has_live_member(pid as i32).unwrap());
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
                -1
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ECHILD)
            );
            drop(authority);
        }
    }

    #[test]
    fn bounded_exec_contains_pipe_holding_descendants_before_relay_join() {
        let root = crate::test_support::TestDir::new("bounded");
        let authority = crate::state::ownership::fixture_guard(
            &root,
            &crate::registry::RegistryIdentity::default_slot("test", "abi", "toolchain"),
        );
        let marker = root.join("survived");
        let stdout = root.join("stdout");
        let stderr = root.join("stderr");
        let secrets =
            ResolvedSecrets::from_values(BTreeMap::from([("token".into(), "secret".into())]));
        let redactor = Redactor::from_secrets(&secrets);
        let outcome = spawn_gated_captured_exec(
            &CapturedExec {
                authority: &authority,
                executable: "/bin/sh",
                args: &[
                    "-c".into(),
                    "(/bin/sleep 2; printf survived > \"$1\") & printf secret; exit 0".into(),
                    "probe".into(),
                    marker.to_string_lossy().into_owned(),
                ],
                env: &BTreeMap::new(),
                cwd: &root,
                stdin: StdinPolicy::Null,
                timeout: Some(Duration::from_secs(5)),
                stdout_path: &stdout,
                stderr_path: &stderr,
                redactor: &redactor,
                log_file_mode: LogFileMode::Replace,
                label: "pipe-holder",
            },
            &crate::launch::test_launcher(),
        )
        .unwrap()
        .register_and_release(|_| Ok(()), || Ok(()))
        .unwrap_or_else(|_| panic!("the captured child should be released"))
        .complete(&CancellationToken::new(), || Ok(()), |_| Ok(()))
        .map_err(|failure| *failure.error)
        .unwrap();
        let survived = marker.exists();
        let captured = std::fs::read_to_string(&stdout).unwrap();
        assert!(matches!(outcome, CapturedExecOutcome::Exited(status) if status.success()));
        assert!(
            !survived,
            "relay completion must follow descendant containment, not natural expiry"
        );
        assert_eq!(captured, crate::redaction::REDACTION_TOKEN);
    }

    #[test]
    fn bounded_spawn_failure_and_refused_release_close_capture_and_reap() {
        let root = crate::test_support::TestDir::new("bounded-abort");
        let authority = crate::state::ownership::fixture_guard(
            &root,
            &crate::registry::RegistryIdentity::default_slot("test", "abi", "toolchain"),
        );
        let executable = std::env::var("NIXFIED_TEST_SLEEP").unwrap();
        let missing = root.join("missing-program");
        let stdout = root.join("stdout");
        let stderr = root.join("stderr");
        let mut spec = CapturedExec {
            authority: &authority,
            executable: missing.to_str().unwrap(),
            args: &["30".into()],
            env: &BTreeMap::new(),
            cwd: &root,
            stdin: StdinPolicy::Null,
            timeout: Some(Duration::from_secs(30)),
            stdout_path: &stdout,
            stderr_path: &stderr,
            redactor: &Redactor::empty(),
            log_file_mode: LogFileMode::Replace,
            label: "task process",
        };
        let Some(Refusal::Registered(failure)) =
            spawn_gated_captured_exec(&spec, &crate::launch::test_launcher())
                .unwrap()
                .register_and_release(|_| Ok(()), || Ok(()))
                .err()
        else {
            panic!("an exec failure follows committed registration");
        };
        assert_eq!(failure.error.code, ErrorCode::ProcEscape);
        assert!(std::fs::read(&stdout).unwrap().is_empty());
        assert!(std::fs::read(&stderr).unwrap().is_empty());
        spec.executable = &executable;
        let pending = spawn_gated_captured_exec(&spec, &crate::launch::test_launcher()).unwrap();
        let pid = pending.pid() as i32;
        let error = pending
            .register_and_release(
                |_| {
                    Err(RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        "process recording denied",
                    ))
                },
                || Ok(()),
            )
            .err()
            .unwrap();
        let Refusal::Unregistered(error) = error else {
            panic!("a refused registration leaves no process row to settle");
        };
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        assert_eq!(error.message, "process recording denied");
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "unrecorded child must be reaped"
        );
        assert!(std::fs::read(&stdout).unwrap().is_empty());
        assert!(std::fs::read(&stderr).unwrap().is_empty());
        let pending = spawn_gated_captured_exec(&spec, &crate::launch::test_launcher()).unwrap();
        let pid = pending.pid() as i32;
        let mut checkpoints = 0;
        let failure = pending
            .register_and_release(
                |_| Ok(()),
                || {
                    checkpoints += 1;
                    if checkpoints == 1 {
                        return Ok(());
                    }
                    Err(RuntimeError::new(ErrorCode::Canceled, "session canceled"))
                },
            )
            .err()
            .unwrap();
        let Refusal::Registered(failure) = failure else {
            panic!("a refusal after committed registration must return settlement facts");
        };
        assert_eq!(failure.error.code, ErrorCode::Canceled);
        assert!(failure.outcome.is_none());
        assert!(
            failure.settled,
            "a contained unreleased child settles its row"
        );
        assert_eq!(failure.capture, crate::redaction::CaptureOutcome::Complete);
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "registered but unreleased child must be reaped"
        );
    }

    #[test]
    fn conflict_view_preserves_wire_host_owner_omission_and_native_message() {
        let selected = SelectedEndpoint {
            endpoint_id: "web".into(),
            host: LoopbackHost::parse("::1").unwrap(),
            port: 23080,
        };
        let error = port_conflict_error(
            PortConflictReason::ListenerOccupied,
            "project",
            &selected,
            None,
        );
        assert_eq!(
            error.message,
            "endpoint web is unavailable at ::1:23080 (listener-occupied)"
        );
        assert_eq!(
            error.details,
            serde_json::json!({"portConflict":{
                "reason":"listener-occupied","projectId":"project",
                "endpoint":{"transport":"tcp","family":"ipv6","address":"::1","port":23080,"endpointId":"web"}
            }})
        );
        let owner = NixfiedOwner {
            project_id: "owner".into(),
            environment: "dev".into(),
            slot: 2,
            run_id: "run".into(),
            service_id: "service".into(),
            service_instance_id: "instance".into(),
            process_key: "process".into(),
        };
        let error = port_conflict_error(
            PortConflictReason::StartupLockContended,
            "project",
            &selected,
            Some(&owner),
        );
        assert_eq!(
            error.details["portConflict"]["reason"],
            "startup-lock-contended"
        );
        assert_eq!(
            error.details["portConflict"]["nixfiedOwner"],
            serde_json::json!({
                "projectId":"owner","environment":"dev","slot":2,"runId":"run",
                "serviceId":"service","serviceInstanceId":"instance","processKey":"process"
            })
        );
    }

    fn endpoint(host: &str, port: u16) -> SelectedEndpoint {
        SelectedEndpoint {
            endpoint_id: format!("{host}:{port}"),
            host: LoopbackHost::parse(host).unwrap(),
            port,
        }
    }

    fn checked(
        text: &str,
        substitution: &ExecSubstitution<'_>,
        secrets: &[&str],
        env: bool,
    ) -> RuntimeResult<Template> {
        let declared = secrets.iter().map(|id| (id.to_string(), serde_json::from_value(serde_json::json!({"secretId":id,"source":{"kind":"env-var","envVar":"TEST_ONLY"}})).unwrap())).collect();
        Template::parse(
            text,
            &crate::template::Scope {
                owner: crate::template::Owner::Service("fixture"),
                has_primary: substitution.own_primary.is_some(),
                own_endpoints: substitution
                    .own_endpoints
                    .keys()
                    .map(String::as_str)
                    .collect(),
                services: substitution.named.keys().map(|id| id.as_str()).collect(),
            },
            &declared,
            env,
        )
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
            .value(&checked("--listen ${host}:${port} --db ${host:postgres}:${port:postgres} --data ${stateDir}", &substitution, &[], false).unwrap())
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
            .value(
                &checked(
                    "--http ${port} --ws ${port:ws} --auth ${port:authrpc}",
                    &substitution,
                    &[],
                    false,
                )
                .unwrap(),
            )
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
        let env = env
            .into_iter()
            .map(|(key, value)| (key, checked(&value, &substitution, &[], true).unwrap()))
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

        let env = env
            .into_iter()
            .map(|(key, value)| {
                (
                    key,
                    checked(&value, &substitution, &["api-token"], true).unwrap(),
                )
            })
            .collect();
        let env = substitution.env(&env).expect("secret env substitutes");
        assert_eq!(env["TOKEN"], "bearer:secret-value");
        assert_eq!(
            checked(
                "--token=${secret:api-token}",
                &substitution,
                &["api-token"],
                false
            )
            .unwrap_err()
            .code,
            ErrorCode::ManifestAdmission
        );
    }

    #[test]
    fn undeclared_named_reference_cannot_construct_a_template() {
        let substitution = ExecSubstitution {
            own_primary: None,
            own_endpoints: &BTreeMap::new(),
            named: &SlotEndpoints::new(),
            state_root: Path::new("/state"),
            secrets: &ResolvedSecrets::empty(),
        };
        let error = checked("--db ${port:ghost}", &substitution, &[], false)
            .expect_err("an undeclared named placeholder must not leak to the child");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
    }

    #[test]
    fn inserted_values_are_opaque_and_unknown_child_syntax_keeps_nested_references() {
        let own = endpoint("127.0.0.1", 23080);
        let secrets = ResolvedSecrets::from_values(BTreeMap::from([
            ("a".into(), "${secret:b}/${port}".into()),
            ("b".into(), "actual-b".into()),
        ]));
        let substitution = ExecSubstitution {
            own_primary: Some(&own),
            own_endpoints: &BTreeMap::new(),
            named: &SlotEndpoints::new(),
            state_root: Path::new("/state/${port}/${secret:b}"),
            secrets: &secrets,
        };
        for (authored, expected) in [
            ("${secret:a}|${secret:b}", "${secret:b}/${port}|actual-b"),
            ("${secret:b}|${secret:a}", "actual-b|${secret:b}/${port}"),
            ("${stateDir}|${port}", "/state/${port}/${secret:b}|23080"),
            (
                "${HOME:-${port}}:${portfoo}:${HOME",
                "${HOME:-23080}:${portfoo}:${HOME",
            ),
        ] {
            let template = checked(authored, &substitution, &["a", "b"], true).unwrap();
            assert_eq!(substitution.value(&template).unwrap(), expected);
        }
    }
}
