use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

use serde::{Deserialize, Serialize};

use crate::ids::{ClosureId, CodebaseId, OperationId, ServiceId, TaskId};
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
    /// The isolation namespaces (state roots, slots, registry keys) the model
    /// supports. Membership does not exist: running a task brings up exactly
    /// the services its leaves require. A single `dev` environment for now.
    pub environments: UniqueVec<String>,
    pub slot_policy: SlotPolicy,
    pub placement: Placement,
    pub state: StatePolicy,
    pub closures: BTreeMap<String, ClosureSpec>,
    pub services: BTreeMap<String, ServiceSpec>,
    pub tasks: BTreeMap<String, TaskSpec>,
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

/// The one way anything in the model says "run this program" (INVOKE-1):
/// inline, anonymous, fully applied — no invocation registry, no invocation
/// ids. `tools` references declared closures whose executables' parent
/// directories form the child PATH in declared order; `run` is the argv, and
/// `executable` is the eval-resolved absolute path of `run[0]`: the
/// executable of the first tool closure whose declared executable basename
/// equals `run[0]`. Lowering re-derives that resolution and rejects a model
/// that disagrees, so the runtime resolves nothing on the host (SEAM-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationSpec {
    pub tools: UniqueVec<ClosureId>,
    pub run: Vec<String>,
    pub executable: String,
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

/// How a ready/health op decides the service answers, inlined onto the op. A
/// `tcp` probe connects to the service's single endpoint by construction (no
/// probe id or target); an `exec` probe runs a bound short-lived command (e.g.
/// `pg_isready`) whose exit 0 is success. The wire shape is one closed struct
/// with a `kind` discriminator — `deny_unknown_fields` does not compose with
/// tagged enums — and kind/field coherence is proven at lowering, where the
/// executor side becomes a real tagged enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeSpec {
    pub kind: ProbeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<InvocationSpec>,
    /// Per-attempt budget: the tcp connect timeout, or the exec attempt's
    /// kill-after deadline (the invocation's own timeoutMs does not apply).
    pub timeout_ms: NonZeroU64,
    pub retry_interval_ms: NonZeroU64,
    pub max_attempts: NonZeroU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeKind {
    Tcp,
    Exec,
}

/// The full lifecycle as a per-class record: each class binds exactly the
/// primitive its mechanism requires, so an illegal binding (a stop exec, a probe
/// on start) is unrepresentable. The map key *is* the class — there is no
/// `class` discriminant and no way to declare a class twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lifecycle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare: Option<PrepareSpec>,
    pub start: StartSpec,
    pub ready: ReadySpec,
    pub health: HealthSpec,
    pub stop: StopSpec,
    pub clean: CleanSpec,
}

/// prepare: an optional **task reference** with full task semantics —
/// composites allowed, cross-service `requires` allowed (the combined
/// `connectsTo` + prepare-requires graph must be acyclic). Its evidence is the
/// referenced task's flattened nodes; the position has no operation id of its
/// own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareSpec {
    pub task: TaskId,
}

/// start: spawn-and-own a required invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartSpec {
    pub operation_id: OperationId,
    pub invocation: InvocationSpec,
    pub terminal: TerminalSemantics,
}

/// ready: wait on a probe (tcp connect or bound exec) of the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadySpec {
    pub operation_id: OperationId,
    pub probe: ProbeSpec,
    pub terminal: TerminalSemantics,
}

/// health: wait on a probe (tcp connect or bound exec) of the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthSpec {
    pub operation_id: OperationId,
    pub probe: ProbeSpec,
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
    /// The loopback endpoints the service binds, keyed by endpointId. The planner
    /// assigns each one a port from a contiguous per-service block, so every
    /// listener is reserved and conflict-checked — no service runs an unmodeled
    /// port. Own endpoints are addressable in exec args/env via
    /// `${port:<endpointId>}`/`${host:<endpointId>}`. MAY be empty: durable is
    /// not listening — an endpoint-less service (queue consumer, indexer) is
    /// owned, probed (invocation probes only), contained, and cleaned, but
    /// makes no addressability claim and nothing may address it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endpoints: BTreeMap<String, Endpoint>,
    /// The endpoint bare `${port}`/`${host}` resolve to, the target of the tcp
    /// readiness/health probe, and the endpoint a `connectsTo` dependent reaches
    /// via `${port:<serviceId>}`. Must be a key in `endpoints`; present iff any
    /// endpoint is declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_endpoint: Option<String>,
    /// Same-slot services this service connects to; the declaration gates
    /// `${port:<serviceId>}`/`${host:<serviceId>}` resolution and start order.
    pub connects_to: UniqueVec<ServiceId>,
    pub state_refs: Vec<String>,
    pub log_refs: Vec<String>,
    pub containment: ContainmentRequirement,
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

/// A task is a **leaf** (one bounded invocation with orchestration: requires,
/// exit policy, evidence refs) or a **composite** (a static named-step DAG over
/// task references — STATIC-1: no parameters, conditionals, retries, or loops).
/// The wire shape is one closed struct with a `kind` discriminator — like
/// `ProbeSpec`, because `deny_unknown_fields` does not compose with tagged
/// enums — and kind/field coherence is proven by validation and lowering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSpec {
    pub kind: TaskKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<InvocationSpec>,
    /// Services that must be ready (alive, probed, addressable) while the
    /// leaf runs — the leaf-intrinsic fact `servicesRequired` derives from.
    #[serde(default, skip_serializing_if = "UniqueVec::is_empty")]
    pub requires: UniqueVec<ServiceId>,
    /// Derived (docs/DERIVATION_SPEC.md §3): the union of transitive leaf
    /// `requires`, closed over `connectsTo`, byte-sorted. Computed by the Nix
    /// compiler; the runtime re-derives and compares at admission (DERIVE-1).
    #[serde(default)]
    pub services_required: UniqueVec<ServiceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_policy: Option<ExitPolicy>,
    /// Composite body: named steps referencing declared tasks. Names are the
    /// step-path segments of evidence identity, so they exclude `.`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub steps: BTreeMap<String, StepSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Leaf,
    Composite,
}

/// One named step of a composite: a task reference plus the sibling steps that
/// must have completed successfully first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepSpec {
    pub task: TaskId,
    #[serde(default, skip_serializing_if = "UniqueVec::is_empty")]
    pub depends_on: UniqueVec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExitPolicy {
    pub success_codes: UniqueVec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Docs {
    pub title: String,
    pub summary: String,
}
