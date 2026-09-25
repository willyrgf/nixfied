//! The total lowering `Manifest -> ExecutionManifest`.
//!
//! Every source struct is destructured with **no `..`**, so adding a field to the
//! schema is a compile error here until the lowering consciously maps or refuses
//! it. Rejections are `ManifestAdmission` errors — the manifest was inexpressible to the
//! runtime — never execution failures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use nixfied_manifest::{
    ClosureSpec, InvocationSpec, Lifecycle, Manifest, OperationId, ProbeKind, ProbeSpec, ServiceId,
    ServiceSpec, StepSpec, StopSpec, TaskId, TaskKind, TaskSpec, TerminalSemantics,
    ValidatedManifest,
};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::types::*;
use crate::template::{Owner, Scope};

/// Lower a validated manifest into the executor's input. The result contains only
/// what the runtime can execute; anything it cannot is rejected here.
pub fn lower(document: &ValidatedManifest) -> RuntimeResult<ExecutionManifest> {
    let manifest: &Manifest = document;
    // First prove every cross-reference resolves; the structural map below then
    // builds the executor input from references known to exist.
    prove_references(manifest)?;
    // Exhaustive destructure — no `..`. Static-configuration fields validated by
    // `ValidatedManifest::try_from` are bound and intentionally ignored; the executable
    // fields are mapped below. A new schema field breaks this pattern (E0027).
    let Manifest {
        manifest_version: _,
        toolchain_id: _,
        runtime_abi: _,
        generator: _,
        project: _,
        target: _,
        codebases: _,
        secrets: _,
        environments: _,
        slot_policy: _,
        placement,
        state: _,
        closures,
        services,
        tasks,
    } = manifest;

    let mut resolver = InvocationResolver {
        closures,
        secrets: &manifest.secrets,
    };
    let lowered_services = services
        .iter()
        .map(|(name, service)| {
            Ok((
                ServiceId::new(name),
                lower_service(name, service, services, &mut resolver)?,
            ))
        })
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    let mut lowered_tasks = BTreeMap::new();
    for (id, task) in tasks {
        lowered_tasks.insert(
            TaskId::new(id),
            lower_task(id, task, &mut resolver, &lowered_services)?,
        );
    }

    let mut slot_windows = BTreeMap::new();
    for slot_placement in placement.slot_placements.values() {
        slot_windows.insert(
            slot_placement.slot,
            PortWindow::new(
                slot_placement.candidate_ports.start,
                slot_placement.candidate_ports.end,
            )
            .expect("validated placement has a nonempty port window"),
        );
    }

    let program = Program {
        services: lowered_services,
        tasks: lowered_tasks,
        slot_windows,
    };
    let facts = super::plan::prove_graph(&program)?;
    super::plan::prove_capacity(&program, facts)?;
    Ok(ExecutionManifest { program })
}

fn lower_service(
    name: &str,
    service: &ServiceSpec,
    all_services: &BTreeMap<String, ServiceSpec>,
    resolver: &mut InvocationResolver<'_>,
) -> RuntimeResult<ExecService> {
    let ServiceSpec {
        lifecycle,
        endpoints,
        primary_endpoint,
        connects_to,
        state_refs: _,
        log_refs: _,
        containment,
    } = service;
    let Lifecycle {
        prepare,
        start,
        ready,
        health,
        stop,
        clean,
    } = lifecycle;

    let endpoints = endpoints.clone();

    let owner = || format!("service {name}");
    let endpoint_less = endpoints.is_empty();
    let prepare = prepare.as_ref().map(|spec| spec.task.clone());
    // Named endpoint placeholders `${port:<name>}` in lifecycle invocation
    // args/env (including probe invocations) resolve against the service's own
    // endpoint ids or its declared connectsTo dependencies; reject any other
    // reference here rather than fail (or silently leak the literal placeholder)
    // at execution. Validation already proved own endpoint ids and connectsTo
    // ids are disjoint.
    // An endpoint-less connectsTo target keeps its ordering/derivation meaning
    // but is NOT addressable: it leaves the named-placeholder scope entirely.
    let scope = Scope {
        owner: Owner::Service(name),
        has_primary: primary_endpoint.is_some(),
        own_endpoints: endpoints.keys().map(String::as_str).collect(),
        services: connects_to
            .iter()
            .filter_map(|id| {
                (!all_services[id.as_str()].endpoints.is_empty()).then_some(id.as_str())
            })
            .collect(),
    };
    // Effects coherence, both directions: a listening service's start closure
    // must attest `network-listener`; an endpoint-less service's must not — it
    // would announce a listener the planner cannot reserve.
    let (start_exec, closure_id, closure) = resolver.resolve(&owner, &start.invocation, &scope)?;
    {
        let listens = closure
            .effects
            .iter()
            .any(|effect| matches!(effect, nixfied_manifest::ClosureEffect::NetworkListener));
        if !endpoint_less && !listens {
            return Err(Rejection::EffectsIncoherent {
                service: name.to_string(),
                closure_id: closure_id.to_string(),
                expected: "declared endpoints require `network-listener` on the start closure",
            }
            .into());
        }
        if endpoint_less && listens {
            return Err(Rejection::EffectsIncoherent {
                service: name.to_string(),
                closure_id: closure_id.to_string(),
                expected: "an endpoint-less service's start closure must not declare `network-listener` (an unreservable listener)",
            }
            .into());
        }
    }
    let start = StartOp {
        meta: op_meta(&start.operation_id, &start.terminal),
        exec: start_exec,
    };
    let ready = ReadyOp {
        meta: op_meta(&ready.operation_id, &ready.terminal),
        probe: lower_probe(name, "ready", &ready.probe, &scope, resolver)?,
    };
    let health = HealthOp {
        meta: op_meta(&health.operation_id, &health.terminal),
        probe: lower_probe(name, "health", &health.probe, &scope, resolver)?,
    };
    let stop = lower_stop(stop);
    let clean = CleanOp {
        meta: op_meta(&clean.operation_id, &clean.terminal),
    };

    Ok(ExecService {
        name: ServiceId::new(name),
        prepare,
        start,
        ready,
        health,
        stop,
        clean,
        endpoints,
        primary_endpoint: primary_endpoint.clone(),
        connects_to: connects_to.iter().cloned().collect(),
        containment: *containment,
    })
}

fn lower_stop(stop: &StopSpec) -> StopOp {
    StopOp {
        meta: op_meta(&stop.operation_id, &stop.terminal),
        signal: stop.signal,
        timeout: Duration::from_millis(stop.timeout_ms.get()),
    }
}

fn op_meta(operation_id: &OperationId, terminal: &TerminalSemantics) -> OpMeta {
    OpMeta {
        operation_id: operation_id.clone(),
        terminal_success: terminal.success.clone(),
        terminal_failure: terminal.failure.clone(),
    }
}

fn lower_task(
    task_id: &str,
    task: &TaskSpec,
    resolver: &mut InvocationResolver<'_>,
    services: &BTreeMap<ServiceId, ExecService>,
) -> RuntimeResult<ExecutableTask> {
    let TaskSpec {
        kind,
        default_output: _,
        operation_id,
        invocation,
        requires,
        exit_policy,
        steps,
        artifact_refs: _,
        log_refs: _,
        summary_refs: _,
    } = task;
    match kind {
        TaskKind::Composite => {
            return Ok(ExecutableTask::Composite(lower_composite(task_id, steps)));
        }
        TaskKind::Leaf => {}
    }
    let owner = || format!("task {task_id}");
    // Convert the structurally checked wire alternatives into the native leaf.
    let invocation = invocation
        .as_ref()
        .expect("validated leaf has an invocation");
    let exit_policy = exit_policy
        .as_ref()
        .expect("validated leaf has an exit policy");
    operation_id
        .as_ref()
        .expect("validated leaf has an operation id");
    let requires: Vec<_> = requires.iter().cloned().collect();
    // `${port}`/`${host}` resolve from the task's primary (first) requirement.
    // A task that requires no services — or whose primary requirement is
    // endpoint-less — has no endpoint, so referencing them is unrunnable:
    // reject it here rather than fail at execution.
    let primary_has_endpoint = requires
        .first()
        .and_then(|id| services.get(id))
        .map(|service| !service.endpoints.is_empty())
        .unwrap_or(false);
    // Named endpoint placeholders may only reference declared service
    // requirements (any of them, not just the primary) that actually declare
    // endpoints — an endpoint-less requirement is not addressable.
    let named: BTreeSet<&str> = requires
        .iter()
        .filter(|id| {
            services
                .get(id.as_str())
                .map(|service| !service.endpoints.is_empty())
                .unwrap_or(false)
        })
        .map(|id| id.as_str())
        .collect();
    let scope = Scope {
        owner: Owner::Task(task_id),
        has_primary: primary_has_endpoint,
        own_endpoints: BTreeSet::new(),
        services: named,
    };
    let (exec, _, _) = resolver.resolve(&owner, invocation, &scope)?;
    Ok(ExecutableTask::Leaf(ExecTask {
        task_id: TaskId::new(task_id),
        timeout: invocation
            .timeout_ms
            .map(|timeout| Duration::from_millis(timeout.get())),
        exec,
        requires,
        success_codes: exit_policy.success_codes.iter().copied().collect(),
    }))
}

/// References were checked before resolution; the planner owns cycle proof.
fn lower_composite(task_id: &str, steps: &BTreeMap<String, StepSpec>) -> ExecComposite {
    ExecComposite {
        task_id: TaskId::new(task_id),
        steps: steps
            .iter()
            .map(|(name, step)| ExecStep {
                name: name.clone(),
                task: step.task.clone(),
                depends_on: step.depends_on.iter().cloned().collect(),
            })
            .collect(),
    }
}

/// Lower the kind-discriminated wire probe into the executor's closed enum,
/// proving kind/field coherence: a tcp probe must not carry an invocation, an
/// exec probe must carry one. The probe's own timing governs every attempt —
/// the invocation's authored timeout is not an execution deadline here.
fn lower_probe(
    service: &str,
    class: &'static str,
    probe: &ProbeSpec,
    scope: &Scope<'_>,
    resolver: &mut InvocationResolver<'_>,
) -> RuntimeResult<Probe> {
    let ProbeSpec {
        kind,
        invocation,
        timeout_ms,
        retry_interval_ms,
        max_attempts,
    } = probe;
    let timeout = Duration::from_millis(timeout_ms.get());
    let retry_interval = Duration::from_millis(retry_interval_ms.get());
    let max_attempts = *max_attempts;
    match kind {
        ProbeKind::Tcp => {
            if !scope.has_primary {
                return Err(Rejection::TcpProbeWithoutEndpoint {
                    service: service.to_string(),
                    class,
                }
                .into());
            }
            if invocation.is_some() {
                return Err(Rejection::ProbeExecOnTcp {
                    service: service.to_string(),
                    class,
                }
                .into());
            }
            Ok(Probe::Tcp(ProbePolicy {
                label: class.to_string(),
                timeout,
                retry_interval,
                max_attempts,
            }))
        }
        ProbeKind::Exec => {
            let Some(invocation) = invocation else {
                return Err(Rejection::ProbeExecMissing {
                    service: service.to_string(),
                    class,
                }
                .into());
            };
            let owner = || format!("service {service} {class} probe");
            let (exec, _, _) = resolver.resolve(&owner, invocation, scope)?;
            Ok(Probe::Exec(ExecProbe {
                exec,
                policy: ProbePolicy {
                    label: class.to_string(),
                    timeout,
                    retry_interval,
                    max_attempts,
                },
            }))
        }
    }
}

/// Selection and effect checks use the same chosen closure during lowering.
struct InvocationResolver<'a> {
    closures: &'a BTreeMap<String, ClosureSpec>,
    secrets: &'a BTreeMap<String, nixfied_manifest::SecretDescriptor>,
}

impl<'a> InvocationResolver<'a> {
    /// Resolve an inline invocation against the declared closures: every tool must
    /// be a declared closure, run[0] must resolve per the derivation spec, and the
    /// carried `executable` must equal that resolution (fail closed). The PATH
    /// roots — each tool executable's parent directory, in declared order — are
    /// carried for the runtime's child-PATH assembly.
    fn resolve(
        &mut self,
        owner: &impl Fn() -> String,
        invocation: &InvocationSpec,
        scope: &Scope<'_>,
    ) -> RuntimeResult<(ResolvedInvocation, &'a str, &'a ClosureSpec)> {
        let InvocationSpec {
            tools,
            run,
            executable,
            env,
            codebase_id: _,
            cwd,
            stdin,
            timeout_ms: _,
        } = invocation;
        let Some(program) = run.first() else {
            return Err(Rejection::RunUnresolvable {
                owner: owner(),
                program: String::new(),
            }
            .into());
        };
        // PATH is runtime-owned: it is assembled from the tool roots at spawn, so a
        // declared PATH would be silently overwritten — reject it instead.
        if env.contains_key("PATH") {
            return Err(Rejection::ReservedEnvVar {
                owner: owner(),
                name: "PATH",
            }
            .into());
        }
        let mut tool_roots = Vec::with_capacity(tools.len());
        let mut selected = None;
        for tool in tools.iter() {
            let Some((id, closure)) = self.closures.get_key_value(tool.as_str()) else {
                return Err(undeclared("invocation.tools", tool.as_str()).into());
            };
            if selected.is_none()
                && Path::new(&closure.executable)
                    .file_name()
                    .and_then(|name| name.to_str())
                    == Some(program.as_str())
            {
                selected = Some((id.as_str(), closure));
            }
            let root = Path::new(&closure.executable)
                .parent()
                .map(|parent| parent.display().to_string())
                .unwrap_or_default();
            tool_roots.push(root);
        }
        let Some((closure_id, resolved)) = selected else {
            return Err(Rejection::RunUnresolvable {
                owner: owner(),
                program: program.clone(),
            }
            .into());
        };
        if resolved.executable != *executable {
            return Err(Rejection::ExecutableMismatch {
                owner: owner(),
                carried: executable.clone(),
                resolved: resolved.executable.clone(),
            }
            .into());
        }
        Ok((
            ResolvedInvocation {
                executable: executable.clone(),
                args: run[1..]
                    .iter()
                    .map(|text| Template::parse(text, scope, self.secrets, false))
                    .collect::<RuntimeResult<_>>()?,
                env: env
                    .iter()
                    .map(|(key, value)| {
                        Ok((
                            key.clone(),
                            Template::parse(value, scope, self.secrets, true)?,
                        ))
                    })
                    .collect::<RuntimeResult<_>>()?,
                cwd: RelativeCwd::new(cwd)
                    .ok_or_else(|| Rejection::InvalidCwd { owner: owner() })?,
                stdin: *stdin,
                tool_roots,
            },
            closure_id,
            resolved,
        ))
    }
}

/// The closed set of reasons the manifest cannot be lowered into an executable
/// program. Every relational/reference check the runtime needs lives here, so a
/// successful `lower` is a proof the references resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    InvalidCwd {
        owner: String,
    },
    UndeclaredReference {
        kind: &'static str,
        id: String,
    },
    DuplicateOperationId {
        id: String,
    },
    ClosureTargetMismatch {
        closure_id: String,
        target_system: String,
        closure_system: String,
    },
    RunUnresolvable {
        owner: String,
        program: String,
    },
    ExecutableMismatch {
        owner: String,
        carried: String,
        resolved: String,
    },
    ReservedEnvVar {
        owner: String,
        name: &'static str,
    },
    ProbeExecOnTcp {
        service: String,
        class: &'static str,
    },
    TcpProbeWithoutEndpoint {
        service: String,
        class: &'static str,
    },
    EffectsIncoherent {
        service: String,
        closure_id: String,
        expected: &'static str,
    },
    ServiceGraphCycle {
        cycle: String,
    },
    ProbeExecMissing {
        service: String,
        class: &'static str,
    },
}

impl Rejection {
    fn message(&self) -> String {
        match self {
            Rejection::InvalidCwd { owner } => {
                format!("{owner} cwd must be a confined relative path")
            }
            Rejection::UndeclaredReference { kind, id } => {
                format!("{kind} references undeclared {id}")
            }
            Rejection::DuplicateOperationId { id } => {
                format!("operation id {id} is declared more than once")
            }
            Rejection::ClosureTargetMismatch {
                closure_id,
                target_system,
                closure_system,
            } => format!(
                "closure {closure_id} targetSystem {target_system} does not match closureSystem {closure_system}"
            ),
            Rejection::RunUnresolvable { owner, program } => format!(
                "{owner} run[0] {program:?} is not the executable of any declared tool closure"
            ),
            Rejection::ExecutableMismatch {
                owner,
                carried,
                resolved,
            } => format!(
                "{owner} carries executable {carried} but its tools resolve run[0] to {resolved}"
            ),
            Rejection::ReservedEnvVar { owner, name } => {
                format!("{owner} declares runtime-owned environment variable {name}")
            }
            Rejection::ProbeExecOnTcp { service, class } => {
                format!("service {service} {class} probe is tcp but carries an invocation")
            }
            Rejection::TcpProbeWithoutEndpoint { service, class } => {
                format!(
                    "service {service} is endpoint-less but its {class} probe is tcp (no target to connect)"
                )
            }
            Rejection::EffectsIncoherent {
                service,
                closure_id,
                expected,
            } => format!("service {service} start closure {closure_id}: {expected}"),
            Rejection::ServiceGraphCycle { cycle } => format!(
                "the combined connectsTo + prepare-requires service graph has a cycle: {cycle}"
            ),
            Rejection::ProbeExecMissing { service, class } => {
                format!("service {service} {class} probe is exec but declares no invocation")
            }
        }
    }
}

impl From<Rejection> for RuntimeError {
    fn from(rejection: Rejection) -> Self {
        RuntimeError::new(ErrorCode::ManifestAdmission, rejection.message())
    }
}

fn undeclared(kind: &'static str, id: impl Into<String>) -> Rejection {
    Rejection::UndeclaredReference {
        kind,
        id: id.into(),
    }
}

/// Check cross-reference membership and operation identity before local invocation
/// resolution. Graph algorithms consume these facts without repeating membership
/// checks; cycles and capacity are proved after local resolution.
fn prove_references(manifest: &Manifest) -> Result<(), Rejection> {
    let codebase_ids = manifest
        .codebases
        .iter()
        .map(|codebase| codebase.codebase_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut declared_operations = BTreeSet::new();
    let positions = super::invocations(manifest).collect::<Vec<_>>();

    for invocation in &positions {
        if !codebase_ids.contains(invocation.codebase_id.as_str()) {
            return Err(undeclared(
                "invocation.codebaseId",
                invocation.codebase_id.as_str(),
            ));
        }
    }

    for (closure_id, closure) in &manifest.closures {
        if closure.target_system != manifest.target.closure_system {
            return Err(Rejection::ClosureTargetMismatch {
                closure_id: closure_id.to_string(),
                target_system: closure.target_system.clone(),
                closure_system: manifest.target.closure_system.clone(),
            });
        }
    }

    // Operation ids are globally unique across every lifecycle and task.
    for service in manifest.services.values() {
        for operation_id in lifecycle_op_ids(&service.lifecycle) {
            if !declared_operations.insert(operation_id) {
                return Err(Rejection::DuplicateOperationId {
                    id: operation_id.to_string(),
                });
            }
        }
    }
    for operation_id in manifest
        .tasks
        .values()
        .filter_map(|task| task.operation_id.as_ref())
    {
        if !declared_operations.insert(operation_id.as_str()) {
            return Err(Rejection::DuplicateOperationId {
                id: operation_id.to_string(),
            });
        }
    }

    for service in manifest.services.values() {
        for target in service.connects_to.iter() {
            if !manifest.services.contains_key(target.as_str()) {
                return Err(undeclared("service.connectsTo", target.as_str()));
            }
        }
        if let Some(prepare) = &service.lifecycle.prepare
            && !manifest.tasks.contains_key(prepare.task.as_str())
        {
            return Err(undeclared("service.prepare.task", prepare.task.as_str()));
        }
    }
    for task in manifest.tasks.values() {
        for target in task.requires.iter() {
            if !manifest.services.contains_key(target.as_str()) {
                return Err(undeclared("task.requires", target.as_str()));
            }
        }
        for step in task.steps.values() {
            if !manifest.tasks.contains_key(step.task.as_str()) {
                return Err(undeclared("task.steps.task", step.task.as_str()));
            }
            for dependency in step.depends_on.iter() {
                if !task.steps.contains_key(dependency) {
                    return Err(undeclared("task.steps.dependsOn", dependency));
                }
            }
        }
    }

    // Tool membership is a reference check. Executable selection belongs to
    // the subsequent per-invocation resolution pass.
    for invocation in &positions {
        for tool in invocation.tools.iter() {
            if !manifest.closures.contains_key(tool.as_str()) {
                return Err(undeclared("invocation.tools", tool.as_str()));
            }
        }
    }

    Ok(())
}

/// The operation id of each lifecycle op, in canonical order. Prepare is a
/// task reference and carries no operation id.
fn lifecycle_op_ids(lifecycle: &Lifecycle) -> [&str; 5] {
    [
        lifecycle.start.operation_id.as_str(),
        lifecycle.ready.operation_id.as_str(),
        lifecycle.health.operation_id.as_str(),
        lifecycle.stop.operation_id.as_str(),
        lifecycle.clean.operation_id.as_str(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn invocation_value(executable: &str, run: Value) -> Value {
        json!({
            "tools": ["c"],
            "run": run,
            "executable": executable,
            "env": {},
            "codebaseId": "main",
            "cwd": ".",
            "stdin": "null",
            "timeoutMs": 1000
        })
    }

    fn manifest_value() -> Value {
        json!({
            "manifestVersion": 1,
            "toolchainId": "nixfied-toolchain:1",
            "runtimeAbi": nixfied_manifest::runtime_abi(),
            "generator": { "name": "n", "version": "1", "emitter": "e" },
            "project": { "projectId": "p", "name": "P" },
            "target": {
                "system": "x86_64-linux", "os": "linux", "arch": "x86_64",
                "closureSystem": "x86_64-linux"
            },
            "codebases": [{
                "codebaseId": "main", "logicalRoot": ".", "sourceMode": "live-workspace",
                "sourceIdentity": "live",
                "sourcePolicy": { "dirtyPolicy": "warn", "admissionFingerprintPolicy": "live" }
            }],
            "secrets": {},
            "environments": ["dev"],
            "slotPolicy": { "min": 0, "default": 0, "max": 0 },
            "placement": {
                "slotPlacements": {
                    "0": {
                        "slot": 0,
                        "candidatePorts": { "start": 23080, "end": 23090 }
                    }
                }
            },
            "state": {
                "markerIdentity": "nixfied-state",
                "persistence": "run-scoped"
            },
            "closures": {
                "c": {
                    "kind": "executable", "storePath": "/nix/store/c", "executable": "/nix/store/c/bin/svc",
                    "targetSystem": "x86_64-linux",

                    "requiresExecutable": true, "effects": ["process", "network-listener"]
                },
                "ct": {
                    "kind": "executable", "storePath": "/nix/store/ct", "executable": "/nix/store/ct/bin/task",
                    "targetSystem": "x86_64-linux",

                    "requiresExecutable": true, "effects": ["process"]
                }
            },
            "services": { "svc": service_value() },
            "tasks": {
                "t": {
                    "kind": "leaf",
                    "operationId": "task.t.run",
                    "invocation": {
                        "tools": ["ct"],
                        "run": ["task", "--port", "${port}"],
                        "executable": "/nix/store/ct/bin/task",
                        "env": {},
                        "codebaseId": "main",
                        "cwd": ".",
                        "stdin": "null",
                        "timeoutMs": 1000
                    },
                    "requires": ["svc"],

                    "exitPolicy": { "successCodes": [0] },
                    "artifactRefs": [], "logRefs": [], "summaryRefs": []
                }
            },
        })
    }

    fn service_value() -> Value {
        json!({
            "lifecycle": {
                "start": {
                    "operationId": "svc.start",
                    "invocation": invocation_value("/nix/store/c/bin/svc", json!(["svc", "serve", "--port", "${port}"])),
                    "terminal": { "success": "spawned", "failure": "failed" }
                },
                "ready": { "operationId": "svc.ready", "probe": { "kind": "tcp", "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "ready", "failure": "not-ready" } },
                "health": { "operationId": "svc.health", "probe": { "kind": "tcp", "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "healthy", "failure": "unhealthy" } },
                "stop": { "operationId": "svc.stop", "signal": "TERM", "timeoutMs": 5000, "terminal": { "success": "stopped", "failure": "failed" } },
                "clean": { "operationId": "svc.clean", "terminal": { "success": "cleaned", "failure": "failed" } }
            },
            "endpoints": { "svc-tcp": { "endpointId": "svc-tcp", "host": "127.0.0.1" } },
            "primaryEndpoint": "svc-tcp",
            "connectsTo": [],
            "stateRefs": [], "logRefs": [],
            "containment": "process-group"
        })
    }

    fn named_service_value(name: &str) -> Value {
        let mut service = service_value();
        service["lifecycle"]["start"]["operationId"] = json!(format!("{name}.start"));
        service["lifecycle"]["ready"]["operationId"] = json!(format!("{name}.ready"));
        service["lifecycle"]["health"]["operationId"] = json!(format!("{name}.health"));
        service["lifecycle"]["stop"]["operationId"] = json!(format!("{name}.stop"));
        service["lifecycle"]["clean"]["operationId"] = json!(format!("{name}.clean"));
        service["endpoints"] = json!({ format!("{name}-tcp"): { "endpointId": format!("{name}-tcp"), "host": "127.0.0.1" } });
        service["primaryEndpoint"] = json!(format!("{name}-tcp"));
        service
    }

    fn add_named_service(value: &mut Value, name: &str, connects_to: &[&str]) {
        let mut service = named_service_value(name);
        service["connectsTo"] = json!(connects_to);
        value["services"][name] = service;
    }

    fn manifest_from(value: Value) -> ValidatedManifest {
        ValidatedManifest::try_from(
            serde_json::from_value::<Manifest>(value).expect("fixture should deserialize"),
        )
        .expect("lowering fixture must be structurally valid")
    }

    #[test]
    fn lowers_a_valid_manifest() {
        let em = lower(&manifest_from(manifest_value())).expect("valid manifest lowers");
        let svc = em.services().get("svc").expect("service lowered");
        assert_eq!(svc.endpoints["svc-tcp"].host.to_string(), "127.0.0.1");
        assert_eq!(svc.primary_endpoint.as_deref(), Some("svc-tcp"));
        assert_eq!(svc.stop.signal, StopSignal::Term);
        assert!(svc.prepare.is_none());
        // Args are run[1..]; run[0] resolved to the closure executable.
        assert_eq!(svc.start.exec.executable, "/nix/store/c/bin/svc");
        assert_eq!(svc.start.exec.tool_roots, vec!["/nix/store/c/bin"]);
        assert_eq!(svc.start.exec.stdin, StdinPolicy::Null);
        assert_eq!(em.program.slot_windows[&0].start(), 23080);
        let task = em.leaf("t").expect("task lowered");
        assert_eq!(task.success_codes, vec![0]);
        assert_eq!(task.exec.tool_roots, vec!["/nix/store/ct/bin"]);
        assert_eq!(task.requires, vec![ServiceId::new("svc")]);
        assert_eq!(task.timeout, Some(Duration::from_millis(1000)));
    }

    /// An absent authored deadline lowers to none: no former default returns.
    #[test]
    fn missing_task_timeout_lowers_to_no_deadline() {
        let mut value = manifest_value();
        value["tasks"]["t"]["invocation"]
            .as_object_mut()
            .unwrap()
            .remove("timeoutMs");
        let execution = lower(&manifest_from(value)).expect("a task without a deadline lowers");
        assert_eq!(execution.leaf("t").expect("task lowered").timeout, None);
    }

    #[test]
    fn first_tool_selection_owns_executable_and_effects() {
        for first in ["c", "alternate"] {
            let mut value = manifest_value();
            value["closures"]["alternate"] = value["closures"]["c"].clone();
            value["closures"]["alternate"]["executable"] = json!("/nix/store/alternate/bin/svc");
            value["closures"]["alternate"]["storePath"] = json!("/nix/store/alternate");
            let other = if first == "c" { "alternate" } else { "c" };
            value["services"]["svc"]["lifecycle"]["start"]["invocation"]["tools"] =
                json!([first, other]);
            value["services"]["svc"]["lifecycle"]["start"]["invocation"]["executable"] =
                value["closures"][first]["executable"].clone();
            value["closures"][other]["effects"] = json!(["process"]);

            let execution = lower(&manifest_from(value.clone())).unwrap();
            assert_eq!(
                execution.services()["svc"].start.exec.executable,
                value["closures"][first]["executable"].as_str().unwrap()
            );
            value["closures"][first]["effects"] = json!(["process"]);
            value["closures"][other]["effects"] = json!(["process", "network-listener"]);
            let error = lower(&manifest_from(value)).unwrap_err();
            assert_eq!(
                error.message,
                format!(
                    "service svc start closure {first}: declared endpoints require `network-listener` on the start closure"
                )
            );
        }
    }

    #[test]
    fn local_invocation_errors_precede_cycles() {
        let mut value = manifest_value();
        value["services"]["svc"]["connectsTo"] = json!(["svc"]);

        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"]["PATH"] =
            json!("forbidden");
        let error = lower(&manifest_from(value.clone())).unwrap_err();
        assert_eq!(
            error.message,
            "service svc declares runtime-owned environment variable PATH"
        );
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"] = json!({});
        let error = lower(&manifest_from(value)).unwrap_err();
        assert_eq!(
            error.message,
            "the combined connectsTo + prepare-requires service graph has a cycle: svc -[connectsTo]-> svc"
        );
    }

    #[test]
    fn each_service_invocation_finishes_validation_before_the_next_position() {
        let mut value = manifest_value();
        with_exec_ready_probe(&mut value);
        value["services"]["svc"]["lifecycle"]["ready"]["probe"]["invocation"]["executable"] =
            json!("/nix/store/incorrect/bin/svc");
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"]["ADDR"] =
            json!("${port:missing}");
        let error = lower(&manifest_from(value.clone())).unwrap_err();
        assert_eq!(
            error.message,
            "service svc references the endpoint of missing without declaring it in own endpoints or addressable connectsTo"
        );
        value["services"]["svc"]["endpoints"] = json!({});
        value["services"]["svc"]
            .as_object_mut()
            .unwrap()
            .remove("primaryEndpoint");
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"] = json!({});
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] = json!(["svc"]);
        value["closures"]["c"]["effects"] = json!(["process"]);
        let error = lower(&manifest_from(value)).unwrap_err();
        assert_eq!(
            error.message,
            "service svc ready probe carries executable /nix/store/incorrect/bin/svc but its tools resolve run[0] to /nix/store/c/bin/svc"
        );
    }

    #[test]
    fn cwd_is_lexically_confined_before_execution() {
        for cwd in [".", "work/subdir", "./work", "unix\\name"] {
            let mut value = manifest_value();
            value["tasks"]["t"]["invocation"]["cwd"] = json!(cwd);
            lower(&manifest_from(value)).expect("filesystem existence is checked at execution");
        }
        for cwd in ["..", "work/../outside", "/absolute", "nul\0byte"] {
            let mut value = manifest_value();
            value["tasks"]["t"]["invocation"]["cwd"] = json!(cwd);
            let error = lower(&manifest_from(value)).unwrap_err();
            assert_eq!(error.code, ErrorCode::ManifestAdmission);
            assert_eq!(error.message, "task t cwd must be a confined relative path");
        }
    }

    #[test]
    fn descriptive_refs_change_manifest_bytes_but_not_lowered_service() {
        let baseline = manifest_value();
        let baseline_execution = lower(&manifest_from(baseline.clone())).unwrap();
        for labels in [
            json!([]),
            json!(["slot"]),
            json!(["arbitrary", "../label", ""]),
        ] {
            let mut changed = baseline.clone();
            changed["services"]["svc"]["stateRefs"] = labels.clone();
            changed["services"]["svc"]["logRefs"] = json!(["descriptive-log"]);
            changed["tasks"]["t"]["artifactRefs"] = json!(["descriptive-artifact"]);
            changed["tasks"]["t"]["logRefs"] = json!(["descriptive-task-log"]);
            changed["tasks"]["t"]["summaryRefs"] = json!(["descriptive-summary"]);
            assert_ne!(
                serde_json::to_vec(&baseline).unwrap(),
                serde_json::to_vec(&changed).unwrap()
            );
            let manifest = manifest_from(changed);
            assert_eq!(
                serde_json::to_value(&*manifest).unwrap()["services"]["svc"]["stateRefs"],
                labels
            );
            let execution = lower(&manifest).expect("descriptive strings remain accepted");
            assert_eq!(
                format!("{:?}", execution.services()["svc"]),
                format!("{:?}", baseline_execution.services()["svc"])
            );
            assert_eq!(
                execution.services()["svc"].start.exec.args,
                baseline_execution.services()["svc"].start.exec.args
            );
            assert_eq!(
                execution.leaf("t").unwrap().exec.args,
                baseline_execution.leaf("t").unwrap().exec.args
            );
        }
    }

    #[test]
    fn stdin_inherit_is_carried_into_resolved_invocation() {
        // A manifest that declares `stdin: inherit` must lower to an invocation that
        // records it, not silently collapse to null.
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["stdin"] = json!("inherit");
        let em = lower(&manifest_from(value)).expect("inherit stdin lowers");
        assert_eq!(
            em.services().get("svc").unwrap().start.exec.stdin,
            StdinPolicy::Inherit
        );
    }

    #[test]
    fn rejects_an_undeclared_tool_reference() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["tools"] = json!(["ghost"]);
        let error = lower(&manifest_from(value)).expect_err("undeclared tool must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn rejects_an_unresolvable_run_program() {
        // run[0] names a program no tool closure's executable provides.
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["ghost-program"]);
        let error = lower(&manifest_from(value)).expect_err("unresolvable run[0] must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost-program"));
    }

    #[test]
    fn rejects_a_carried_executable_that_disagrees_with_resolution() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["executable"] =
            json!("/nix/store/other/bin/svc");
        let error = lower(&manifest_from(value)).expect_err("executable mismatch must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("resolve"));
    }

    fn reject_reason(value: Value) -> Rejection {
        prove_references(&manifest_from(value)).expect_err("references must not resolve")
    }

    #[test]
    fn invocations_must_reference_declared_codebases() {
        let mut value = manifest_value();
        value["tasks"]["t"]["invocation"]["codebaseId"] = json!("ghost");
        assert_eq!(
            reject_reason(value),
            undeclared("invocation.codebaseId", "ghost")
        );
    }

    #[test]
    fn closure_target_must_match_closure_system() {
        let mut value = manifest_value();
        value["closures"]["c"]["targetSystem"] = json!("aarch64-darwin");
        assert!(matches!(
            reject_reason(value),
            Rejection::ClosureTargetMismatch { .. }
        ));
    }

    #[test]
    fn lifecycle_operation_ids_must_be_globally_unique() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["health"]["operationId"] = json!("svc.start");
        assert_eq!(
            reject_reason(value),
            Rejection::DuplicateOperationId {
                id: "svc.start".to_string()
            }
        );
    }

    fn planned_services(document: &ValidatedManifest, task: &str) -> Vec<String> {
        let execution = lower(document).expect("graph inputs must admit");
        let plan = super::super::plan::plan(&execution, &TaskId::new(task), 0).unwrap();
        let mut services: Vec<_> = plan
            .services
            .iter()
            .map(|binding| binding.service.name.to_string())
            .collect();
        services.sort();
        services
    }

    /// Golden vector V4 (docs/DERIVATION_SPEC.md §5) on the runtime side:
    /// union of transitive leaf requires, closed over connectsTo, byte-sorted.
    #[test]
    fn services_required_derivation_matches_vector_v4() {
        let mut value = manifest_value();
        // svc gains a connectsTo dependency `dep`, so the task's derived union
        // closes over it.
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }

        value["services"]["dep"] = dep;
        value["services"]["svc"]["connectsTo"] = json!(["dep"]);

        let manifest = manifest_from(value);
        assert_eq!(planned_services(&manifest, "t"), vec!["dep", "svc"]);
    }

    #[test]
    fn executable_selection_vector_v5_tools_supply_path() {
        let mut value = manifest_value();
        value["closures"]["gitC"] = json!({
            "kind": "executable", "storePath": "/nix/store/git", "executable": "/nix/store/git/bin/git",
            "targetSystem": "x86_64-linux",
            "requiresExecutable": true, "effects": ["process"]
        });
        value["closures"]["probeC"] = json!({
            "kind": "executable", "storePath": "/nix/store/probe", "executable": "/nix/store/probe/bin/probe",
            "targetSystem": "x86_64-linux",
            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["t"]["invocation"]["tools"] = json!(["ct", "gitC"]);
        let mut probe_invocation =
            invocation_value("/nix/store/probe/bin/probe", json!(["probe", "ready"]));
        probe_invocation["tools"] = json!(["probeC"]);
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "exec",
            "invocation": probe_invocation,
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        let execution = lower(&manifest_from(value)).unwrap();
        let ExecutableTask::Leaf(task) = &execution.program.tasks["t"] else {
            panic!("leaf")
        };
        assert_eq!(task.exec.executable, "/nix/store/ct/bin/task");
        assert_eq!(
            task.exec.tool_roots,
            vec!["/nix/store/ct/bin", "/nix/store/git/bin"]
        );
        let Probe::Exec(probe) = &execution.services()["svc"].ready.probe else {
            panic!("exec probe")
        };
        assert_eq!(probe.exec.executable, "/nix/store/probe/bin/probe");
    }

    #[test]
    fn operation_identity_vector_v6_override_and_v7_multiple_leaves() {
        let mut value = manifest_value();

        value["tasks"]["odd"] = json!({
            "kind": "leaf",
            "operationId": "task.custom.odd",
            "invocation": {
                "tools": ["ct"],
                "run": ["task", "odd"],
                "executable": "/nix/store/ct/bin/task",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": [],

            "exitPolicy": { "successCodes": [0] }
        });
        let execution = lower(&manifest_from(value.clone())).unwrap();
        for id in ["t", "odd"] {
            let ExecutableTask::Leaf(task) = &execution.program.tasks[id] else {
                panic!("leaf")
            };
            assert_eq!(task.exec.executable, "/nix/store/ct/bin/task");
        }
        value["tasks"]["odd"]["operationId"] = json!("task.t.run");
        assert_eq!(
            reject_reason(value),
            Rejection::DuplicateOperationId {
                id: "task.t.run".into()
            }
        );
    }

    #[test]
    fn services_required_vector_v8_diamond_dedups() {
        let mut value = manifest_value();
        add_named_service(&mut value, "db", &[]);
        add_named_service(&mut value, "a", &["db"]);
        add_named_service(&mut value, "b", &["db"]);

        value["tasks"]["t"]["requires"] = json!(["a", "b"]);

        let manifest = manifest_from(value);
        assert_eq!(planned_services(&manifest, "t"), vec!["a", "b", "db"]);
        lower(&manifest).expect("diamond service graph should lower");
    }

    #[test]
    fn services_required_vector_v9_prepare_task_may_be_composite() {
        let mut value = manifest_value();
        add_named_service(&mut value, "dep", &[]);

        value["tasks"]["migrate"] = json!({
            "kind": "leaf",
            "operationId": "task.migrate.run",
            "invocation": {
                "tools": ["ct"],
                "run": ["task", "migrate", "--db", "${port:dep}"],
                "executable": "/nix/store/ct/bin/task",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["dep"],

            "exitPolicy": { "successCodes": [0] }
        });
        value["tasks"]["seed"] = json!({
            "kind": "leaf",
            "operationId": "task.seed.run",
            "invocation": {
                "tools": ["ct"],
                "run": ["task", "seed"],
                "executable": "/nix/store/ct/bin/task",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": [],

            "exitPolicy": { "successCodes": [0] }
        });
        value["tasks"]["prep"] = json!({
            "kind": "composite",
            "steps": {
                "migrate": { "task": "migrate" },
                "seed": { "task": "seed", "dependsOn": ["migrate"] }
            },

        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "prep" });

        let manifest = manifest_from(value);
        assert_eq!(planned_services(&manifest, "t"), vec!["dep", "svc"]);
        lower(&manifest).expect("composite prepare task should lower");
    }

    #[test]
    fn services_required_vector_v10_connects_to_fixpoint() {
        let mut value = manifest_value();
        add_named_service(&mut value, "api", &["worker"]);
        add_named_service(&mut value, "worker", &["db"]);
        add_named_service(&mut value, "db", &["cache"]);
        add_named_service(&mut value, "cache", &[]);

        value["tasks"]["t"]["requires"] = json!(["api"]);

        let manifest = manifest_from(value);
        assert_eq!(
            planned_services(&manifest, "t"),
            vec!["api", "cache", "db", "worker"]
        );
        lower(&manifest).expect("long connectsTo closure should lower");
    }

    #[test]
    fn service_less_task_lowers() {
        // A task may require zero services (e.g. a lint/test task) as long as it
        // does not reference a service-derived placeholder.
        let mut value = manifest_value();
        value["tasks"]["t"]["requires"] = json!([]);

        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "--check"]);
        let em = lower(&manifest_from(value)).expect("a service-less task lowers");
        assert!(em.leaf("t").unwrap().requires.is_empty());
    }

    #[test]
    fn service_less_task_using_port_is_rejected() {
        // The fixture task's run args carry "${port}"; with no service to resolve
        // it, admission must reject rather than admit-then-fail.
        let mut value = manifest_value();
        value["tasks"]["t"]["requires"] = json!([]);

        let error = lower(&manifest_from(value))
            .expect_err("a service-less task using ${port} must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
    }

    #[test]
    fn service_less_task_using_port_in_env_is_rejected() {
        // Env values substitute like args; a bare ${port} with no service would
        // otherwise run with the literal placeholder in the environment.
        let mut value = manifest_value();
        value["tasks"]["t"]["requires"] = json!([]);

        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "--check"]);
        value["tasks"]["t"]["invocation"]["env"] = json!({ "PORT": "${port}" });
        let error = lower(&manifest_from(value))
            .expect_err("a service-less task using ${port} in env must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
    }

    /// Rewrite the fixture's ready probe as an invocation probe bound to
    /// closure `c`.
    fn with_exec_ready_probe(value: &mut Value) {
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "exec",
            "invocation": invocation_value("/nix/store/c/bin/svc", json!(["svc", "ping"])),
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
    }

    #[test]
    fn exec_probe_lowers_to_exec_variant() {
        let mut value = manifest_value();
        with_exec_ready_probe(&mut value);
        let em = lower(&manifest_from(value)).expect("exec probe lowers");
        let svc = em.services().get("svc").expect("service lowered");
        let Probe::Exec(probe) = &svc.ready.probe else {
            panic!("ready probe should lower to the exec variant");
        };
        // Probe args are run[1..]; the probe's per-attempt timeout overrides the
        // invocation's own timeout.
        assert_eq!(probe.policy.max_attempts.get(), 5);
        assert!(matches!(svc.health.probe, Probe::Tcp(_)));
    }

    #[test]
    fn tcp_probe_with_invocation_is_rejected() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "tcp",
            "invocation": invocation_value("/nix/store/c/bin/svc", json!(["svc", "ping"])),
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        let error =
            lower(&manifest_from(value)).expect_err("tcp probe with invocation must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("tcp but carries an invocation"));
    }

    #[test]
    fn exec_probe_without_invocation_is_rejected() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "exec",
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        let error =
            lower(&manifest_from(value)).expect_err("exec probe without invocation must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("declares no invocation"));
    }

    #[test]
    fn exec_probe_named_ref_outside_connects_to_is_rejected() {
        let mut value = manifest_value();
        with_exec_ready_probe(&mut value);
        value["services"]["svc"]["lifecycle"]["ready"]["probe"]["invocation"]["run"] =
            json!(["svc", "--db", "${port:ghost}"]);
        let error = lower(&manifest_from(value)).expect_err("out-of-scope named ref must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn composite_task_lowers_with_resolved_steps() {
        let mut value = manifest_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": {
                "first": { "task": "t" },
                "second": { "task": "t", "dependsOn": ["first"] }
            }
        });
        let em = lower(&manifest_from(value)).expect("composite lowers");
        let ExecutableTask::Composite(composite) = &em.tasks()["pipeline"] else {
            panic!("composite lowered")
        };
        assert_eq!(composite.steps.len(), 2);
        assert_eq!(composite.steps[0].name, "first");
        assert_eq!(composite.steps[1].depends_on, vec!["first"]);
        assert!(em.leaf("pipeline").is_none());
        let plan = super::super::plan::plan(&em, &TaskId::new("pipeline"), 0).unwrap();
        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| node.node_id.as_str())
                .collect::<Vec<_>>(),
            ["pipeline.first", "pipeline.second"]
        );
    }

    #[test]
    fn composite_step_must_reference_a_declared_task() {
        let mut value = manifest_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": { "only": { "task": "ghost" } }
        });
        let error = lower(&manifest_from(value)).expect_err("dangling step task must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn composite_step_depends_on_must_name_a_sibling() {
        let mut value = manifest_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": { "only": { "task": "t", "dependsOn": ["ghost"] } }
        });
        let error = lower(&manifest_from(value)).expect_err("dangling dependsOn must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn invalid_leaf_shapes_are_rejected_before_lowering() {
        for (member, replacement, expected_field) in [
            ("invocation", json!(null), "tasks.invocation"),
            ("exitPolicy", json!(null), "tasks.exitPolicy"),
            ("steps", json!({"child": {"task": "t"}}), "tasks.steps"),
        ] {
            let mut value = manifest_value();
            value["tasks"]["t"][member] = replacement;
            let error =
                ValidatedManifest::try_from(serde_json::from_value::<Manifest>(value).unwrap())
                    .expect_err("invalid leaf cannot reach lowering");
            assert!(
                matches!(
                    error,
                    nixfied_manifest::ValidationError::UnsupportedValue { field, .. }
                        if field == expected_field
                ),
                "{member}: {error}"
            );
        }
    }

    #[test]
    fn lowering_rejects_cycles_before_exposing_execution() {
        for (steps, diagnostic) in [
            (
                json!({"again": {"task": "pipeline"}}),
                "task reference cycle",
            ),
            (
                json!({"a": {"task": "t", "dependsOn": ["b"]},
                    "b": {"task": "t", "dependsOn": ["a"]}}),
                "step dependency cycle",
            ),
        ] {
            let mut value = manifest_value();
            value["tasks"]["pipeline"] = json!({"kind": "composite", "steps": steps, });
            let error =
                lower(&manifest_from(value)).expect_err("lower must prove graph feasibility");
            assert!(error.message.contains(diagnostic), "{}", error.message);
        }
    }

    #[test]
    fn lowering_rejects_invalid_or_insufficient_windows() {
        for (placements, diagnostic) in [
            (json!({}), "exact slotPolicy range"),
            (
                json!({"0": {"slot": 0, "candidatePorts": {"start": 0, "end": 1}}}),
                "candidatePorts",
            ),
            (
                json!({"0": {"slot": 0, "candidatePorts": {"start": 2, "end": 1}}}),
                "candidatePorts",
            ),
            (
                json!({"0": {"slot": 0, "candidatePorts": {"start": 23080, "end": 23080}}}),
                "cannot host 2 endpoints",
            ),
        ] {
            let mut value = manifest_value();
            value["placement"]["slotPlacements"] = placements;
            value["services"]["svc"]["endpoints"]["second"] =
                json!({"endpointId": "second", "host": "127.0.0.1"});
            let raw: Manifest = serde_json::from_value(value).unwrap();
            match ValidatedManifest::try_from(raw) {
                Ok(document) => {
                    assert_eq!(diagnostic, "cannot host 2 endpoints");
                    let error = lower(&document).expect_err("lower owns capacity rejection");
                    assert!(error.message.contains(diagnostic), "{}", error.message);
                }
                Err(error) => {
                    assert_ne!(diagnostic, "cannot host 2 endpoints");
                    assert!(error.to_string().contains(diagnostic), "{error}");
                }
            }
        }
    }

    /// An endpoint-less sibling service `worker` with invocation probes; the
    /// closure binds its ops.
    fn with_endpoint_less_worker(value: &mut Value) {
        value["closures"]["cw"] = json!({
            "kind": "executable", "storePath": "/nix/store/cw", "executable": "/nix/store/cw/bin/worker",
            "targetSystem": "x86_64-linux",

            "requiresExecutable": true, "effects": ["process"]
        });
        let worker_invocation = |run: Value| {
            json!({
                "tools": ["cw"],
                "run": run,
                "executable": "/nix/store/cw/bin/worker",
                "env": {},
                "codebaseId": "main",
                "cwd": ".",
                "stdin": "null",
                "timeoutMs": 1000
            })
        };
        let probe = json!({
            "kind": "exec",
            "invocation": worker_invocation(json!(["worker", "ping"])),
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        value["services"]["worker"] = json!({
            "lifecycle": {
                "start": {
                    "operationId": "worker.start",
                    "invocation": worker_invocation(json!(["worker", "consume"])),
                    "terminal": { "success": "spawned", "failure": "failed" }
                },
                "ready": { "operationId": "worker.ready", "probe": probe.clone(), "terminal": { "success": "ready", "failure": "not-ready" } },
                "health": { "operationId": "worker.health", "probe": probe, "terminal": { "success": "healthy", "failure": "unhealthy" } },
                "stop": { "operationId": "worker.stop", "signal": "TERM", "timeoutMs": 5000, "terminal": { "success": "stopped", "failure": "failed" } },
                "clean": { "operationId": "worker.clean", "terminal": { "success": "cleaned", "failure": "failed" } }
            },
            "connectsTo": [],
            "stateRefs": [], "logRefs": [],
            "containment": "process-group"
        });
    }

    #[test]
    fn endpoint_less_service_lowers_without_a_primary() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        let em = lower(&manifest_from(value)).expect("endpoint-less service lowers");
        let worker = em.services().get("worker").expect("worker lowered");
        assert!(worker.endpoints.is_empty());
        assert!(worker.primary_endpoint.is_none());
    }

    #[test]
    fn endpoint_less_tcp_probe_is_rejected() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        value["services"]["worker"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "tcp", "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        // The tcp probe carries no invocation, so the closure's derived

        let error =
            lower(&manifest_from(value)).expect_err("tcp probe without endpoint must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("endpoint-less"), "{}", error.message);
    }

    #[test]
    fn endpoint_less_bare_placeholder_is_rejected() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        value["services"]["worker"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["worker", "consume", "--listen", "${port}"]);
        let error = lower(&manifest_from(value)).expect_err("bare placeholder must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(
            error.message.contains("declares no endpoint"),
            "{}",
            error.message
        );
    }

    #[test]
    fn named_ref_toward_endpoint_less_service_is_rejected() {
        // svc connectsTo worker (legal: ordering + derivation) but addresses
        // it (illegal: no addressability claim exists).
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        value["services"]["svc"]["connectsTo"] = json!(["worker"]);

        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "serve", "--peer", "${port:worker}"]);
        let error =
            lower(&manifest_from(value)).expect_err("named ref toward endpoint-less must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert_eq!(
            error.message,
            "service svc references the endpoint of worker without declaring it in own endpoints or addressable connectsTo"
        );
    }

    #[test]
    fn task_named_ref_toward_endpoint_less_requirement_is_rejected() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        value["tasks"]["t"]["requires"] = json!(["svc", "worker"]);

        value["tasks"]["t"]["invocation"]["run"] =
            json!(["task", "--port", "${port}", "--peer", "${port:worker}"]);
        let error =
            lower(&manifest_from(value)).expect_err("named ref toward endpoint-less must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert_eq!(
            error.message,
            "task t references the endpoint of worker without declaring it in addressable requires"
        );
    }

    #[test]
    fn endpoint_less_requirement_stays_legal_without_placeholders() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        value["tasks"]["t"]["requires"] = json!(["svc", "worker"]);

        let em = lower(&manifest_from(value)).expect("requiring an endpoint-less service is legal");
        assert_eq!(em.leaf("t").unwrap().requires.len(), 2);
    }

    #[test]
    fn listening_service_start_closure_must_declare_network_listener() {
        let mut value = manifest_value();
        value["closures"]["c"]["effects"] = json!(["process"]);
        let error = lower(&manifest_from(value)).expect_err("missing network-listener must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(
            error.message.contains("network-listener"),
            "{}",
            error.message
        );
    }

    #[test]
    fn endpoint_less_start_closure_must_not_declare_network_listener() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);
        value["closures"]["cw"]["effects"] = json!(["process", "network-listener"]);
        let error = lower(&manifest_from(value)).expect_err("unreservable listener must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(
            error.message.contains("network-listener"),
            "{}",
            error.message
        );
    }

    #[test]
    fn prepare_task_must_be_declared() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "ghost" });
        let error = lower(&manifest_from(value)).expect_err("dangling prepare task must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"), "{}", error.message);
    }

    #[test]
    fn prepare_task_reference_lowers_and_widens_the_union() {
        // Cross-service prepare: svc's prepare task requires dep, so starting
        // svc pulls dep into every union that contains svc.
        let mut value = manifest_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }

        value["services"]["dep"] = dep;
        value["closures"]["cm"] = json!({
            "kind": "executable", "storePath": "/nix/store/cm", "executable": "/nix/store/cm/bin/migrate",
            "targetSystem": "x86_64-linux",

            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["migrate"] = json!({
            "kind": "leaf",
            "operationId": "task.migrate.run",
            "invocation": {
                "tools": ["cm"],
                "run": ["migrate", "--db", "${port:dep}"],
                "executable": "/nix/store/cm/bin/migrate",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["dep"],

            "exitPolicy": { "successCodes": [0] }
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "migrate" });
        // Task t requires svc; svc's prepare requires dep -> union closes over it.

        let em = lower(&manifest_from(value)).expect("cross-service prepare lowers");
        assert_eq!(
            em.services()
                .get("svc")
                .unwrap()
                .prepare
                .as_ref()
                .map(|t| t.as_str()),
            Some("migrate")
        );
    }

    #[test]
    fn heterogeneous_cycle_error_names_each_edge_kind() {
        // svc -[prepare requires]-> dep (via the migrate task) and
        // dep -[connectsTo]-> svc: the rejection must render the cycle with
        // each hop's kind, so the operator can tell wiring from preparation.
        let mut value = manifest_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }

        dep["connectsTo"] = json!(["svc"]);
        value["services"]["dep"] = dep;
        value["closures"]["cm"] = json!({
            "kind": "executable", "storePath": "/nix/store/cm", "executable": "/nix/store/cm/bin/migrate",
            "targetSystem": "x86_64-linux",

            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["migrate"] = json!({
            "kind": "leaf",
            "operationId": "task.migrate.run",
            "invocation": {
                "tools": ["cm"],
                "run": ["migrate"],
                "executable": "/nix/store/cm/bin/migrate",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["dep"],

            "exitPolicy": { "successCodes": [0] }
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "migrate" });
        let error = lower(&manifest_from(value)).expect_err("heterogeneous cycle must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(
            error.message.contains("-[prepare requires]->"),
            "must name the prepare edge: {}",
            error.message
        );
        assert!(
            error.message.contains("-[connectsTo]->"),
            "must name the wiring edge: {}",
            error.message
        );
    }

    #[test]
    fn prepare_requiring_the_owner_is_a_self_cycle() {
        let mut value = manifest_value();
        value["closures"]["cm"] = json!({
            "kind": "executable", "storePath": "/nix/store/cm", "executable": "/nix/store/cm/bin/selfinit",
            "targetSystem": "x86_64-linux",

            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["selfinit"] = json!({
            "kind": "leaf",
            "operationId": "task.selfinit.run",
            "invocation": {
                "tools": ["cm"],
                "run": ["selfinit"],
                "executable": "/nix/store/cm/bin/selfinit",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["svc"],

            "exitPolicy": { "successCodes": [0] }
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "selfinit" });
        let error = lower(&manifest_from(value)).expect_err("self-preparing service must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(
            error.message.contains("svc -[prepare requires]-> svc"),
            "{}",
            error.message
        );
    }

    #[test]
    fn declared_path_env_is_rejected() {
        // PATH is runtime-owned (assembled from tool roots); a declared PATH
        // would be silently overwritten, so it is rejected fail-closed.
        let mut value = manifest_value();
        value["tasks"]["t"]["invocation"]["env"] = json!({ "PATH": "/usr/bin" });
        let error = lower(&manifest_from(value)).expect_err("declared PATH must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("PATH"));
    }

    #[test]
    fn task_named_placeholders_use_service_ids_not_endpoint_ids() {
        let mut value = manifest_value();
        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "${host:svc}", "${port:svc}"]);
        lower(&manifest_from(value.clone())).expect("direct required service is addressable");
        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "${port:svc-tcp}"]);
        let error =
            lower(&manifest_from(value)).expect_err("endpoint id is not a task service reference");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("svc-tcp"));
    }

    #[test]
    fn task_bare_placeholders_do_not_skip_endpoint_less_first_requirement() {
        let mut value = manifest_value();
        with_endpoint_less_worker(&mut value);

        value["tasks"]["t"]["requires"] = json!(["svc", "worker"]);
        lower(&manifest_from(value.clone()))
            .expect("addressable first dependency accepts bare port");
        value["tasks"]["t"]["requires"] = json!(["worker", "svc"]);
        let error =
            lower(&manifest_from(value)).expect_err("first dependency cannot be skipped or sorted");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("${port}"));
    }

    #[test]
    fn task_named_ref_outside_requires_is_rejected() {
        let mut value = manifest_value();
        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "--db", "${port:ghost}"]);
        let error = lower(&manifest_from(value)).expect_err("out-of-scope named ref must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn service_named_ref_outside_connects_to_is_rejected() {
        let mut value = manifest_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "--db", "${port:ghost}"]);
        let error = lower(&manifest_from(value)).expect_err("out-of-scope named ref must reject");
        assert_eq!(error.code, ErrorCode::ManifestAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn named_ref_inside_connects_to_lowers_with_env() {
        let mut value = manifest_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }

        value["services"]["dep"] = dep;
        value["services"]["svc"]["connectsTo"] = json!(["dep"]);
        // Task t requires svc; svc now connectsTo dep, so the derived union
        // closes over it.

        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "serve", "--db", "${host:dep}:${port:dep}"]);
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"] =
            json!({ "DB_URL": "tcp://${host:dep}:${port:dep}" });
        let em = lower(&manifest_from(value)).expect("declared named refs lower");
        let svc = em.services().get("svc").expect("service lowered");
        assert_eq!(svc.connects_to, vec![ServiceId::new("dep")]);
    }
}
