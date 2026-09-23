//! The executor's input types. An `ExecutionManifest` is a resolved, fully-typed
//! view of a `nixfied_manifest::Manifest` containing only what the runtime can
//! execute, shaped so unsupported manifest states are unrepresentable here. It is
//! produced by [`crate::execution::lower`]; the executor consumes it and never
//! reads `Manifest` directly.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::time::Duration;

use nixfied_manifest::{ContainmentRequirement, OperationId, ServiceId, ServiceLifetime, TaskId};
pub use nixfied_manifest::{LoopbackHost, StdinPolicy, StopSignal};

/// A service's reuse identity, computed by the lowering from the service's actual
/// contract — never supplied by the manifest. The four components hash the endpoint,
/// the state policy, the behavioral runtime contract (lifecycle/wiring/execs), and
/// the build target; the runtime folds them with the slot address into the
/// `service_instance_id` registry key, so the reuse boundary is a pure function of
/// the contract the runtime executes, not of values the manifest carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub endpoint_identity_hash: String,
    pub state_identity_hash: String,
    pub runtime_compatibility_hash: String,
    pub target_identity_hash: String,
}

/// A checked executable program. Only lowering constructs it; consumers receive
/// shared references so graph validity cannot be invalidated after admission.
#[derive(Debug, Clone)]
pub struct ExecutionManifest {
    pub(super) services: BTreeMap<ServiceId, ExecService>,
    pub(super) tasks: BTreeMap<TaskId, ExecutableTask>,
    pub(super) slot_windows: BTreeMap<u32, PortWindow>,
}

impl ExecutionManifest {
    pub fn services(&self) -> &BTreeMap<ServiceId, ExecService> {
        &self.services
    }

    pub fn tasks(&self) -> &BTreeMap<TaskId, ExecutableTask> {
        &self.tasks
    }

    pub fn leaf(&self, task: &str) -> Option<&ExecTask> {
        match self.tasks.get(task)? {
            ExecutableTask::Leaf(leaf) => Some(leaf),
            ExecutableTask::Composite(_) => None,
        }
    }

    /// Retain the established leaf-then-composite diagnostic/proof order.
    pub fn task_ids(&self) -> impl Iterator<Item = &TaskId> {
        self.tasks
            .iter()
            .filter_map(|(id, task)| matches!(task, ExecutableTask::Leaf(_)).then_some(id))
            .chain(self.tasks.iter().filter_map(|(id, task)| {
                matches!(task, ExecutableTask::Composite(_)).then_some(id)
            }))
    }
}

#[derive(Debug, Clone)]
pub enum ExecutableTask {
    Leaf(ExecTask),
    Composite(ExecComposite),
}

/// A composite task: a static named-step DAG over task references (leaf or
/// composite). Steps are kept in canonical (byte) order of their names — the
/// spec's emission order.
#[derive(Debug, Clone)]
pub struct ExecComposite {
    pub task_id: TaskId,
    pub service_lifetime: ServiceLifetime,
    pub steps: Vec<ExecStep>,
}

#[derive(Debug, Clone)]
pub struct ExecStep {
    pub name: String,
    pub task: TaskId,
    pub depends_on: Vec<String>,
}

/// A slot's candidate port window, the range the planner assigns service ports
/// from.
#[derive(Debug, Clone, Copy)]
pub struct PortWindow {
    start: u16,
    end: u16,
}

impl PortWindow {
    pub(super) fn new(start: u16, end: u16) -> Option<Self> {
        (start > 0 && start <= end).then_some(Self { start, end })
    }

    pub(super) fn start(self) -> u16 {
        self.start
    }

    pub(super) fn end(self) -> u16 {
        self.end
    }

    /// Number of distinct ports the window can host.
    pub fn capacity(&self) -> u32 {
        u32::from(self.end - self.start) + 1
    }
}

/// A foreground service with each lifecycle class resolved to its own shape: an
/// exec for prepare/start, a tcp probe for ready/health, a signal for stop, and
/// nothing for clean. There is no lifecycle union, so an illegal binding (a stop
/// exec, an http probe, an exec on ready) cannot be expressed.
#[derive(Debug, Clone)]
pub struct ExecService {
    pub name: ServiceId,
    /// The optional prepare **task reference** (full task semantics): its
    /// flattened nodes run inside the service's reservation, before start.
    pub prepare: Option<TaskId>,
    pub start: StartOp,
    pub ready: ReadyOp,
    pub health: HealthOp,
    pub stop: StopOp,
    pub clean: CleanOp,
    /// The loopback endpoints the service binds, keyed by endpointId. Each is
    /// assigned a port from the service's contiguous slot block, reserved, and
    /// ownership-verified.
    pub endpoints: BTreeMap<String, ResolvedEndpoint>,
    /// The endpoint bare `${port}`/`${host}` and the tcp readiness/health probe
    /// resolve to, and the one a `connectsTo` dependent reaches by service id. A
    /// key in `endpoints`; `None` for an endpoint-less service.
    pub primary_endpoint: Option<String>,
    /// Same-slot services this service connects to; gates named endpoint
    /// placeholder resolution and orders service startup.
    pub connects_to: Vec<ServiceId>,
    pub containment: ContainmentRequirement,
    pub identity: ServiceIdentity,
}

/// Operation identity carried for durable lifecycle-event recording.
#[derive(Debug, Clone)]
pub struct OpMeta {
    pub operation_id: OperationId,
    pub terminal_success: String,
    pub terminal_failure: String,
}

#[derive(Debug, Clone)]
pub struct StartOp {
    pub meta: OpMeta,
    pub exec: ResolvedInvocation,
}

#[derive(Debug, Clone)]
pub struct ReadyOp {
    pub meta: OpMeta,
    pub probe: Probe,
}

#[derive(Debug, Clone)]
pub struct HealthOp {
    pub meta: OpMeta,
    pub probe: Probe,
}

/// The closed set of probe mechanisms the executor honors. The wire shape is a
/// kind-discriminated struct; the lowering proves coherence and produces this
/// enum, so an exec-less exec probe (or a tcp probe carrying an exec) is
/// unrepresentable past admission.
#[derive(Debug, Clone)]
pub enum Probe {
    Tcp(TcpProbe),
    Exec(ExecProbe),
}

/// A short-lived bound command probe (e.g. `pg_isready`): success is exit 0.
/// `timeout` is the per-attempt kill-after deadline — the exec spec's own
/// timeout does not apply to probe attempts.
#[derive(Debug, Clone)]
pub struct ExecProbe {
    pub label: String,
    pub exec: ResolvedInvocation,
    pub timeout: Duration,
    pub retry_interval: Duration,
    pub max_attempts: NonZeroU32,
}

#[derive(Debug, Clone)]
pub struct StopOp {
    pub meta: OpMeta,
    pub signal: StopSignal,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct CleanOp {
    pub meta: OpMeta,
}

/// A resolved invocation: the eval-resolved executable, the argv tail
/// (`run[1..]`), environment, confined relative working directory, timeout,
/// and the tool PATH roots (each tool executable's parent directory, in
/// declared order) the runtime assembles the child PATH from.
/// `${port}`/`${stateDir}`/`${host}` are substituted at run time.
#[derive(Debug, Clone)]
pub struct ResolvedInvocation {
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub stdin: StdinPolicy,
    pub timeout: Duration,
    pub tool_roots: Vec<String>,
}

impl ResolvedInvocation {
    /// The child PATH: the tool roots joined in declared order. PATH is
    /// runtime-owned — a declared `env.PATH` is rejected at lowering — so the
    /// child's search path is exactly the declared tool set.
    pub fn path_value(&self) -> String {
        self.tool_roots.join(":")
    }

    /// The given (already substituted) declared env with the runtime-owned
    /// PATH inserted.
    pub fn env_with_path(&self, mut env: BTreeMap<String, String>) -> BTreeMap<String, String> {
        env.insert("PATH".to_string(), self.path_value());
        env
    }
}

/// A tcp-connect probe of the service's single bound endpoint; `label` only names
/// the op (ready/health) in diagnostics. No http target, no cross-endpoint ref.
#[derive(Debug, Clone)]
pub struct TcpProbe {
    pub label: String,
    pub timeout: Duration,
    pub retry_interval: Duration,
    pub max_attempts: NonZeroU32,
}

/// The endpoint the runtime binds and verifies ownership of. The port is assigned
/// by the planner from the slot window, so it is not part of the endpoint.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint_id: String,
    pub host: LoopbackHost,
}

/// A resolved leaf task: its invocation, the services it requires ready while
/// it runs, and its success codes.
#[derive(Debug, Clone)]
pub struct ExecTask {
    pub task_id: TaskId,
    pub service_lifetime: ServiceLifetime,
    pub exec: ResolvedInvocation,
    pub requires: Vec<ServiceId>,
    pub success_codes: Vec<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_host_accepts_ipv4_loopback() {
        let host = LoopbackHost::parse("127.0.0.1").expect("ipv4 loopback parses");
        assert_eq!(host.to_string(), "127.0.0.1");
        assert!(host.ip().is_loopback());
    }

    #[test]
    fn loopback_host_accepts_ipv6_loopback() {
        assert!(LoopbackHost::parse("::1").is_ok());
    }

    #[test]
    fn loopback_host_rejects_hostname() {
        assert!(LoopbackHost::parse("localhost").is_err());
    }

    #[test]
    fn loopback_host_rejects_non_loopback_ip() {
        assert!(LoopbackHost::parse("0.0.0.0").is_err());
        assert!(LoopbackHost::parse("10.0.0.1").is_err());
    }

    #[test]
    fn port_window_capacity_is_inclusive() {
        assert_eq!(
            PortWindow {
                start: 100,
                end: 100
            }
            .capacity(),
            1
        );
        assert_eq!(
            PortWindow {
                start: 100,
                end: 109
            }
            .capacity(),
            10
        );
    }
}
