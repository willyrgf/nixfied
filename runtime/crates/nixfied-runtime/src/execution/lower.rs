//! The total lowering `Model -> ExecutionModel`.
//!
//! Every source struct is destructured with **no `..`**, so adding a field to the
//! schema is a compile error here until the lowering consciously maps or refuses
//! it. Rejections are `ModelAdmission` errors — the model was inexpressible to the
//! runtime — never execution failures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use nixfied_model::{
    ClosureSpec, Environment, InvocationSpec, Lifecycle, Model, OperationId, ProbeKind, ProbeSpec,
    ServiceId, ServiceSpec, StatePolicy, StepSpec, StopSpec, Target, TaskId, TaskKind, TaskSpec,
    TerminalSemantics,
};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::types::*;

/// Lower a validated model into the executor's input. The result contains only
/// what the runtime can execute; anything it cannot is rejected here.
pub fn lower(model: &Model) -> RuntimeResult<ExecutionModel> {
    // First prove every cross-reference resolves; the structural map below then
    // builds the executor input from references known to exist.
    prove_references(model)?;
    // Exhaustive destructure — no `..`. Static-configuration fields validated by
    // `Model::validate` are bound and intentionally ignored; the executable
    // fields are mapped below. A new schema field breaks this pattern (E0027).
    let Model {
        model_version: _,
        toolchain_id: _,
        runtime_abi: _,
        generator: _,
        project: _,
        target: _,
        codebases: _,
        environments,
        slot_policy: _,
        placement,
        state,
        closures,
        services,
        tasks,
        docs: _,
    } = model;

    let lowered_services = services
        .iter()
        .map(|(name, service)| {
            Ok((
                ServiceId::new(name),
                lower_service(name, service, closures, state, &model.target)?,
            ))
        })
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    let mut lowered_tasks = BTreeMap::new();
    let mut lowered_composites = BTreeMap::new();
    let task_ids = tasks.keys().map(String::as_str).collect::<BTreeSet<_>>();
    for (id, task) in tasks {
        match lower_task(id, task, closures, &lowered_services, &task_ids)? {
            LoweredTask::Leaf(leaf) => {
                lowered_tasks.insert(TaskId::new(id), leaf);
            }
            LoweredTask::Composite(composite) => {
                lowered_composites.insert(TaskId::new(id), composite);
            }
        }
    }

    // Resolve each environment reference against the maps just built, so every
    // id the executor later follows is a handle proven to resolve. A miss is an
    // admission rejection here, not a runtime `None`.
    let environment = match environments.values().next() {
        Some(environment) => lower_environment(
            environment,
            &lowered_services,
            &lowered_tasks,
            &lowered_composites,
        )?,
        None => return Err(Rejection::NoEnvironment.into()),
    };

    let mut slot_windows = BTreeMap::new();
    for slot_placement in placement.slot_placements.values() {
        slot_windows.insert(
            slot_placement.slot,
            PortWindow {
                start: slot_placement.candidate_ports.start,
                end: slot_placement.candidate_ports.end,
            },
        );
    }

    Ok(ExecutionModel {
        services: lowered_services,
        tasks: lowered_tasks,
        composites: lowered_composites,
        environment,
        slot_windows,
    })
}

fn lower_environment(
    environment: &Environment,
    services: &BTreeMap<ServiceId, ExecService>,
    tasks: &BTreeMap<TaskId, ExecTask>,
    composites: &BTreeMap<TaskId, ExecComposite>,
) -> RuntimeResult<ExecEnvironment> {
    let Environment {
        services: env_services,
        tasks: env_tasks,
    } = environment;
    let resolved_services = env_services
        .iter()
        .map(|id| require_service("environment.services", services, id))
        .collect::<Result<Vec<_>, Rejection>>()?;
    // An environment task may be a leaf or a composite; the planner flattens
    // composites onto the run plan with stable step paths.
    let resolved_tasks = env_tasks
        .iter()
        .map(|id| {
            if tasks.contains_key(id) || composites.contains_key(id) {
                Ok(id.clone())
            } else {
                Err(undeclared("environment.tasks", id.as_str()))
            }
        })
        .collect::<Result<Vec<_>, Rejection>>()?;
    require_connects_to_closed("environment.services", &resolved_services, services)?;
    Ok(ExecEnvironment {
        services: resolved_services,
        tasks: resolved_tasks,
    })
}

/// Every started service's `connectsTo` dependency must itself be started by
/// the same selection, or its named endpoint placeholders would have no
/// slot-plan entry to resolve against.
fn require_connects_to_closed(
    scope: &'static str,
    selected: &[ServiceId],
    services: &BTreeMap<ServiceId, ExecService>,
) -> RuntimeResult<()> {
    for id in selected {
        let Some(service) = services.get(id) else {
            continue;
        };
        for target in &service.connects_to {
            if !selected.contains(target) {
                return Err(Rejection::ConnectsToNotStarted {
                    scope,
                    service: id.as_str().to_string(),
                    target: target.as_str().to_string(),
                }
                .into());
            }
        }
    }
    Ok(())
}

/// Resolve a service reference to a handle proven to exist among the lowered
/// services; the returned `ServiceId` is the proof.
fn require_service(
    kind: &'static str,
    services: &BTreeMap<ServiceId, ExecService>,
    id: &ServiceId,
) -> Result<ServiceId, Rejection> {
    if services.contains_key(id) {
        Ok(id.clone())
    } else {
        Err(undeclared(kind, id.as_str()))
    }
}

fn lower_service(
    name: &str,
    service: &ServiceSpec,
    closures: &BTreeMap<String, ClosureSpec>,
    state: &StatePolicy,
    target: &Target,
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

    let endpoints: BTreeMap<String, ResolvedEndpoint> = endpoints
        .iter()
        .map(|(id, endpoint)| {
            (
                id.clone(),
                ResolvedEndpoint {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    host: endpoint.host,
                },
            )
        })
        .collect();

    let owner = || format!("service {name}");
    let prepare = PrepareOp {
        meta: op_meta(&prepare.operation_id, &prepare.terminal),
        exec: match &prepare.invocation {
            Some(invocation) => Some(resolve_invocation(&owner, invocation, closures)?),
            None => None,
        },
    };
    let start = StartOp {
        meta: op_meta(&start.operation_id, &start.terminal),
        exec: resolve_invocation(&owner, &start.invocation, closures)?,
    };
    let ready = ReadyOp {
        meta: op_meta(&ready.operation_id, &ready.terminal),
        probe: lower_probe(name, "ready", &ready.probe, closures)?,
    };
    let health = HealthOp {
        meta: op_meta(&health.operation_id, &health.terminal),
        probe: lower_probe(name, "health", &health.probe, closures)?,
    };
    let stop = lower_stop(stop);
    let clean = CleanOp {
        meta: op_meta(&clean.operation_id, &clean.terminal),
    };

    // Named endpoint placeholders `${port:<name>}` in lifecycle invocation
    // args/env (including probe invocations) resolve against the service's own
    // endpoint ids or its declared connectsTo dependencies; reject any other
    // reference here rather than fail (or silently leak the literal placeholder)
    // at execution. Validation already proved own endpoint ids and connectsTo
    // ids are disjoint.
    let allowed: BTreeSet<&str> = endpoints
        .keys()
        .map(String::as_str)
        .chain(connects_to.iter().map(|id| id.as_str()))
        .collect();
    for exec in prepare
        .exec
        .iter()
        .chain(std::iter::once(&start.exec))
        .chain(probe_exec(&ready.probe))
        .chain(probe_exec(&health.probe))
    {
        require_named_refs_in_scope(&owner, "own endpoints or connectsTo", exec, &allowed)?;
    }

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
        containment: containment.clone(),
        identity: crate::service::identity::compute_service_identity(service, state, target),
    })
}

fn lower_stop(stop: &StopSpec) -> StopOp {
    StopOp {
        meta: op_meta(&stop.operation_id, &stop.terminal),
        signal: StopSignal::from(stop.signal),
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

/// A lowered task: the executor's leaf, or a composite for the planner to
/// flatten.
enum LoweredTask {
    Leaf(ExecTask),
    Composite(ExecComposite),
}

fn lower_task(
    task_id: &str,
    task: &TaskSpec,
    closures: &BTreeMap<String, ClosureSpec>,
    services: &BTreeMap<ServiceId, ExecService>,
    task_ids: &BTreeSet<&str>,
) -> RuntimeResult<LoweredTask> {
    let TaskSpec {
        kind,
        operation_id: _,
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
            return Ok(LoweredTask::Composite(lower_composite(
                task_id, steps, task_ids,
            )?));
        }
        TaskKind::Leaf => {}
    }
    let owner = || format!("task {task_id}");
    // Kind/field coherence is validated structurally; re-prove it here so the
    // lowering is total on any deserialized model (fail closed).
    let Some(invocation) = invocation else {
        return Err(Rejection::TaskKindIncoherent {
            task_id: task_id.to_string(),
            expected: "a leaf task carries an invocation",
        }
        .into());
    };
    let Some(exit_policy) = exit_policy else {
        return Err(Rejection::TaskKindIncoherent {
            task_id: task_id.to_string(),
            expected: "a leaf task carries an exit policy",
        }
        .into());
    };
    if !steps.is_empty() {
        return Err(Rejection::TaskKindIncoherent {
            task_id: task_id.to_string(),
            expected: "a leaf task carries no steps",
        }
        .into());
    }
    let exec = resolve_invocation(&owner, invocation, closures)?;
    let requires = requires
        .iter()
        .map(|id| require_service("task.requires", services, id))
        .collect::<Result<Vec<_>, Rejection>>()?;
    // `${port}`/`${host}` resolve from the task's primary service. A task that
    // requires no services has no endpoint, so referencing them is unrunnable —
    // reject it here rather than fail at execution.
    if requires.is_empty() {
        for placeholder in ["${port}", "${host}"] {
            if exec
                .args
                .iter()
                .chain(exec.env.values())
                .any(|value| value.contains(placeholder))
            {
                return Err(Rejection::TaskPlaceholderWithoutService {
                    task_id: task_id.to_string(),
                    placeholder,
                }
                .into());
            }
        }
    }
    // Named endpoint placeholders may only reference declared service
    // requirements (any of them, not just the primary).
    let allowed: BTreeSet<&str> = requires.iter().map(|id| id.as_str()).collect();
    require_named_refs_in_scope(&owner, "requires", &exec, &allowed)?;
    Ok(LoweredTask::Leaf(ExecTask {
        task_id: TaskId::new(task_id),
        exec,
        requires,
        success_codes: exit_policy.success_codes.iter().copied().collect(),
    }))
}

/// Lower a composite body: every step references a declared task (leaf or
/// composite) and `dependsOn` names sibling steps. Acyclicity through nesting
/// is proven by the planner's flattening, which admission runs for every
/// slot/selection.
fn lower_composite(
    task_id: &str,
    steps: &BTreeMap<String, StepSpec>,
    task_ids: &BTreeSet<&str>,
) -> Result<ExecComposite, Rejection> {
    if steps.is_empty() {
        return Err(Rejection::TaskKindIncoherent {
            task_id: task_id.to_string(),
            expected: "a composite task carries at least one step",
        });
    }
    let step_names = steps.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let lowered = steps
        .iter()
        .map(|(name, step)| {
            if !task_ids.contains(step.task.as_str()) {
                return Err(undeclared("task.steps.task", step.task.as_str()));
            }
            let depends_on = step
                .depends_on
                .iter()
                .map(|dependency| {
                    if step_names.contains(dependency.as_str()) {
                        Ok(dependency.clone())
                    } else {
                        Err(undeclared("task.steps.dependsOn", dependency.as_str()))
                    }
                })
                .collect::<Result<Vec<_>, Rejection>>()?;
            Ok(ExecStep {
                name: name.clone(),
                task: step.task.clone(),
                depends_on,
            })
        })
        .collect::<Result<Vec<_>, Rejection>>()?;
    Ok(ExecComposite {
        task_id: TaskId::new(task_id),
        steps: lowered,
    })
}

/// Lower the kind-discriminated wire probe into the executor's closed enum,
/// proving kind/field coherence: a tcp probe must not carry an invocation, an
/// exec probe must carry one. The probe's own timing governs every attempt —
/// the invocation's own timeout is overridden.
fn lower_probe(
    service: &str,
    class: &'static str,
    probe: &ProbeSpec,
    closures: &BTreeMap<String, ClosureSpec>,
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
    let max_attempts = max_attempts.get();
    match kind {
        ProbeKind::Tcp => {
            if invocation.is_some() {
                return Err(Rejection::ProbeExecOnTcp {
                    service: service.to_string(),
                    class,
                }
                .into());
            }
            Ok(Probe::Tcp(TcpProbe {
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
            let mut exec = resolve_invocation(&owner, invocation, closures)?;
            exec.timeout = timeout;
            Ok(Probe::Exec(ExecProbe {
                label: class.to_string(),
                exec,
                timeout,
                retry_interval,
                max_attempts,
            }))
        }
    }
}

fn probe_exec(probe: &Probe) -> Option<&ResolvedInvocation> {
    match probe {
        Probe::Exec(probe) => Some(&probe.exec),
        Probe::Tcp(_) => None,
    }
}

/// The declarative run[0] resolution rule (docs/DERIVATION_SPEC.md §1.1): the
/// first tool closure whose declared executable basename equals `run[0]`
/// provides the executable. No filesystem scan, so eval and admission derive
/// the same answer from the same declarations.
fn executable_closure<'a>(
    invocation: &InvocationSpec,
    closures: &'a BTreeMap<String, ClosureSpec>,
) -> Option<(&'a String, &'a ClosureSpec)> {
    let program = invocation.run.first()?;
    invocation.tools.iter().find_map(|tool| {
        let (id, closure) = closures.get_key_value(tool.as_str())?;
        let basename = Path::new(&closure.executable).file_name()?;
        (basename.to_str() == Some(program.as_str())).then_some((id, closure))
    })
}

/// Resolve an inline invocation against the declared closures: every tool must
/// be a declared closure, run[0] must resolve per the derivation spec, and the
/// carried `executable` must equal that resolution (fail closed). The PATH
/// roots — each tool executable's parent directory, in declared order — are
/// carried for the runtime's child-PATH assembly.
fn resolve_invocation(
    owner: &impl Fn() -> String,
    invocation: &InvocationSpec,
    closures: &BTreeMap<String, ClosureSpec>,
) -> Result<ResolvedInvocation, Rejection> {
    let InvocationSpec {
        tools,
        run,
        executable,
        env,
        codebase_id: _,
        cwd,
        stdin,
        timeout_ms,
    } = invocation;
    let Some(program) = run.first() else {
        return Err(Rejection::RunUnresolvable {
            owner: owner(),
            program: String::new(),
        });
    };
    // PATH is runtime-owned: it is assembled from the tool roots at spawn, so a
    // declared PATH would be silently overwritten — reject it instead.
    if env.contains_key("PATH") {
        return Err(Rejection::ReservedEnvVar {
            owner: owner(),
            name: "PATH",
        });
    }
    let mut tool_roots = Vec::with_capacity(tools.len());
    for tool in tools.iter() {
        let Some(closure) = closures.get(tool.as_str()) else {
            return Err(undeclared("invocation.tools", tool.as_str()));
        };
        let root = Path::new(&closure.executable)
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        tool_roots.push(root);
    }
    let Some((_, resolved)) = executable_closure(invocation, closures) else {
        return Err(Rejection::RunUnresolvable {
            owner: owner(),
            program: program.clone(),
        });
    };
    if resolved.executable != *executable {
        return Err(Rejection::ExecutableMismatch {
            owner: owner(),
            carried: executable.clone(),
            resolved: resolved.executable.clone(),
        });
    }
    Ok(ResolvedInvocation {
        executable: executable.clone(),
        args: run[1..].to_vec(),
        env: env.clone(),
        cwd: cwd.clone(),
        stdin: *stdin,
        timeout: Duration::from_millis(timeout_ms.get()),
        tool_roots,
    })
}

/// The closed set of reasons the model cannot be lowered into an executable
/// program. Every relational/reference check the runtime needs lives here, so a
/// successful `lower` is a proof the references resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    NoEnvironment,
    UndeclaredReference {
        kind: &'static str,
        id: String,
    },
    DuplicateOperationId {
        id: String,
    },
    UnknownOperationBinding {
        binding: String,
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
    TaskKindIncoherent {
        task_id: String,
        expected: &'static str,
    },
    TaskPlaceholderWithoutService {
        task_id: String,
        placeholder: &'static str,
    },
    PlaceholderOutOfScope {
        owner: String,
        scope: &'static str,
        service: String,
    },
    ConnectsToNotStarted {
        scope: &'static str,
        service: String,
        target: String,
    },
    ProbeExecOnTcp {
        service: String,
        class: &'static str,
    },
    ProbeExecMissing {
        service: String,
        class: &'static str,
    },
    OperationUnbound {
        operation_id: String,
        closure_id: String,
    },
}

impl Rejection {
    fn message(&self) -> String {
        match self {
            Rejection::NoEnvironment => "model declares no environment".to_string(),
            Rejection::UndeclaredReference { kind, id } => {
                format!("{kind} references undeclared {id}")
            }
            Rejection::DuplicateOperationId { id } => {
                format!("operation id {id} is declared more than once")
            }
            Rejection::UnknownOperationBinding { binding } => {
                format!("closure binds undeclared operation {binding}")
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
            Rejection::TaskKindIncoherent { task_id, expected } => {
                format!("task {task_id} is kind-incoherent: {expected}")
            }
            Rejection::TaskPlaceholderWithoutService {
                task_id,
                placeholder,
            } => format!(
                "task {task_id} references {placeholder} but requires no service to resolve it"
            ),
            Rejection::PlaceholderOutOfScope {
                owner,
                scope,
                service,
            } => format!(
                "{owner} references the endpoint of {service} without declaring it in {scope}"
            ),
            Rejection::ConnectsToNotStarted {
                scope,
                service,
                target,
            } => format!("{scope} starts {service} but not its connectsTo dependency {target}"),
            Rejection::ProbeExecOnTcp { service, class } => {
                format!("service {service} {class} probe is tcp but carries an invocation")
            }
            Rejection::ProbeExecMissing { service, class } => {
                format!("service {service} {class} probe is exec but declares no invocation")
            }
            Rejection::OperationUnbound {
                operation_id,
                closure_id,
            } => {
                format!(
                    "operation {operation_id} is not bound by its invocation's closure {closure_id}"
                )
            }
        }
    }
}

impl From<Rejection> for RuntimeError {
    fn from(rejection: Rejection) -> Self {
        RuntimeError::new(ErrorCode::ModelAdmission, rejection.message())
    }
}

/// Service ids referenced by named endpoint placeholders (`${port:<id>}` /
/// `${host:<id>}`) in one invocation value.
pub(crate) fn named_endpoint_refs(value: &str) -> Vec<&str> {
    let mut refs = Vec::new();
    for prefix in ["${port:", "${host:"] {
        let mut rest = value;
        while let Some(start) = rest.find(prefix) {
            rest = &rest[start + prefix.len()..];
            let Some(end) = rest.find('}') else { break };
            refs.push(&rest[..end]);
            rest = &rest[end..];
        }
    }
    refs
}

fn require_named_refs_in_scope(
    owner: &impl Fn() -> String,
    scope: &'static str,
    exec: &ResolvedInvocation,
    allowed: &BTreeSet<&str>,
) -> RuntimeResult<()> {
    for value in exec.args.iter().chain(exec.env.values()) {
        for reference in named_endpoint_refs(value) {
            if !allowed.contains(reference) {
                return Err(Rejection::PlaceholderOutOfScope {
                    owner: owner(),
                    scope,
                    service: reference.to_string(),
                }
                .into());
            }
        }
    }
    Ok(())
}

fn undeclared(kind: &'static str, id: impl Into<String>) -> Rejection {
    Rejection::UndeclaredReference {
        kind,
        id: id.into(),
    }
}

/// Every invocation position in the model with its operation id, in canonical
/// order: task leaves, then each service's prepare/start/ready/health.
fn invocation_positions(model: &Model) -> Vec<(&OperationId, &InvocationSpec)> {
    let mut positions = Vec::new();
    for service in model.services.values() {
        let lifecycle = &service.lifecycle;
        if let Some(invocation) = &lifecycle.prepare.invocation {
            positions.push((&lifecycle.prepare.operation_id, invocation));
        }
        positions.push((&lifecycle.start.operation_id, &lifecycle.start.invocation));
        for (probe, operation_id) in [
            (&lifecycle.ready.probe, &lifecycle.ready.operation_id),
            (&lifecycle.health.probe, &lifecycle.health.operation_id),
        ] {
            // A tcp probe carrying an invocation is incoherent; `lower_probe`
            // rejects it with the precise probe-kind error, so it is not an
            // invocation position here.
            if probe.kind != ProbeKind::Exec {
                continue;
            }
            if let Some(invocation) = &probe.invocation {
                positions.push((operation_id, invocation));
            }
        }
    }
    for task in model.tasks.values() {
        if let (Some(operation_id), Some(invocation)) = (&task.operation_id, &task.invocation) {
            positions.push((operation_id, invocation));
        }
    }
    positions
}

/// Prove the *relational* invariants the per-reference resolver in `lower` cannot
/// express on its own: every invocation references a declared codebase, closures
/// match the target system, lifecycle/task operation ids are globally unique, an
/// environment task requires only services that program starts, and
/// closure operation bindings name a declared operation. The single-reference
/// existence checks (tool/service/task/node ids) are discharged where they are
/// consumed — `lower` resolves each into a typed handle, so a dangling reference
/// is rejected there. Acyclicity is proven separately by the planner.
fn prove_references(model: &Model) -> Result<(), Rejection> {
    let codebase_ids = model
        .codebases
        .iter()
        .map(|codebase| codebase.codebase_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut declared_operations = BTreeSet::new();

    for (_, invocation) in invocation_positions(model) {
        if !codebase_ids.contains(invocation.codebase_id.as_str()) {
            return Err(undeclared(
                "invocation.codebaseId",
                invocation.codebase_id.as_str(),
            ));
        }
    }

    for (closure_id, closure) in &model.closures {
        if closure.target_system != model.target.closure_system {
            return Err(Rejection::ClosureTargetMismatch {
                closure_id: closure_id.clone(),
                target_system: closure.target_system.clone(),
                closure_system: model.target.closure_system.clone(),
            });
        }
    }

    // Operation ids are globally unique across every lifecycle and task; the
    // declared set also anchors the closure binding check below.
    for service in model.services.values() {
        for operation_id in lifecycle_op_ids(&service.lifecycle) {
            if !declared_operations.insert(operation_id) {
                return Err(Rejection::DuplicateOperationId {
                    id: operation_id.to_string(),
                });
            }
        }
    }
    for operation_id in model
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

    // An environment task can only require services the environment starts, or
    // the run fails mid-flight on a missing dependency.
    for env in model.environments.values() {
        let env_services = env
            .services
            .iter()
            .map(|service| service.as_str())
            .collect::<BTreeSet<_>>();
        for task_id in &env.tasks {
            if let Some(task) = model.tasks.get(task_id.as_str()) {
                for service in &task.requires {
                    if !env_services.contains(service.as_str()) {
                        return Err(undeclared("environment.task.requires", service.as_str()));
                    }
                }
            }
        }
    }

    for closure in model.closures.values() {
        for binding in &closure.operation_bindings {
            if !declared_operations.contains(binding.as_str()) {
                return Err(Rejection::UnknownOperationBinding {
                    binding: binding.to_string(),
                });
            }
        }
    }

    // Every operation that runs through an invocation runs through a closure:
    // the closure providing the resolved run[0] must explicitly bind that
    // operation, or the model is executing a closure for an operation it never
    // declared. Dangling tool references are rejected by the per-reference
    // resolver, so only the binding coverage is proven here.
    for (operation_id, invocation) in invocation_positions(model) {
        let Some((closure_id, closure)) = executable_closure(invocation, &model.closures) else {
            continue;
        };
        if !closure
            .operation_bindings
            .iter()
            .any(|binding| binding.as_str() == operation_id.as_str())
        {
            return Err(Rejection::OperationUnbound {
                operation_id: operation_id.to_string(),
                closure_id: closure_id.clone(),
            });
        }
    }

    Ok(())
}

/// The operation id of each lifecycle op, in canonical order.
fn lifecycle_op_ids(lifecycle: &Lifecycle) -> [&str; 6] {
    [
        lifecycle.prepare.operation_id.as_str(),
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

    fn model_value() -> Value {
        json!({
            "modelVersion": 1,
            "toolchainId": "nixfied-toolchain:1",
            "runtimeAbi": "nixfied-runtime-abi:1",
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
            "environments": { "dev": { "services": ["svc"], "tasks": ["t"] } },
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
                "markerIdentity": "nixfied-state", "stateEpoch": "1",
                "cleanupPolicy": "delete-on-clean", "persistence": "run-scoped"
            },
            "closures": {
                "c": {
                    "kind": "executable", "storePath": "/nix/store/c", "executable": "/nix/store/c/bin/svc",
                    "targetSystem": "x86_64-linux",
                    "operationBindings": ["svc.start", "task.t.run"],
                    "requiresExecutable": true, "effects": ["process"]
                },
                "ct": {
                    "kind": "executable", "storePath": "/nix/store/ct", "executable": "/nix/store/ct/bin/task",
                    "targetSystem": "x86_64-linux",
                    "operationBindings": ["task.t.run"],
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
            "docs": { "title": "t", "summary": "s" }
        })
    }

    fn service_value() -> Value {
        json!({
            "lifecycle": {
                "prepare": { "operationId": "svc.prepare", "terminal": { "success": "prepared", "failure": "failed" } },
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

    fn model_from(value: Value) -> Model {
        serde_json::from_value(value).expect("fixture should deserialize")
    }

    #[test]
    fn lowers_a_valid_model() {
        let em = lower(&model_from(model_value())).expect("valid model lowers");
        let svc = em.services.get("svc").expect("service lowered");
        assert_eq!(svc.endpoints["svc-tcp"].host.to_string(), "127.0.0.1");
        assert_eq!(svc.primary_endpoint, "svc-tcp");
        assert_eq!(svc.stop.signal, StopSignal::Term);
        assert!(svc.prepare.exec.is_none());
        // Args are run[1..]; run[0] resolved to the closure executable.
        assert_eq!(svc.start.exec.executable, "/nix/store/c/bin/svc");
        assert_eq!(svc.start.exec.args, vec!["serve", "--port", "${port}"]);
        assert_eq!(svc.start.exec.tool_roots, vec!["/nix/store/c/bin"]);
        assert_eq!(svc.start.exec.stdin, StdinPolicy::Null);
        assert_eq!(em.environment.services, vec![ServiceId::new("svc")]);
        assert_eq!(em.slot_windows[&0].start, 23080);
        let task = em.tasks.get("t").expect("task lowered");
        assert_eq!(task.success_codes, vec![0]);
        assert_eq!(task.exec.args, vec!["--port", "${port}"]);
        assert_eq!(task.exec.tool_roots, vec!["/nix/store/ct/bin"]);
        assert_eq!(task.requires, vec![ServiceId::new("svc")]);
    }

    #[test]
    fn stdin_inherit_is_carried_into_resolved_invocation() {
        // A model that declares `stdin: inherit` must lower to an invocation that
        // records it, not silently collapse to null.
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["stdin"] = json!("inherit");
        let em = lower(&model_from(value)).expect("inherit stdin lowers");
        assert_eq!(
            em.services.get("svc").unwrap().start.exec.stdin,
            StdinPolicy::Inherit
        );
    }

    #[test]
    fn rejects_an_undeclared_tool_reference() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["tools"] = json!(["ghost"]);
        let error = lower(&model_from(value)).expect_err("undeclared tool must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn rejects_an_unresolvable_run_program() {
        // run[0] names a program no tool closure's executable provides.
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["ghost-program"]);
        let error = lower(&model_from(value)).expect_err("unresolvable run[0] must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost-program"));
    }

    #[test]
    fn rejects_a_carried_executable_that_disagrees_with_resolution() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["executable"] =
            json!("/nix/store/other/bin/svc");
        let error = lower(&model_from(value)).expect_err("executable mismatch must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("resolve"));
    }

    fn reject_reason(value: Value) -> Rejection {
        prove_references(&model_from(value)).expect_err("references must not resolve")
    }

    #[test]
    fn invocations_must_reference_declared_codebases() {
        let mut value = model_value();
        value["tasks"]["t"]["invocation"]["codebaseId"] = json!("ghost");
        assert_eq!(
            reject_reason(value),
            undeclared("invocation.codebaseId", "ghost")
        );
    }

    #[test]
    fn closure_target_must_match_closure_system() {
        let mut value = model_value();
        value["closures"]["c"]["targetSystem"] = json!("aarch64-darwin");
        assert!(matches!(
            reject_reason(value),
            Rejection::ClosureTargetMismatch { .. }
        ));
    }

    #[test]
    fn lifecycle_operation_ids_must_be_globally_unique() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["health"]["operationId"] = json!("svc.start");
        assert_eq!(
            reject_reason(value),
            Rejection::DuplicateOperationId {
                id: "svc.start".to_string()
            }
        );
    }

    #[test]
    fn closure_bindings_must_reference_declared_operations() {
        let mut value = model_value();
        value["closures"]["c"]["operationBindings"]
            .as_array_mut()
            .unwrap()
            .push(json!("ghost.op"));
        assert_eq!(
            reject_reason(value),
            Rejection::UnknownOperationBinding {
                binding: "ghost.op".to_string()
            }
        );
    }

    #[test]
    fn start_operation_must_be_bound_by_its_closure() {
        // The closure no longer binds svc.start, so the start invocation would
        // run a closure for an operation it never declared.
        let mut value = model_value();
        value["closures"]["c"]["operationBindings"] = json!(["task.t.run"]);
        assert_eq!(
            reject_reason(value),
            Rejection::OperationUnbound {
                operation_id: "svc.start".to_string(),
                closure_id: "c".to_string()
            }
        );
    }

    #[test]
    fn task_operation_must_be_bound_by_its_closure() {
        let mut value = model_value();
        value["closures"]["ct"]["operationBindings"] = json!([]);
        assert_eq!(
            reject_reason(value),
            Rejection::OperationUnbound {
                operation_id: "task.t.run".to_string(),
                closure_id: "ct".to_string()
            }
        );
    }

    #[test]
    fn env_task_service_deps_must_be_started_by_the_env() {
        // Task `t` requires `svc`, but the env no longer starts it.
        let mut value = model_value();
        value["environments"]["dev"]["services"] = json!([]);
        assert_eq!(
            reject_reason(value),
            undeclared("environment.task.requires", "svc")
        );
    }

    #[test]
    fn service_less_task_lowers() {
        // A task may require zero services (e.g. a lint/test task) as long as it
        // does not reference a service-derived placeholder.
        let mut value = model_value();
        value["tasks"]["t"]["requires"] = json!([]);
        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "--check"]);
        let em = lower(&model_from(value)).expect("a service-less task lowers");
        assert!(em.tasks["t"].requires.is_empty());
    }

    #[test]
    fn service_less_task_using_port_is_rejected() {
        // The fixture task's run args carry "${port}"; with no service to resolve
        // it, admission must reject rather than admit-then-fail.
        let mut value = model_value();
        value["tasks"]["t"]["requires"] = json!([]);
        let error =
            lower(&model_from(value)).expect_err("a service-less task using ${port} must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }

    #[test]
    fn service_less_task_using_port_in_env_is_rejected() {
        // Env values substitute like args; a bare ${port} with no service would
        // otherwise run with the literal placeholder in the environment.
        let mut value = model_value();
        value["tasks"]["t"]["requires"] = json!([]);
        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "--check"]);
        value["tasks"]["t"]["invocation"]["env"] = json!({ "PORT": "${port}" });
        let error = lower(&model_from(value))
            .expect_err("a service-less task using ${port} in env must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }

    /// Rewrite the fixture's ready probe as an invocation probe bound to
    /// closure `c`.
    fn with_exec_ready_probe(value: &mut Value) {
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "exec",
            "invocation": invocation_value("/nix/store/c/bin/svc", json!(["svc", "ping"])),
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        value["closures"]["c"]["operationBindings"]
            .as_array_mut()
            .unwrap()
            .push(json!("svc.ready"));
    }

    #[test]
    fn exec_probe_lowers_to_exec_variant() {
        let mut value = model_value();
        with_exec_ready_probe(&mut value);
        let em = lower(&model_from(value)).expect("exec probe lowers");
        let svc = em.services.get("svc").expect("service lowered");
        let Probe::Exec(probe) = &svc.ready.probe else {
            panic!("ready probe should lower to the exec variant");
        };
        // Probe args are run[1..]; the probe's per-attempt timeout overrides the
        // invocation's own timeout.
        assert_eq!(probe.exec.args, vec!["ping"]);
        assert_eq!(probe.exec.timeout, Duration::from_millis(500));
        assert_eq!(probe.max_attempts, 5);
        assert!(matches!(svc.health.probe, Probe::Tcp(_)));
    }

    #[test]
    fn tcp_probe_with_invocation_is_rejected() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "tcp",
            "invocation": invocation_value("/nix/store/c/bin/svc", json!(["svc", "ping"])),
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        let error = lower(&model_from(value)).expect_err("tcp probe with invocation must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("tcp but carries an invocation"));
    }

    #[test]
    fn exec_probe_without_invocation_is_rejected() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "exec",
            "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        let error =
            lower(&model_from(value)).expect_err("exec probe without invocation must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("declares no invocation"));
    }

    #[test]
    fn exec_probe_op_must_be_bound_by_its_closure() {
        let mut value = model_value();
        with_exec_ready_probe(&mut value);
        // Remove the binding again: the probe's closure no longer authorizes
        // the ready operation.
        value["closures"]["c"]["operationBindings"] = json!(["svc.start", "task.t.run"]);
        let error = lower(&model_from(value)).expect_err("unbound probe op must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("not bound"));
    }

    #[test]
    fn exec_probe_named_ref_outside_connects_to_is_rejected() {
        let mut value = model_value();
        with_exec_ready_probe(&mut value);
        value["services"]["svc"]["lifecycle"]["ready"]["probe"]["invocation"]["run"] =
            json!(["svc", "--db", "${port:ghost}"]);
        let error = lower(&model_from(value)).expect_err("out-of-scope named ref must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn composite_task_lowers_with_resolved_steps() {
        let mut value = model_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": {
                "first": { "task": "t" },
                "second": { "task": "t", "dependsOn": ["first"] }
            }
        });
        let em = lower(&model_from(value)).expect("composite lowers");
        let composite = em.composites.get("pipeline").expect("composite lowered");
        assert_eq!(composite.steps.len(), 2);
        assert_eq!(composite.steps[0].name, "first");
        assert_eq!(composite.steps[1].depends_on, vec!["first"]);
        assert!(!em.tasks.contains_key("pipeline"));
    }

    #[test]
    fn composite_step_must_reference_a_declared_task() {
        let mut value = model_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": { "only": { "task": "ghost" } }
        });
        let error = lower(&model_from(value)).expect_err("dangling step task must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn composite_step_depends_on_must_name_a_sibling() {
        let mut value = model_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": { "only": { "task": "t", "dependsOn": ["ghost"] } }
        });
        let error = lower(&model_from(value)).expect_err("dangling dependsOn must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn composite_may_be_an_environment_task() {
        let mut value = model_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "steps": { "only": { "task": "t" } }
        });
        value["environments"]["dev"]["tasks"] = json!(["pipeline"]);
        let em = lower(&model_from(value)).expect("composite env selection lowers");
        assert_eq!(em.environment.tasks, vec![TaskId::new("pipeline")]);
    }

    #[test]
    fn leaf_without_invocation_is_rejected() {
        let mut value = model_value();
        value["tasks"]["t"]
            .as_object_mut()
            .unwrap()
            .remove("invocation");
        let error = lower(&model_from(value)).expect_err("invocation-less leaf must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("kind-incoherent"));
    }

    #[test]
    fn declared_path_env_is_rejected() {
        // PATH is runtime-owned (assembled from tool roots); a declared PATH
        // would be silently overwritten, so it is rejected fail-closed.
        let mut value = model_value();
        value["tasks"]["t"]["invocation"]["env"] = json!({ "PATH": "/usr/bin" });
        let error = lower(&model_from(value)).expect_err("declared PATH must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("PATH"));
    }

    #[test]
    fn named_endpoint_refs_are_extracted() {
        assert_eq!(
            named_endpoint_refs("--db ${host:postgres}:${port:postgres} --cache ${port:redis}"),
            vec!["postgres", "redis", "postgres"]
        );
        assert!(named_endpoint_refs("--port ${port}").is_empty());
    }

    #[test]
    fn task_named_ref_outside_requires_is_rejected() {
        let mut value = model_value();
        value["tasks"]["t"]["invocation"]["run"] = json!(["task", "--db", "${port:ghost}"]);
        let error = lower(&model_from(value)).expect_err("out-of-scope named ref must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn service_named_ref_outside_connects_to_is_rejected() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "--db", "${port:ghost}"]);
        let error = lower(&model_from(value)).expect_err("out-of-scope named ref must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn environment_must_start_connects_to_dependencies() {
        let mut value = model_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }
        value["closures"]["c"]["operationBindings"]
            .as_array_mut()
            .unwrap()
            .push(json!("dep.start"));
        value["services"]["dep"] = dep;
        value["services"]["svc"]["connectsTo"] = json!(["dep"]);
        // dev environment starts only svc: the wiring target is missing.
        let error = lower(&model_from(value)).expect_err("unstarted connectsTo must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("dep"));
    }

    #[test]
    fn named_ref_inside_connects_to_lowers_with_env() {
        let mut value = model_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }
        value["closures"]["c"]["operationBindings"]
            .as_array_mut()
            .unwrap()
            .push(json!("dep.start"));
        value["services"]["dep"] = dep;
        value["services"]["svc"]["connectsTo"] = json!(["dep"]);
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "serve", "--db", "${host:dep}:${port:dep}"]);
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"] =
            json!({ "DB_URL": "tcp://${host:dep}:${port:dep}" });
        value["environments"]["dev"]["services"] = json!(["svc", "dep"]);
        let em = lower(&model_from(value)).expect("declared named refs lower");
        let svc = em.services.get("svc").expect("service lowered");
        assert_eq!(svc.connects_to, vec![ServiceId::new("dep")]);
    }
}
