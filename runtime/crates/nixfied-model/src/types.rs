use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub capabilities: Capabilities,
    pub runtime_constraints: RuntimeConstraints,
    pub surfaces: Vec<SurfaceSpec>,
    pub placement: Placement,
    pub state: StatePolicy,
    pub secrets: Vec<SecretRef>,
    pub closures: Vec<ClosureSpec>,
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
    pub required_runtime_capabilities: RuntimeCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCapabilities {
    pub process_group: bool,
    pub tcp_port_ownership: bool,
    pub sqlite_wal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Codebase {
    pub codebase_id: String,
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
    pub environment_id: String,
    pub services: Vec<String>,
    pub tasks: Vec<String>,
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
pub struct Capabilities {
    pub environments: Vec<String>,
    pub slots: Vec<u32>,
    pub services: Vec<String>,
    pub tasks: Vec<String>,
    pub workflows: Vec<String>,
    pub surfaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeConstraints {
    pub allowed_environments: Vec<String>,
    pub slot_min: u32,
    pub slot_default: u32,
    pub slot_max: u32,
    pub allow_port_override: bool,
    pub collision_policy: CollisionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollisionPolicy {
    Fail,
    ProbeInRange,
    RequestOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub input_schema: Value,
    pub output_schema: Value,
    pub exit_classes: Vec<String>,
    pub evaluation_permission: EvaluationPermission,
    pub maturity: SurfaceMaturity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationPermission {
    Never,
    Allowed,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceMaturity {
    M0,
    Experimental,
    Stable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placement {
    pub state_root_template: String,
    pub registry_dir: String,
    pub run_dir_template: String,
    pub logs_dir_template: String,
    pub artifacts_dir_template: String,
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
pub struct SecretRef {
    pub secret_id: String,
    pub target: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosureSpec {
    pub closure_id: String,
    pub kind: ClosureKind,
    pub store_path: String,
    pub executable: String,
    pub target_system: String,
    pub operation_bindings: Vec<String>,
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
    pub exec_id: String,
    pub closure_id: String,
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub codebase_id: String,
    pub cwd: String,
    pub stdin: StdinPolicy,
    pub timeout_ms: u64,
    pub output_capture: OutputCapture,
    pub cancellation_mode: CancellationMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StdinPolicy {
    Null,
    Inherit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputCapture {
    None,
    Stdout,
    Stderr,
    StdoutStderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CancellationMode {
    KillProcessGroup,
    KillProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndpointSpec {
    pub endpoint_id: String,
    pub protocol: EndpointProtocol,
    pub host: String,
    pub port: PortPolicy,
    pub ownership_verification: OwnershipVerification,
    pub socket_activation: SocketActivation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointProtocol {
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PortPolicy {
    Fixed { port: u16 },
    CandidateWindow { start: u16, end: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnershipVerification {
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SocketActivation {
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeSpec {
    pub probe_id: String,
    pub target: ProbeTarget,
    pub timeout_ms: u64,
    pub retry_interval_ms: u64,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProbeTarget {
    TcpConnect { endpoint_id: String },
    HttpGet { endpoint_id: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleOpSpec {
    pub operation_id: String,
    pub class: LifecycleOpClass,
    pub exec_id: Option<String>,
    pub exec_args: Vec<String>,
    pub probe_id: Option<String>,
    pub terminal: TerminalSemantics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleOpClass {
    Start,
    Ready,
    Stop,
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
    pub service_id: String,
    pub foreground: bool,
    pub lifecycle: Vec<LifecycleOpSpec>,
    pub endpoints: Vec<EndpointSpec>,
    pub probes: Vec<ProbeSpec>,
    pub readiness_probe: String,
    pub stop_policy: StopPolicy,
    pub state_refs: Vec<String>,
    pub log_refs: Vec<String>,
    pub containment: ContainmentRequirement,
    pub lifetime: ServiceLifetime,
    pub identity: ServiceIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopPolicy {
    pub signal: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainmentRequirement {
    ProcessGroup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceLifetime {
    RunScoped,
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
    pub task_id: String,
    pub operation_id: String,
    pub exec_id: String,
    pub args: Vec<String>,
    pub depends_on_services_ready: Vec<String>,
    pub exit_policy: ExitPolicy,
    pub output_capture: OutputCapture,
    pub artifact_refs: Vec<String>,
    pub log_refs: Vec<String>,
    pub summary_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExitPolicy {
    pub success_codes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowSpec {
    pub workflow_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Docs {
    pub title: String,
    pub summary: String,
}
