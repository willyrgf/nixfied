//! The executor's input types. An `ExecutionModel` is a resolved, fully-typed
//! view of a `nixfied_model::Model` containing only what the runtime can
//! execute, shaped so unsupported model states are unrepresentable here. It is
//! produced by [`crate::execution::lower`]; the executor consumes it and never
//! reads `Model` directly.

use std::collections::BTreeMap;
use std::time::Duration;

use nixfied_model::{
    ContainmentRequirement, NodeId, OperationId, ServiceId, ServiceIdentity, TaskId,
};
pub use nixfied_model::{LoopbackHost, StdinPolicy};

/// The whole executable program for a run: services to start, tasks to run, the
/// environment and workflow plans, and the per-slot port windows the planner
/// assigns from.
#[derive(Debug, Clone)]
pub struct ExecutionModel {
    pub services: BTreeMap<ServiceId, ExecService>,
    pub tasks: BTreeMap<TaskId, ExecTask>,
    pub environment: ExecEnvironment,
    pub workflows: BTreeMap<String, ExecWorkflow>,
    pub slot_windows: BTreeMap<u32, PortWindow>,
}

/// The single environment's start order: services first, then tasks. Every id is
/// a handle the lowering minted by resolving the reference against the declared
/// services/tasks, so the executor's `services.get(id)` cannot miss.
#[derive(Debug, Clone)]
pub struct ExecEnvironment {
    pub services: Vec<ServiceId>,
    pub tasks: Vec<TaskId>,
}

/// A bounded acyclic graph of task nodes over a set of required services.
#[derive(Debug, Clone)]
pub struct ExecWorkflow {
    pub services_required: Vec<ServiceId>,
    pub nodes: Vec<ExecWorkflowNode>,
}

#[derive(Debug, Clone)]
pub struct ExecWorkflowNode {
    pub node_id: NodeId,
    pub task_id: TaskId,
    pub depends_on: Vec<NodeId>,
}

/// A slot's candidate port window, the range the planner assigns service ports
/// from.
#[derive(Debug, Clone, Copy)]
pub struct PortWindow {
    pub start: u16,
    pub end: u16,
}

impl PortWindow {
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
    pub prepare: PrepareOp,
    pub start: StartOp,
    pub ready: ReadyOp,
    pub health: HealthOp,
    pub stop: StopOp,
    pub clean: CleanOp,
    /// The single endpoint bound by the readiness probe.
    pub endpoint: ResolvedEndpoint,
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

/// Prepare is always a recorded lifecycle class; its exec is optional (e.g. a
/// data-dir init like initdb, or nothing).
#[derive(Debug, Clone)]
pub struct PrepareOp {
    pub meta: OpMeta,
    pub exec: Option<ResolvedExec>,
}

#[derive(Debug, Clone)]
pub struct StartOp {
    pub meta: OpMeta,
    pub exec: ResolvedExec,
}

#[derive(Debug, Clone)]
pub struct ReadyOp {
    pub meta: OpMeta,
    pub probe: TcpProbe,
}

#[derive(Debug, Clone)]
pub struct HealthOp {
    pub meta: OpMeta,
    pub probe: TcpProbe,
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

/// A resolved exec: its closure executable, the combined (base + operation/task)
/// argument template, environment, confined relative working directory, and
/// timeout. `${port}`/`${stateDir}`/`${host}` are substituted at run time.
#[derive(Debug, Clone)]
pub struct ResolvedExec {
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub stdin: StdinPolicy,
    pub timeout: Duration,
}

/// A tcp-connect probe of the service's single bound endpoint; `label` only names
/// the op (ready/health) in diagnostics. No http target, no cross-endpoint ref.
#[derive(Debug, Clone)]
pub struct TcpProbe {
    pub label: String,
    pub timeout: Duration,
    pub retry_interval: Duration,
    pub max_attempts: u32,
}

/// The endpoint the runtime binds and verifies ownership of. The port is assigned
/// by the planner from the slot window, so it is not part of the endpoint.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint_id: String,
    pub host: LoopbackHost,
}

/// A stop signal the runtime can send. Replaces the free-form `stopPolicy.signal`
/// string with the closed set the runtime honors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSignal {
    Term,
    Int,
    Quit,
    Hup,
}

impl StopSignal {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "TERM" => Some(Self::Term),
            "INT" => Some(Self::Int),
            "QUIT" => Some(Self::Quit),
            "HUP" => Some(Self::Hup),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Term => "TERM",
            Self::Int => "INT",
            Self::Quit => "QUIT",
            Self::Hup => "HUP",
        }
    }

    pub fn libc(&self) -> i32 {
        match self {
            Self::Term => libc::SIGTERM,
            Self::Int => libc::SIGINT,
            Self::Quit => libc::SIGQUIT,
            Self::Hup => libc::SIGHUP,
        }
    }
}

impl From<nixfied_model::StopSignal> for StopSignal {
    fn from(signal: nixfied_model::StopSignal) -> Self {
        match signal {
            nixfied_model::StopSignal::Term => Self::Term,
            nixfied_model::StopSignal::Int => Self::Int,
            nixfied_model::StopSignal::Quit => Self::Quit,
            nixfied_model::StopSignal::Hup => Self::Hup,
        }
    }
}

/// A resolved task: the combined exec/task argument template, the services it
/// gates on, and its success codes.
#[derive(Debug, Clone)]
pub struct ExecTask {
    pub task_id: TaskId,
    pub exec: ResolvedExec,
    pub depends_on_services_ready: Vec<ServiceId>,
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
    fn stop_signal_round_trips_known_names() {
        for name in ["TERM", "INT", "QUIT", "HUP"] {
            let signal = StopSignal::from_name(name).expect("known signal parses");
            assert_eq!(signal.name(), name);
        }
        assert!(StopSignal::from_name("KILL").is_none());
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
