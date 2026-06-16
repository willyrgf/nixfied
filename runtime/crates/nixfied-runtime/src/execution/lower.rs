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
    ClosureSpec, InvocationSpec, Lifecycle, Model, OperationId, ProbeKind, ProbeSpec, ServiceId,
    ServiceSpec, StatePolicy, StepSpec, StopSpec, Target, TaskId, TaskKind, TaskSpec,
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
        secrets: _,
        environments: _,
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
                lower_service(
                    name,
                    service,
                    services,
                    tasks,
                    closures,
                    state,
                    &model.target,
                )?,
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
        slot_windows,
    })
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
    all_services: &BTreeMap<String, ServiceSpec>,
    tasks: &BTreeMap<String, TaskSpec>,
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
    let endpoint_less = endpoints.is_empty();
    // prepare is a task reference with full task semantics. It must name a
    // declared task, and its derived service union must not (transitively)
    // include the owning service — a service cannot wait on itself to prepare.
    let prepare = match prepare {
        Some(spec) => {
            if !tasks.contains_key(spec.task.as_str()) {
                return Err(undeclared("service.prepare.task", spec.task.as_str()).into());
            }
            Some(spec.task.clone())
        }
        None => None,
    };
    // Effects coherence, both directions: a listening service's start closure
    // must attest `network-listener`; an endpoint-less service's must not — it
    // would announce a listener the planner cannot reserve.
    if let Some((closure_id, closure)) = executable_closure(&start.invocation, closures) {
        let listens = closure
            .effects
            .iter()
            .any(|effect| matches!(effect, nixfied_model::ClosureEffect::NetworkListener));
        if !endpoint_less && !listens {
            return Err(Rejection::EffectsIncoherent {
                service: name.to_string(),
                closure_id: closure_id.clone(),
                expected: "declared endpoints require `network-listener` on the start closure",
            }
            .into());
        }
        if endpoint_less && listens {
            return Err(Rejection::EffectsIncoherent {
                service: name.to_string(),
                closure_id: closure_id.clone(),
                expected: "an endpoint-less service's start closure must not declare `network-listener` (an unreservable listener)",
            }
            .into());
        }
    }
    let start = StartOp {
        meta: op_meta(&start.operation_id, &start.terminal),
        exec: resolve_invocation(&owner, &start.invocation, closures)?,
    };
    // An endpoint-less service has no tcp probe target: readiness means "the
    // probe answers", so its probes must be invocations (rejected at eval and
    // re-proven here).
    if endpoint_less {
        for (class, probe) in [("ready", &ready.probe), ("health", &health.probe)] {
            if probe.kind == ProbeKind::Tcp {
                return Err(Rejection::TcpProbeWithoutEndpoint {
                    service: name.to_string(),
                    class,
                }
                .into());
            }
        }
    }
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
    // An endpoint-less connectsTo target keeps its ordering/derivation meaning
    // but is NOT addressable: it leaves the named-placeholder scope entirely.
    let allowed: BTreeSet<&str> = endpoints
        .keys()
        .map(String::as_str)
        .chain(connects_to.iter().filter_map(|id| {
            let target = all_services.get(id.as_str())?;
            (!target.endpoints.is_empty()).then_some(id.as_str())
        }))
        .collect();
    for exec in std::iter::once(&start.exec)
        .chain(probe_exec(&ready.probe))
        .chain(probe_exec(&health.probe))
    {
        require_named_refs_in_scope(
            &owner,
            "own endpoints or addressable connectsTo",
            exec,
            &allowed,
        )?;
        // The bare-placeholder rule tasks already have, applied symmetrically:
        // an endpoint-less service has no primary endpoint for `${port}` /
        // `${host}` to resolve to.
        if endpoint_less {
            for placeholder in ["${port}", "${host}"] {
                if exec
                    .args
                    .iter()
                    .chain(exec.env.values())
                    .any(|value| value.contains(placeholder))
                {
                    return Err(Rejection::ServicePlaceholderWithoutEndpoint {
                        service: name.to_string(),
                        placeholder,
                    }
                    .into());
                }
            }
        }
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
        service_lifetime: _,
        operation_id: _,
        invocation,
        requires,
        // Re-derived and compared against the carried value at admission
        // (DERIVE-1); the executor reads the derived union via the planner.
        services_required: _,
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
    // `${port}`/`${host}` resolve from the task's primary (first) requirement.
    // A task that requires no services — or whose primary requirement is
    // endpoint-less — has no endpoint, so referencing them is unrunnable:
    // reject it here rather than fail at execution.
    let primary_has_endpoint = requires
        .first()
        .and_then(|id| services.get(id))
        .map(|service| !service.endpoints.is_empty())
        .unwrap_or(false);
    if !primary_has_endpoint {
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
    // requirements (any of them, not just the primary) that actually declare
    // endpoints — an endpoint-less requirement is not addressable.
    let allowed: BTreeSet<&str> = requires
        .iter()
        .filter(|id| {
            services
                .get(id.as_str())
                .map(|service| !service.endpoints.is_empty())
                .unwrap_or(false)
        })
        .map(|id| id.as_str())
        .collect();
    require_named_refs_in_scope(&owner, "addressable requires", &exec, &allowed)?;
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
    TcpProbeWithoutEndpoint {
        service: String,
        class: &'static str,
    },
    ServicePlaceholderWithoutEndpoint {
        service: String,
        placeholder: &'static str,
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
    DerivedFactMismatch {
        owner: String,
        fact: &'static str,
        carried: String,
        derived: String,
    },
}

impl Rejection {
    fn message(&self) -> String {
        match self {
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
            Rejection::TcpProbeWithoutEndpoint { service, class } => {
                format!(
                    "service {service} is endpoint-less but its {class} probe is tcp (no target to connect)"
                )
            }
            Rejection::ServicePlaceholderWithoutEndpoint {
                service,
                placeholder,
            } => format!(
                "service {service} references {placeholder} but declares no endpoint to resolve it"
            ),
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
            Rejection::DerivedFactMismatch {
                owner,
                fact,
                carried,
                derived,
            } => {
                format!(
                    "{owner} carries {fact} [{carried}] but the graph derives [{derived}] (DERIVE-1)"
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

    // Per-position tool/resolution coherence first, so an undeclared tool or
    // unresolvable run[0] surfaces as itself rather than as a downstream
    // derived-fact mismatch.
    for (operation_id, invocation) in invocation_positions(model) {
        for tool in invocation.tools.iter() {
            if !model.closures.contains_key(tool.as_str()) {
                return Err(undeclared("invocation.tools", tool.as_str()));
            }
        }
        if executable_closure(invocation, &model.closures).is_none() {
            return Err(Rejection::RunUnresolvable {
                owner: format!("operation {operation_id}"),
                program: invocation.run.first().cloned().unwrap_or_default(),
            });
        }
    }

    // The combined connectsTo + prepare-requires graph must be acyclic before
    // any union derivation walks it.
    prove_service_graph_acyclic(model)?;

    // DERIVE-1: derived facts are re-derived here and compared with the
    // carried values, fail closed, naming both sides.
    prove_derived_operation_bindings(model)?;
    prove_derived_services_required(model)?;

    Ok(())
}

/// Re-derive each closure's operation bindings (docs/DERIVATION_SPEC.md §4):
/// the byte-sorted operation ids of every invocation position whose run[0]
/// resolves to the closure. The carried `operationBindings` must equal the
/// derivation exactly — the eval-side narrowing gate has already been applied
/// there, and the emitted value is the derived set.
fn prove_derived_operation_bindings(model: &Model) -> Result<(), Rejection> {
    let positions = invocation_positions(model);
    for (closure_id, closure) in &model.closures {
        let mut derived: Vec<&str> = positions
            .iter()
            .filter(|(_, invocation)| {
                executable_closure(invocation, &model.closures)
                    .is_some_and(|(id, _)| id == closure_id)
            })
            .map(|(operation_id, _)| operation_id.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        derived.sort_unstable();
        let carried: Vec<&str> = closure
            .operation_bindings
            .iter()
            .map(|binding| binding.as_str())
            .collect();
        if derived != carried {
            return Err(Rejection::DerivedFactMismatch {
                owner: format!("closure {closure_id}"),
                fact: "operationBindings",
                carried: carried.join(", "),
                derived: derived.join(", "),
            });
        }
    }
    Ok(())
}

/// Re-derive each task's `servicesRequired` (docs/DERIVATION_SPEC.md §3): the
/// union of transitive leaf `requires`, closed over `connectsTo`, byte-sorted.
fn prove_derived_services_required(model: &Model) -> Result<(), Rejection> {
    for (task_id, task) in &model.tasks {
        let derived = derive_services_required(model, task_id);
        let carried: Vec<&str> = task
            .services_required
            .iter()
            .map(|service| service.as_str())
            .collect();
        if derived != carried {
            return Err(Rejection::DerivedFactMismatch {
                owner: format!("task {task_id}"),
                fact: "servicesRequired",
                carried: carried.join(", "),
                derived: derived.join(", "),
            });
        }
    }
    Ok(())
}

/// The derived service union of one task. Set semantics throughout; the
/// authored task-reference graph is acyclic by the time admission compares
/// (the planner proves it), but the walk guards with a seen set so this
/// function is total on any deserialized model.
pub(crate) fn derive_services_required<'a>(model: &'a Model, task_id: &str) -> Vec<&'a str> {
    let mut base = BTreeSet::new();
    leaf_requires(model, task_id, &mut BTreeSet::new(), &mut base);
    // Close over connectsTo AND prepare requirements to a fixpoint: starting a
    // service runs its prepare task first, whose leaves may require other
    // services (docs/DERIVATION_SPEC.md §3).
    loop {
        let mut additions: Vec<&str> = Vec::new();
        for service_id in base.iter() {
            let Some(service) = model.services.get(*service_id) else {
                continue;
            };
            additions.extend(
                service
                    .connects_to
                    .iter()
                    .map(|target| target.as_str())
                    .filter(|target| !base.contains(target)),
            );
            if let Some(prepare) = &service.lifecycle.prepare {
                let mut prepare_base = BTreeSet::new();
                leaf_requires(
                    model,
                    prepare.task.as_str(),
                    &mut BTreeSet::new(),
                    &mut prepare_base,
                );
                additions.extend(prepare_base.into_iter().filter(|t| !base.contains(t)));
            }
        }
        if additions.is_empty() {
            break;
        }
        base.extend(additions);
    }
    base.into_iter().collect()
}

/// The union of transitive leaf `requires` reachable from one task.
fn leaf_requires<'a>(
    model: &'a Model,
    task_id: &str,
    seen: &mut BTreeSet<String>,
    out: &mut BTreeSet<&'a str>,
) {
    if !seen.insert(task_id.to_string()) {
        return;
    }
    let Some(task) = model.tasks.get(task_id) else {
        return;
    };
    match task.kind {
        TaskKind::Leaf => {
            out.extend(task.requires.iter().map(|service| service.as_str()));
        }
        TaskKind::Composite => {
            for step in task.steps.values() {
                leaf_requires(model, step.task.as_str(), seen, out);
            }
        }
    }
}

/// The kind-tagged service dependency edges: `connectsTo` wiring plus prepare
/// requirements (the prepare task's transitive leaf `requires`).
fn service_edges<'a>(model: &'a Model, service_id: &str) -> Vec<(&'a str, &'static str)> {
    let Some(service) = model.services.get(service_id) else {
        return Vec::new();
    };
    let mut edges: Vec<(&str, &'static str)> = service
        .connects_to
        .iter()
        .map(|target| (target.as_str(), "connectsTo"))
        .collect();
    if let Some(prepare) = &service.lifecycle.prepare {
        let mut prepare_base = BTreeSet::new();
        leaf_requires(
            model,
            prepare.task.as_str(),
            &mut BTreeSet::new(),
            &mut prepare_base,
        );
        edges.extend(
            prepare_base
                .into_iter()
                .map(|target| (target, "prepare requires")),
        );
    }
    edges
}

/// The combined `connectsTo` + prepare-requires graph must be acyclic. The
/// rejection renders the cycle with each edge's kind, so the operator can see
/// which hops are wiring and which are prepare requirements.
fn prove_service_graph_acyclic(model: &Model) -> Result<(), Rejection> {
    fn visit<'a>(
        model: &'a Model,
        start: &str,
        current: &'a str,
        trail: &mut Vec<(&'a str, &'static str)>,
        seen: &mut BTreeSet<&'a str>,
    ) -> Option<Vec<(&'a str, &'static str)>> {
        for (target, kind) in service_edges(model, current) {
            if target == start {
                let mut cycle = trail.clone();
                cycle.push((target, kind));
                return Some(cycle);
            }
            if seen.insert(target) {
                trail.push((target, kind));
                if let Some(cycle) = visit(model, start, target, trail, seen) {
                    return Some(cycle);
                }
                trail.pop();
            }
        }
        None
    }
    for start in model.services.keys() {
        if let Some(cycle) = visit(model, start, start, &mut Vec::new(), &mut BTreeSet::new()) {
            let mut rendered = start.to_string();
            for (target, kind) in cycle {
                rendered.push_str(&format!(" -[{kind}]-> {target}"));
            }
            return Err(Rejection::ServiceGraphCycle { cycle: rendered });
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
                "markerIdentity": "nixfied-state", "stateEpoch": "1",
                "cleanupPolicy": "delete-on-clean", "persistence": "run-scoped"
            },
            "closures": {
                "c": {
                    "kind": "executable", "storePath": "/nix/store/c", "executable": "/nix/store/c/bin/svc",
                    "targetSystem": "x86_64-linux",
                    "operationBindings": ["svc.start"],
                    "requiresExecutable": true, "effects": ["process", "network-listener"]
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
                    "serviceLifetime": "run-scoped",
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
                    "servicesRequired": ["svc"],
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

    fn model_from(value: Value) -> Model {
        serde_json::from_value(value).expect("fixture should deserialize")
    }

    #[test]
    fn lowers_a_valid_model() {
        let em = lower(&model_from(model_value())).expect("valid model lowers");
        let svc = em.services.get("svc").expect("service lowered");
        assert_eq!(svc.endpoints["svc-tcp"].host.to_string(), "127.0.0.1");
        assert_eq!(svc.primary_endpoint.as_deref(), Some("svc-tcp"));
        assert_eq!(svc.stop.signal, StopSignal::Term);
        assert!(svc.prepare.is_none());
        // Args are run[1..]; run[0] resolved to the closure executable.
        assert_eq!(svc.start.exec.executable, "/nix/store/c/bin/svc");
        assert_eq!(svc.start.exec.args, vec!["serve", "--port", "${port}"]);
        assert_eq!(svc.start.exec.tool_roots, vec!["/nix/store/c/bin"]);
        assert_eq!(svc.start.exec.stdin, StdinPolicy::Null);
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
    fn carried_bindings_must_equal_the_derivation() {
        // The carried bindings list an operation the graph never dispatches
        // against this closure — a derived-fact mismatch (DERIVE-1).
        let mut value = model_value();
        value["closures"]["c"]["operationBindings"] = json!(["svc.start", "task.t.run"]);
        assert!(matches!(
            reject_reason(value),
            Rejection::DerivedFactMismatch {
                fact: "operationBindings",
                ..
            }
        ));
    }

    #[test]
    fn missing_carried_bindings_are_a_derived_fact_mismatch() {
        // The closure is dispatched against svc.start but carries no binding
        // for it.
        let mut value = model_value();
        value["closures"]["c"]["operationBindings"] = json!([]);
        assert!(matches!(
            reject_reason(value),
            Rejection::DerivedFactMismatch {
                fact: "operationBindings",
                ..
            }
        ));
    }

    /// Golden vector V4 (docs/DERIVATION_SPEC.md §6) on the runtime side:
    /// union of transitive leaf requires, closed over connectsTo, byte-sorted.
    #[test]
    fn services_required_derivation_matches_vector_v4() {
        let mut value = model_value();
        // svc gains a connectsTo dependency `dep`, so the task's derived union
        // closes over it.
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }
        value["closures"]["c"]["operationBindings"] = json!(["dep.start", "svc.start"]);
        value["services"]["dep"] = dep;
        value["services"]["svc"]["connectsTo"] = json!(["dep"]);
        let model = model_from(value);
        assert_eq!(derive_services_required(&model, "t"), vec!["dep", "svc"]);
    }

    #[test]
    fn operation_bindings_vector_v5_run0_closure_binds_tools_do_not() {
        let mut value = model_value();
        value["closures"]["gitC"] = json!({
            "kind": "executable", "storePath": "/nix/store/git", "executable": "/nix/store/git/bin/git",
            "targetSystem": "x86_64-linux", "operationBindings": [],
            "requiresExecutable": true, "effects": ["process"]
        });
        value["closures"]["probeC"] = json!({
            "kind": "executable", "storePath": "/nix/store/probe", "executable": "/nix/store/probe/bin/probe",
            "targetSystem": "x86_64-linux", "operationBindings": ["svc.ready"],
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
        lower(&model_from(value)).expect("tool-only closure and probe binding vector should lower");
    }

    #[test]
    fn operation_bindings_vector_v6_override_and_v7_multiple_leaves() {
        let mut value = model_value();
        value["closures"]["ct"]["operationBindings"] = json!(["task.custom.odd", "task.t.run"]);
        value["tasks"]["odd"] = json!({
            "kind": "leaf",
            "serviceLifetime": "run-scoped",
            "operationId": "task.custom.odd",
            "invocation": {
                "tools": ["ct"],
                "run": ["task", "odd"],
                "executable": "/nix/store/ct/bin/task",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": [],
            "servicesRequired": [],
            "exitPolicy": { "successCodes": [0] }
        });
        lower(&model_from(value)).expect("override operation id should participate in bindings");
    }

    #[test]
    fn services_required_vector_v8_diamond_dedups() {
        let mut value = model_value();
        add_named_service(&mut value, "db", &[]);
        add_named_service(&mut value, "a", &["db"]);
        add_named_service(&mut value, "b", &["db"]);
        value["closures"]["c"]["operationBindings"] =
            json!(["a.start", "b.start", "db.start", "svc.start"]);
        value["tasks"]["t"]["requires"] = json!(["a", "b"]);
        value["tasks"]["t"]["servicesRequired"] = json!(["a", "b", "db"]);
        let model = model_from(value);
        assert_eq!(derive_services_required(&model, "t"), vec!["a", "b", "db"]);
        lower(&model).expect("diamond service graph should lower");
    }

    #[test]
    fn services_required_vector_v9_prepare_task_may_be_composite() {
        let mut value = model_value();
        add_named_service(&mut value, "dep", &[]);
        value["closures"]["c"]["operationBindings"] = json!(["dep.start", "svc.start"]);
        value["closures"]["ct"]["operationBindings"] =
            json!(["task.migrate.run", "task.seed.run", "task.t.run"]);
        value["tasks"]["migrate"] = json!({
            "kind": "leaf",
            "serviceLifetime": "run-scoped",
            "operationId": "task.migrate.run",
            "invocation": {
                "tools": ["ct"],
                "run": ["task", "migrate", "--db", "${port:dep}"],
                "executable": "/nix/store/ct/bin/task",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["dep"],
            "servicesRequired": ["dep"],
            "exitPolicy": { "successCodes": [0] }
        });
        value["tasks"]["seed"] = json!({
            "kind": "leaf",
            "serviceLifetime": "run-scoped",
            "operationId": "task.seed.run",
            "invocation": {
                "tools": ["ct"],
                "run": ["task", "seed"],
                "executable": "/nix/store/ct/bin/task",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": [],
            "servicesRequired": [],
            "exitPolicy": { "successCodes": [0] }
        });
        value["tasks"]["prep"] = json!({
            "kind": "composite",
            "serviceLifetime": "run-scoped",
            "steps": {
                "migrate": { "task": "migrate" },
                "seed": { "task": "seed", "dependsOn": ["migrate"] }
            },
            "servicesRequired": ["dep"]
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "prep" });
        value["tasks"]["t"]["servicesRequired"] = json!(["dep", "svc"]);
        let model = model_from(value);
        assert_eq!(derive_services_required(&model, "t"), vec!["dep", "svc"]);
        lower(&model).expect("composite prepare task should lower");
    }

    #[test]
    fn services_required_vector_v10_connects_to_fixpoint() {
        let mut value = model_value();
        add_named_service(&mut value, "api", &["worker"]);
        add_named_service(&mut value, "worker", &["db"]);
        add_named_service(&mut value, "db", &["cache"]);
        add_named_service(&mut value, "cache", &[]);
        value["closures"]["c"]["operationBindings"] = json!([
            "api.start",
            "cache.start",
            "db.start",
            "svc.start",
            "worker.start"
        ]);
        value["tasks"]["t"]["requires"] = json!(["api"]);
        value["tasks"]["t"]["servicesRequired"] = json!(["api", "cache", "db", "worker"]);
        let model = model_from(value);
        assert_eq!(
            derive_services_required(&model, "t"),
            vec!["api", "cache", "db", "worker"]
        );
        lower(&model).expect("long connectsTo closure should lower");
    }

    #[test]
    fn services_required_mismatch_is_rejected_naming_both_values() {
        let mut value = model_value();
        value["tasks"]["t"]["servicesRequired"] = json!([]);
        let error = lower(&model_from(value)).expect_err("derived-fact mismatch must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(
            error.message.contains("servicesRequired"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("[svc]") || error.message.contains("svc"),
            "{}",
            error.message
        );
        assert!(error.message.contains("DERIVE-1"), "{}", error.message);
    }

    #[test]
    fn service_less_task_lowers() {
        // A task may require zero services (e.g. a lint/test task) as long as it
        // does not reference a service-derived placeholder.
        let mut value = model_value();
        value["tasks"]["t"]["requires"] = json!([]);
        value["tasks"]["t"]["servicesRequired"] = json!([]);
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
        value["tasks"]["t"]["servicesRequired"] = json!([]);
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
        value["tasks"]["t"]["servicesRequired"] = json!([]);
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
        // Carried bindings are the byte-sorted derived set.
        value["closures"]["c"]["operationBindings"] = json!(["svc.ready", "svc.start"]);
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
        // Remove the binding again: the carried bindings no longer cover the
        // ready operation the graph dispatches against the closure.
        value["closures"]["c"]["operationBindings"] = json!(["svc.start"]);
        let error = lower(&model_from(value)).expect_err("unbound probe op must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("DERIVE-1"), "{}", error.message);
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
            "serviceLifetime": "run-scoped",
            "servicesRequired": ["svc"],
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
            "serviceLifetime": "run-scoped",
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
            "serviceLifetime": "run-scoped",
            "servicesRequired": ["svc"],
            "steps": { "only": { "task": "t", "dependsOn": ["ghost"] } }
        });
        let error = lower(&model_from(value)).expect_err("dangling dependsOn must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn composite_is_a_selectable_root() {
        let mut value = model_value();
        value["tasks"]["pipeline"] = json!({
            "kind": "composite",
            "serviceLifetime": "run-scoped",
            "servicesRequired": ["svc"],
            "steps": { "only": { "task": "t" } }
        });
        let em = lower(&model_from(value)).expect("composite lowers as a selectable root");
        assert!(em.composites.contains_key("pipeline"));
    }

    #[test]
    fn leaf_without_invocation_is_rejected() {
        let mut value = model_value();
        value["tasks"]["t"]
            .as_object_mut()
            .unwrap()
            .remove("invocation");
        // The invocation-less leaf no longer dispatches against its closure, so
        // the carried bindings already mismatch the derivation before the
        // kind-coherence backstop fires; either way the model is refused.
        let error = lower(&model_from(value)).expect_err("invocation-less leaf must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }

    /// An endpoint-less sibling service `worker` with invocation probes; the
    /// closure binds its ops.
    fn with_endpoint_less_worker(value: &mut Value) {
        value["closures"]["cw"] = json!({
            "kind": "executable", "storePath": "/nix/store/cw", "executable": "/nix/store/cw/bin/worker",
            "targetSystem": "x86_64-linux",
            "operationBindings": ["worker.health", "worker.ready", "worker.start"],
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
        value["closures"]["c"]["operationBindings"] = json!(["svc.start"]);
    }

    #[test]
    fn endpoint_less_service_lowers_without_a_primary() {
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        let em = lower(&model_from(value)).expect("endpoint-less service lowers");
        let worker = em.services.get("worker").expect("worker lowered");
        assert!(worker.endpoints.is_empty());
        assert!(worker.primary_endpoint.is_none());
    }

    #[test]
    fn endpoint_less_tcp_probe_is_rejected() {
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        value["services"]["worker"]["lifecycle"]["ready"]["probe"] = json!({
            "kind": "tcp", "timeoutMs": 500, "retryIntervalMs": 100, "maxAttempts": 5
        });
        // The tcp probe carries no invocation, so the closure's derived
        // bindings shrink with it.
        value["closures"]["cw"]["operationBindings"] = json!(["worker.health", "worker.start"]);
        let error = lower(&model_from(value)).expect_err("tcp probe without endpoint must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("endpoint-less"), "{}", error.message);
    }

    #[test]
    fn endpoint_less_bare_placeholder_is_rejected() {
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        value["services"]["worker"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["worker", "consume", "--listen", "${port}"]);
        let error = lower(&model_from(value)).expect_err("bare placeholder must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
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
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        value["services"]["svc"]["connectsTo"] = json!(["worker"]);
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "serve", "--peer", "${port:worker}"]);
        let error =
            lower(&model_from(value)).expect_err("named ref toward endpoint-less must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("worker"), "{}", error.message);
    }

    #[test]
    fn task_named_ref_toward_endpoint_less_requirement_is_rejected() {
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        value["tasks"]["t"]["requires"] = json!(["svc", "worker"]);
        value["tasks"]["t"]["servicesRequired"] = json!(["svc", "worker"]);
        value["tasks"]["t"]["invocation"]["run"] =
            json!(["task", "--port", "${port}", "--peer", "${port:worker}"]);
        let error =
            lower(&model_from(value)).expect_err("named ref toward endpoint-less must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("worker"), "{}", error.message);
    }

    #[test]
    fn endpoint_less_requirement_stays_legal_without_placeholders() {
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        value["tasks"]["t"]["requires"] = json!(["svc", "worker"]);
        value["tasks"]["t"]["servicesRequired"] = json!(["svc", "worker"]);
        let em = lower(&model_from(value)).expect("requiring an endpoint-less service is legal");
        assert_eq!(em.tasks["t"].requires.len(), 2);
    }

    #[test]
    fn listening_service_start_closure_must_declare_network_listener() {
        let mut value = model_value();
        value["closures"]["c"]["effects"] = json!(["process"]);
        let error = lower(&model_from(value)).expect_err("missing network-listener must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(
            error.message.contains("network-listener"),
            "{}",
            error.message
        );
    }

    #[test]
    fn endpoint_less_start_closure_must_not_declare_network_listener() {
        let mut value = model_value();
        with_endpoint_less_worker(&mut value);
        value["closures"]["cw"]["effects"] = json!(["process", "network-listener"]);
        let error = lower(&model_from(value)).expect_err("unreservable listener must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(
            error.message.contains("network-listener"),
            "{}",
            error.message
        );
    }

    #[test]
    fn prepare_task_must_be_declared() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "ghost" });
        let error = lower(&model_from(value)).expect_err("dangling prepare task must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"), "{}", error.message);
    }

    #[test]
    fn prepare_task_reference_lowers_and_widens_the_union() {
        // Cross-service prepare: svc's prepare task requires dep, so starting
        // svc pulls dep into every union that contains svc.
        let mut value = model_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }
        value["closures"]["c"]["operationBindings"] = json!(["dep.start", "svc.start"]);
        value["services"]["dep"] = dep;
        value["closures"]["cm"] = json!({
            "kind": "executable", "storePath": "/nix/store/cm", "executable": "/nix/store/cm/bin/migrate",
            "targetSystem": "x86_64-linux",
            "operationBindings": ["task.migrate.run"],
            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["migrate"] = json!({
            "kind": "leaf",
            "serviceLifetime": "run-scoped",
            "operationId": "task.migrate.run",
            "invocation": {
                "tools": ["cm"],
                "run": ["migrate", "--db", "${port:dep}"],
                "executable": "/nix/store/cm/bin/migrate",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["dep"],
            "servicesRequired": ["dep"],
            "exitPolicy": { "successCodes": [0] }
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "migrate" });
        // Task t requires svc; svc's prepare requires dep -> union closes over it.
        value["tasks"]["t"]["servicesRequired"] = json!(["dep", "svc"]);
        let em = lower(&model_from(value)).expect("cross-service prepare lowers");
        assert_eq!(
            em.services
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
        let mut value = model_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }
        value["closures"]["c"]["operationBindings"] = json!(["dep.start", "svc.start"]);
        dep["connectsTo"] = json!(["svc"]);
        value["services"]["dep"] = dep;
        value["closures"]["cm"] = json!({
            "kind": "executable", "storePath": "/nix/store/cm", "executable": "/nix/store/cm/bin/migrate",
            "targetSystem": "x86_64-linux",
            "operationBindings": ["task.migrate.run"],
            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["migrate"] = json!({
            "kind": "leaf",
            "serviceLifetime": "run-scoped",
            "operationId": "task.migrate.run",
            "invocation": {
                "tools": ["cm"],
                "run": ["migrate"],
                "executable": "/nix/store/cm/bin/migrate",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["dep"],
            "servicesRequired": ["dep"],
            "exitPolicy": { "successCodes": [0] }
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "migrate" });
        let error = lower(&model_from(value)).expect_err("heterogeneous cycle must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
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
        let mut value = model_value();
        value["closures"]["cm"] = json!({
            "kind": "executable", "storePath": "/nix/store/cm", "executable": "/nix/store/cm/bin/selfinit",
            "targetSystem": "x86_64-linux",
            "operationBindings": ["task.selfinit.run"],
            "requiresExecutable": true, "effects": ["process"]
        });
        value["tasks"]["selfinit"] = json!({
            "kind": "leaf",
            "serviceLifetime": "run-scoped",
            "operationId": "task.selfinit.run",
            "invocation": {
                "tools": ["cm"],
                "run": ["selfinit"],
                "executable": "/nix/store/cm/bin/selfinit",
                "env": {}, "codebaseId": "main", "cwd": ".", "stdin": "null", "timeoutMs": 1000
            },
            "requires": ["svc"],
            "servicesRequired": ["svc"],
            "exitPolicy": { "successCodes": [0] }
        });
        value["services"]["svc"]["lifecycle"]["prepare"] = json!({ "task": "selfinit" });
        let error = lower(&model_from(value)).expect_err("self-preparing service must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
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
    fn named_ref_inside_connects_to_lowers_with_env() {
        let mut value = model_value();
        let mut dep = service_value();
        dep["endpoints"] = json!({ "dep-tcp": { "endpointId": "dep-tcp", "host": "127.0.0.1" } });
        dep["primaryEndpoint"] = json!("dep-tcp");
        for (class, op) in dep["lifecycle"].as_object_mut().unwrap() {
            op["operationId"] = json!(format!("dep.{class}"));
        }
        value["closures"]["c"]["operationBindings"] = json!(["dep.start", "svc.start"]);
        value["services"]["dep"] = dep;
        value["services"]["svc"]["connectsTo"] = json!(["dep"]);
        // Task t requires svc; svc now connectsTo dep, so the derived union
        // closes over it.
        value["tasks"]["t"]["servicesRequired"] = json!(["dep", "svc"]);
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["run"] =
            json!(["svc", "serve", "--db", "${host:dep}:${port:dep}"]);
        value["services"]["svc"]["lifecycle"]["start"]["invocation"]["env"] =
            json!({ "DB_URL": "tcp://${host:dep}:${port:dep}" });
        let em = lower(&model_from(value)).expect("declared named refs lower");
        let svc = em.services.get("svc").expect("service lowered");
        assert_eq!(svc.connects_to, vec![ServiceId::new("dep")]);
    }
}
