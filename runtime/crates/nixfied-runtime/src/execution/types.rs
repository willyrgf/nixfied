//! The executor's input types. An `ExecutionManifest` is a resolved, fully-typed
//! view of a `nixfied_manifest::Manifest` containing only what the runtime can
//! execute, shaped so unsupported manifest states are unrepresentable here. It is
//! produced by [`crate::execution::lower`]; the executor consumes it and never
//! reads `Manifest` directly.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::time::Duration;

pub use crate::template::Template;
use nixfied_manifest::{ContainmentRequirement, OperationId, ServiceId, TaskId};
pub use nixfied_manifest::{LoopbackHost, StdinPolicy, StopSignal};

/// A checked executable program. Only lowering constructs it; consumers receive
/// shared references so graph validity cannot be invalidated after admission.
#[derive(Debug, Clone)]
pub struct ExecutionManifest {
    pub(super) program: Program,
}

/// Candidate storage is moved, not copied, into the admitted program after proof.
#[derive(Debug, Clone)]
pub(super) struct Program {
    pub services: BTreeMap<ServiceId, ExecService>,
    pub tasks: BTreeMap<TaskId, ExecutableTask>,
    pub slot_windows: BTreeMap<u32, PortWindow>,
}

impl ExecutionManifest {
    pub fn services(&self) -> &BTreeMap<ServiceId, ExecService> {
        &self.program.services
    }
    pub fn tasks(&self) -> &BTreeMap<TaskId, ExecutableTask> {
        &self.program.tasks
    }
    pub fn leaf(&self, task: &str) -> Option<&ExecTask> {
        self.program.leaf(task)
    }
    pub fn task_ids(&self) -> impl Iterator<Item = &TaskId> {
        self.program.task_ids()
    }
}

impl Program {
    pub fn leaf(&self, task: &str) -> Option<&ExecTask> {
        match self.tasks.get(task)? {
            ExecutableTask::Leaf(leaf) => Some(leaf),
            ExecutableTask::Composite(_) => None,
        }
    }
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
    pub addressing: ServiceAddressing,
    /// Same-slot services this service connects to; gates named endpoint
    /// placeholder resolution and orders service startup.
    pub connects_to: Vec<ServiceId>,
    pub containment: ContainmentRequirement,
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
    pub policy: ProbePolicy,
}

#[derive(Debug, Clone)]
pub struct HealthOp {
    pub meta: OpMeta,
    pub policy: ProbePolicy,
}

/// Admission constructs exactly one addressing alternative. Probes cannot become
/// detached from their endpoint, and an endpoint set always has a primary member.
#[derive(Debug, Clone)]
pub enum ServiceAddressing {
    Endpointless {
        ready: Box<ResolvedInvocation>,
        health: Box<ResolvedInvocation>,
    },
    Endpoints(EndpointSet),
}

#[derive(Debug, Clone)]
pub struct EndpointSet {
    primary: EndpointId,
    entries: BTreeMap<EndpointId, EndpointPlan>,
}

impl EndpointSet {
    pub(super) fn new(
        primary: EndpointId,
        entries: BTreeMap<EndpointId, EndpointPlan>,
    ) -> Option<Self> {
        entries
            .contains_key(&primary)
            .then_some(Self { primary, entries })
    }

    pub fn primary(&self) -> &EndpointId {
        &self.primary
    }
    pub fn entries(&self) -> &BTreeMap<EndpointId, EndpointPlan> {
        &self.entries
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EndpointId(String);

impl EndpointId {
    pub(super) fn parse(value: &str) -> Option<Self> {
        let mut chars = value.chars();
        (chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
        .then(|| Self(value.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for EndpointId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl std::fmt::Display for EndpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct EndpointPlan {
    pub host: LoopbackHost,
    pub ready: ResolvedInvocation,
    pub health: ResolvedInvocation,
}

impl ExecService {
    pub fn endpoints(&self) -> impl Iterator<Item = (&EndpointId, &EndpointPlan)> {
        match &self.addressing {
            ServiceAddressing::Endpointless { .. } => None,
            ServiceAddressing::Endpoints(set) => Some(set.entries.iter()),
        }
        .into_iter()
        .flatten()
    }
    pub fn primary_endpoint(&self) -> Option<&EndpointId> {
        match &self.addressing {
            ServiceAddressing::Endpointless { .. } => None,
            ServiceAddressing::Endpoints(set) => Some(set.primary()),
        }
    }
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
/// (`run[1..]`), environment, confined relative working directory,
/// and the tool PATH roots (each tool executable's parent directory, in
/// declared order) the runtime assembles the child PATH from.
/// `${port}`/`${stateDir}`/`${host}` are substituted at run time.
#[derive(Debug, Clone)]
pub struct ResolvedInvocation {
    pub executable: String,
    pub args: Vec<Template>,
    pub env: BTreeMap<String, Template>,
    pub cwd: RelativeCwd,
    pub stdin: StdinPolicy,
    pub tool_roots: Vec<String>,
}

/// Lexically confined at lowering; filesystem confinement is rechecked at every spawn.
#[derive(Debug, Clone)]
pub struct RelativeCwd(String);

impl RelativeCwd {
    pub(super) fn new(value: &str) -> Option<Self> {
        use std::path::{Component, Path};
        (!value.is_empty()
            && !value.contains('\0')
            && !Path::new(value).components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            }))
        .then(|| Self(value.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
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

/// Per-invocation deadline and the phase-owned whole-service retry schedule.
#[derive(Debug, Clone)]
pub struct ProbePolicy {
    pub label: String,
    pub timeout: Duration,
    pub retry_interval: Duration,
    pub max_attempts: NonZeroU32,
}

/// A resolved leaf task: its invocation, the services it requires ready while
/// it runs, and its success codes.
#[derive(Debug, Clone)]
pub struct ExecTask {
    pub timeout: Option<Duration>,
    pub task_id: TaskId,
    pub exec: ResolvedInvocation,
    pub requires: Vec<ServiceId>,
    pub success_codes: Vec<i32>,
}
