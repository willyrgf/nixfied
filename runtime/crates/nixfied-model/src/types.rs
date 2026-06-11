use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

use serde::{Deserialize, Serialize};

use crate::ids::{ClosureId, CodebaseId, ExecId, NodeId, OperationId, ServiceId, TaskId};
use crate::unique_vec::UniqueVec;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Model {
    pub model_version: u32,
    pub toolchain_id: String,
    pub runtime_abi: String,
    pub generator: Generator,
    pub project: Project,
    pub target: Target,
    pub codebases: Vec<Codebase>,
    pub environments: BTreeMap<String, Environment>,
    pub slot_policy: SlotPolicy,
    pub placement: Placement,
    pub state: StatePolicy,
    pub closures: BTreeMap<String, ClosureSpec>,
    pub execs: BTreeMap<String, ExecSpec>,
    pub services: BTreeMap<String, ServiceSpec>,
    pub tasks: BTreeMap<String, TaskSpec>,
    pub workflows: BTreeMap<String, WorkflowSpec>,
    pub docs: Docs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Generator {
    pub name: String,
    pub version: String,
    pub emitter: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub project_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub system: String,
    pub os: String,
    pub arch: String,
    pub closure_system: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Codebase {
    pub codebase_id: CodebaseId,
    pub logical_root: String,
    pub source_mode: SourceMode,
    pub source_identity: String,
    pub source_policy: SourcePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceMode {
    Snapshot,
    FlakeInput,
    LiveWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePolicy {
    pub dirty_policy: DirtyPolicy,
    pub admission_fingerprint_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirtyPolicy {
    Allow,
    Warn,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Environment {
    pub services: UniqueVec<ServiceId>,
    pub tasks: UniqueVec<TaskId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotPolicy {
    pub min: u32,
    pub default: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placement {
    pub slot_placements: BTreeMap<String, SlotPlacement>,
}

/// The per-slot candidate port window. Directory layout templates were pinned
/// constants the runtime owns, so they live there, not in the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotPlacement {
    pub slot: u32,
    pub candidate_ports: CandidatePortWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidatePortWindow {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatePolicy {
    pub marker_identity: String,
    pub state_epoch: String,
    pub cleanup_policy: CleanupPolicy,
    pub persistence: PersistencePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupPolicy {
    DeleteOnClean,
    Protected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersistencePolicy {
    RunScoped,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosureSpec {
    pub kind: ClosureKind,
    pub store_path: String,
    pub executable: String,
    pub target_system: String,
    pub operation_bindings: UniqueVec<OperationId>,
    pub requires_executable: bool,
    pub effects: Vec<ClosureEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClosureKind {
    Executable,
    Helper,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClosureEffect {
    Process,
    NetworkListener,
    SourceRead,
    FileWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecSpec {
    pub closure_id: ClosureId,
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub codebase_id: CodebaseId,
    pub cwd: String,
    pub stdin: StdinPolicy,
    pub timeout_ms: NonZeroU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StdinPolicy {
    Null,
    Inherit,
}

/// The single tcp endpoint a service binds. Port is assigned by the runtime from
/// the slot window, so it is not declared; protocol/ownership/socket-activation
/// were frozen constants and are gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub endpoint_id: String,
    pub host: LoopbackHost,
}

/// A host that is provably an IP loopback literal — `"localhost"` and `"0.0.0.0"`
/// cannot deserialize, so the host-parse failure is unrepresentable at the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackHost(std::net::IpAddr);

impl LoopbackHost {
    /// Construct from a string in code (e.g. tests); the same loopback check the
    /// `Deserialize` impl applies.
    pub fn parse(host: &str) -> Result<Self, String> {
        let ip: std::net::IpAddr = host
            .parse()
            .map_err(|_| format!("host {host} is not an IP literal"))?;
        if !ip.is_loopback() {
            return Err(format!("host {host} is not a loopback address"));
        }
        Ok(Self(ip))
    }

    pub fn ip(&self) -> std::net::IpAddr {
        self.0
    }
}

impl std::fmt::Display for LoopbackHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for LoopbackHost {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LoopbackHost {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let ip: std::net::IpAddr = raw
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("host {raw} is not an IP literal")))?;
        if !ip.is_loopback() {
            return Err(serde::de::Error::custom(format!(
                "host {raw} is not a loopback address"
            )));
        }
        Ok(Self(ip))
    }
}

/// The timing of a tcp-connect probe, inlined onto the ready/health ops. The
/// probe targets the service's single endpoint by construction, so there is no
/// probe id or target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeTiming {
    pub timeout_ms: NonZeroU64,
    pub retry_interval_ms: NonZeroU64,
    pub max_attempts: NonZeroU32,
}

/// The full lifecycle as a per-class record: each class binds exactly the
/// primitive its mechanism requires, so an illegal binding (a stop exec, a probe
/// on start) is unrepresentable. The map key *is* the class — there is no
/// `class` discriminant and no way to declare a class twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lifecycle {
    pub prepare: PrepareSpec,
    pub start: StartSpec,
    pub ready: ReadySpec,
    pub health: HealthSpec,
    pub stop: StopSpec,
    pub clean: CleanSpec,
}

/// prepare: an optional data-dir init exec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareSpec {
    pub operation_id: OperationId,
    pub exec_id: Option<ExecId>,
    pub exec_args: Vec<String>,
    pub terminal: TerminalSemantics,
}

/// start: spawn-and-own a required exec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartSpec {
    pub operation_id: OperationId,
    pub exec_id: ExecId,
    pub exec_args: Vec<String>,
    pub terminal: TerminalSemantics,
}

/// ready: wait on a tcp probe of the service endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadySpec {
    pub operation_id: OperationId,
    pub probe: ProbeTiming,
    pub terminal: TerminalSemantics,
}

/// health: wait on a tcp probe of the service endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthSpec {
    pub operation_id: OperationId,
    pub probe: ProbeTiming,
    pub terminal: TerminalSemantics,
}

/// stop: a signal-based runtime primitive (the former `stopPolicy`, folded in).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopSpec {
    pub operation_id: OperationId,
    pub signal: StopSignal,
    pub timeout_ms: NonZeroU64,
    pub terminal: TerminalSemantics,
}

/// clean: a marker-gated runtime primitive that binds nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanSpec {
    pub operation_id: OperationId,
    pub terminal: TerminalSemantics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalSemantics {
    pub success: String,
    pub failure: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceSpec {
    pub lifecycle: Lifecycle,
    pub endpoint: Endpoint,
    pub state_refs: Vec<String>,
    pub log_refs: Vec<String>,
    pub containment: ContainmentRequirement,
    pub identity: ServiceIdentity,
}

/// The signal the runtime sends for graceful shutdown. A closed set so an
/// unsendable signal is unrepresentable at the wire boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum StopSignal {
    Term,
    Int,
    Quit,
    Hup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainmentRequirement {
    /// All spawned processes must stay in the single runtime-owned process group.
    ProcessGroup,
    /// The service's direct child is a stable supervisor of its own descendant
    /// tree (children may form their own process groups, e.g. Postgres). The
    /// runtime contains and reconciles the whole process tree.
    ProcessTree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceIdentity {
    pub service_address_hash: String,
    pub endpoint_identity_hash: String,
    pub state_identity_hash: String,
    pub runtime_compatibility_hash: String,
    pub target_identity_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSpec {
    pub operation_id: OperationId,
    pub exec_id: ExecId,
    pub args: Vec<String>,
    pub depends_on_services_ready: UniqueVec<ServiceId>,
    pub exit_policy: ExitPolicy,
    pub artifact_refs: Vec<String>,
    pub log_refs: Vec<String>,
    pub summary_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExitPolicy {
    pub success_codes: UniqueVec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowSpec {
    /// Services that must be started and ready before any node runs.
    pub services_required: UniqueVec<ServiceId>,
    /// Bounded acyclic dependency graph of task nodes, keyed by node id.
    pub nodes: BTreeMap<String, WorkflowNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowNode {
    pub task_id: TaskId,
    /// Other node ids in the same workflow that must succeed first.
    pub depends_on: UniqueVec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Docs {
    pub title: String,
    pub summary: String,
}
