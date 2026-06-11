//! The total lowering `Model -> ExecutionModel`.
//!
//! Every source struct is destructured with **no `..`**, so adding a field to the
//! schema is a compile error here until the lowering consciously maps or refuses
//! it. Rejections are `ModelAdmission` errors — the model was inexpressible to the
//! runtime — never execution failures.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use nixfied_model::{
    Environment, ExecSpec, Lifecycle, Model, OperationId, ProbeTiming, ServiceId, ServiceSpec,
    StopSpec, TaskId, TaskSpec, TerminalSemantics, WorkflowSpec,
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
        state: _,
        closures: _,
        execs,
        services,
        tasks,
        workflows,
        docs: _,
    } = model;

    let lowered_services = services
        .iter()
        .map(|(name, service)| Ok((ServiceId::new(name), lower_service(name, service, execs)?)))
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    let lowered_tasks = tasks
        .iter()
        .map(|(id, task)| {
            Ok((
                TaskId::new(id),
                lower_task(id, task, execs, &lowered_services)?,
            ))
        })
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    // Resolve each environment/workflow reference against the maps just built, so
    // every id the executor later follows is a handle proven to resolve. A miss is
    // an admission rejection here, not a runtime `None`.
    let lowered_workflows = workflows
        .iter()
        .map(|(id, workflow)| {
            Ok((
                id.clone(),
                lower_workflow(workflow, &lowered_services, &lowered_tasks)?,
            ))
        })
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    let environment = match environments.values().next() {
        Some(environment) => lower_environment(environment, &lowered_services, &lowered_tasks)?,
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
        environment,
        workflows: lowered_workflows,
        slot_windows,
    })
}

fn lower_environment(
    environment: &Environment,
    services: &BTreeMap<ServiceId, ExecService>,
    tasks: &BTreeMap<TaskId, ExecTask>,
) -> RuntimeResult<ExecEnvironment> {
    let Environment {
        services: env_services,
        tasks: env_tasks,
    } = environment;
    let resolved_services = env_services
        .iter()
        .map(|id| require_service("environment.services", services, id))
        .collect::<Result<Vec<_>, Rejection>>()?;
    let resolved_tasks = env_tasks
        .iter()
        .map(|id| require_task("environment.tasks", tasks, id))
        .collect::<Result<Vec<_>, Rejection>>()?;
    Ok(ExecEnvironment {
        services: resolved_services,
        tasks: resolved_tasks,
    })
}

fn lower_workflow(
    workflow: &WorkflowSpec,
    services: &BTreeMap<ServiceId, ExecService>,
    tasks: &BTreeMap<TaskId, ExecTask>,
) -> RuntimeResult<ExecWorkflow> {
    let WorkflowSpec {
        services_required,
        nodes,
    } = workflow;
    let resolved_services = services_required
        .iter()
        .map(|id| require_service("workflow.servicesRequired", services, id))
        .collect::<Result<Vec<_>, Rejection>>()?;
    let node_ids = nodes.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let resolved_nodes = nodes
        .iter()
        .map(|(node_id, node)| {
            let task_id = require_task("workflow.node.taskId", tasks, &node.task_id)?;
            let depends_on = node
                .depends_on
                .iter()
                .map(|dependency| {
                    if node_ids.contains(dependency.as_str()) {
                        Ok(dependency.clone())
                    } else {
                        Err(undeclared("workflow.node.dependsOn", dependency.as_str()))
                    }
                })
                .collect::<Result<Vec<_>, Rejection>>()?;
            Ok(ExecWorkflowNode {
                node_id: nixfied_model::NodeId::new(node_id),
                task_id,
                depends_on,
            })
        })
        .collect::<Result<Vec<_>, Rejection>>()?;
    Ok(ExecWorkflow {
        services_required: resolved_services,
        nodes: resolved_nodes,
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

/// Resolve a task reference to a handle proven to exist among the lowered tasks.
fn require_task(
    kind: &'static str,
    tasks: &BTreeMap<TaskId, ExecTask>,
    id: &TaskId,
) -> Result<TaskId, Rejection> {
    if tasks.contains_key(id) {
        Ok(id.clone())
    } else {
        Err(undeclared(kind, id.as_str()))
    }
}

fn lower_service(
    name: &str,
    service: &ServiceSpec,
    execs: &BTreeMap<String, ExecSpec>,
) -> RuntimeResult<ExecService> {
    let ServiceSpec {
        lifecycle,
        endpoint,
        state_refs: _,
        log_refs: _,
        containment,
        identity,
    } = service;
    let Lifecycle {
        prepare,
        start,
        ready,
        health,
        stop,
        clean,
    } = lifecycle;

    let endpoint = ResolvedEndpoint {
        endpoint_id: endpoint.endpoint_id.clone(),
        host: endpoint.host,
    };

    let prepare = PrepareOp {
        meta: op_meta(&prepare.operation_id, &prepare.terminal),
        exec: match &prepare.exec_id {
            Some(exec_id) => Some(resolve_exec_ref(
                execs,
                exec_id.as_str(),
                &prepare.exec_args,
            )?),
            None => None,
        },
    };
    let start = StartOp {
        meta: op_meta(&start.operation_id, &start.terminal),
        exec: resolve_exec_ref(execs, start.exec_id.as_str(), &start.exec_args)?,
    };
    let ready = ReadyOp {
        meta: op_meta(&ready.operation_id, &ready.terminal),
        probe: lower_probe("ready", &ready.probe),
    };
    let health = HealthOp {
        meta: op_meta(&health.operation_id, &health.terminal),
        probe: lower_probe("health", &health.probe),
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
        endpoint,
        containment: containment.clone(),
        identity: identity.clone(),
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

fn lower_task(
    task_id: &str,
    task: &TaskSpec,
    execs: &BTreeMap<String, ExecSpec>,
    services: &BTreeMap<ServiceId, ExecService>,
) -> RuntimeResult<ExecTask> {
    let TaskSpec {
        operation_id: _,
        exec_id,
        args,
        depends_on_services_ready,
        exit_policy,
        artifact_refs: _,
        log_refs: _,
        summary_refs: _,
    } = task;
    let exec = resolve_exec_ref(execs, exec_id.as_str(), args)?;
    let depends_on_services_ready = depends_on_services_ready
        .iter()
        .map(|id| require_service("task.dependsOnServicesReady", services, id))
        .collect::<Result<Vec<_>, Rejection>>()?;
    // `${port}`/`${host}` resolve from the task's primary service. A task that
    // declares no services has no endpoint, so referencing them is unrunnable —
    // reject it here rather than fail at execution.
    if depends_on_services_ready.is_empty() {
        for placeholder in ["${port}", "${host}"] {
            if exec.args.iter().any(|arg| arg.contains(placeholder)) {
                return Err(Rejection::TaskPlaceholderWithoutService {
                    task_id: task_id.to_string(),
                    placeholder,
                }
                .into());
            }
        }
    }
    Ok(ExecTask {
        task_id: TaskId::new(task_id),
        exec,
        depends_on_services_ready,
        success_codes: exit_policy.success_codes.iter().copied().collect(),
    })
}

fn lower_probe(label: &str, probe: &ProbeTiming) -> TcpProbe {
    TcpProbe {
        label: label.to_string(),
        timeout: Duration::from_millis(probe.timeout_ms.get()),
        retry_interval: Duration::from_millis(probe.retry_interval_ms.get()),
        max_attempts: probe.max_attempts.get(),
    }
}

fn resolve_exec_ref(
    execs: &BTreeMap<String, ExecSpec>,
    exec_id: &str,
    extra_args: &[String],
) -> RuntimeResult<ResolvedExec> {
    let exec = execs.get(exec_id).ok_or_else(|| Rejection::MissingExec {
        exec_id: exec_id.to_string(),
    })?;
    Ok(resolve_exec(exec, extra_args))
}

fn resolve_exec(exec: &ExecSpec, extra_args: &[String]) -> ResolvedExec {
    let ExecSpec {
        closure_id: _,
        executable,
        args,
        env,
        codebase_id: _,
        cwd,
        stdin,
        timeout_ms,
    } = exec;
    ResolvedExec {
        executable: executable.clone(),
        args: args.iter().chain(extra_args).cloned().collect(),
        env: env.clone(),
        cwd: cwd.clone(),
        stdin: *stdin,
        timeout: Duration::from_millis(timeout_ms.get()),
    }
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
    MissingExec {
        exec_id: String,
    },
    TaskPlaceholderWithoutService {
        task_id: String,
        placeholder: &'static str,
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
            Rejection::MissingExec { exec_id } => format!("exec {exec_id} is missing"),
            Rejection::TaskPlaceholderWithoutService {
                task_id,
                placeholder,
            } => format!(
                "task {task_id} references {placeholder} but depends on no service to resolve it"
            ),
        }
    }
}

impl From<Rejection> for RuntimeError {
    fn from(rejection: Rejection) -> Self {
        RuntimeError::new(ErrorCode::ModelAdmission, rejection.message())
    }
}

fn undeclared(kind: &'static str, id: impl Into<String>) -> Rejection {
    Rejection::UndeclaredReference {
        kind,
        id: id.into(),
    }
}

/// Prove the *relational* invariants the per-reference resolver in `lower` cannot
/// express on its own: execs bind a declared closure/codebase, closures match the
/// target system, lifecycle/task operation ids are globally unique, an
/// environment/workflow task depends only on services that program starts, and
/// closure operation bindings name a declared operation. The single-reference
/// existence checks (exec/service/task/node ids) are discharged where they are
/// consumed — `lower` resolves each into a typed handle, so a dangling reference
/// is rejected there. Acyclicity is proven separately by the planner.
fn prove_references(model: &Model) -> Result<(), Rejection> {
    let codebase_ids = model
        .codebases
        .iter()
        .map(|codebase| codebase.codebase_id.as_str())
        .collect::<BTreeSet<_>>();
    let closure_ids = model
        .closures
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut declared_operations = BTreeSet::new();

    for exec in model.execs.values() {
        if !closure_ids.contains(exec.closure_id.as_str()) {
            return Err(undeclared("exec.closureId", exec.closure_id.as_str()));
        }
        if !codebase_ids.contains(exec.codebase_id.as_str()) {
            return Err(undeclared("exec.codebaseId", exec.codebase_id.as_str()));
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
    for task in model.tasks.values() {
        if !declared_operations.insert(task.operation_id.as_str()) {
            return Err(Rejection::DuplicateOperationId {
                id: task.operation_id.to_string(),
            });
        }
    }

    // An environment task can only depend on services the environment starts, or
    // the run fails mid-flight on a missing dependency.
    for env in model.environments.values() {
        let env_services = env
            .services
            .iter()
            .map(|service| service.as_str())
            .collect::<BTreeSet<_>>();
        for task_id in &env.tasks {
            if let Some(task) = model.tasks.get(task_id.as_str()) {
                for service in &task.depends_on_services_ready {
                    if !env_services.contains(service.as_str()) {
                        return Err(undeclared(
                            "environment.task.dependsOnServicesReady",
                            service.as_str(),
                        ));
                    }
                }
            }
        }
    }

    // The run plan starts only the workflow's servicesRequired, so a node task may
    // depend only on those.
    for (id, workflow) in &model.workflows {
        if workflow.nodes.is_empty() {
            return Err(undeclared("workflow.nodes", format!("{id} has no nodes")));
        }
        let required = workflow
            .services_required
            .iter()
            .map(|service| service.as_str())
            .collect::<BTreeSet<_>>();
        for node in workflow.nodes.values() {
            if let Some(task) = model.tasks.get(node.task_id.as_str()) {
                for service in &task.depends_on_services_ready {
                    if !required.contains(service.as_str()) {
                        return Err(undeclared(
                            "workflow.node.task.dependsOnServicesReady",
                            service.as_str(),
                        ));
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
                        "candidatePorts": { "start": 38080, "end": 38090 }
                    }
                }
            },
            "state": {
                "markerIdentity": "nixfied-state", "stateEpoch": "1",
                "cleanupPolicy": "delete-on-clean", "persistence": "run-scoped"
            },
            "closures": {
                "c": {
                    "kind": "executable", "storePath": "/nix/store/c", "executable": "/bin/svc",
                    "targetSystem": "x86_64-linux",
                    "operationBindings": ["svc.start", "task.t.run"],
                    "requiresExecutable": true, "effects": ["process"]
                }
            },
            "execs": {
                "svc-exec": {
                    "closureId": "c", "executable": "/bin/svc",
                    "args": ["serve"], "env": {}, "codebaseId": "main", "cwd": ".",
                    "stdin": "null", "timeoutMs": 1000
                },
                "t-exec": {
                    "closureId": "c", "executable": "/bin/task",
                    "args": [], "env": {}, "codebaseId": "main", "cwd": ".",
                    "stdin": "null", "timeoutMs": 1000
                }
            },
            "services": { "svc": service_value() },
            "tasks": {
                "t": {
                    "operationId": "task.t.run", "execId": "t-exec",
                    "args": ["--port", "${port}"], "dependsOnServicesReady": ["svc"],
                    "exitPolicy": { "successCodes": [0] },
                    "artifactRefs": [], "logRefs": [], "summaryRefs": []
                }
            },
            "workflows": {},
            "docs": { "title": "t", "summary": "s" }
        })
    }

    fn service_value() -> Value {
        json!({
            "lifecycle": {
                "prepare": { "operationId": "svc.prepare", "execId": null, "execArgs": [], "terminal": { "success": "prepared", "failure": "failed" } },
                "start": { "operationId": "svc.start", "execId": "svc-exec", "execArgs": ["--port", "${port}"], "terminal": { "success": "spawned", "failure": "failed" } },
                "ready": { "operationId": "svc.ready", "probe": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "ready", "failure": "not-ready" } },
                "health": { "operationId": "svc.health", "probe": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "healthy", "failure": "unhealthy" } },
                "stop": { "operationId": "svc.stop", "signal": "TERM", "timeoutMs": 5000, "terminal": { "success": "stopped", "failure": "failed" } },
                "clean": { "operationId": "svc.clean", "terminal": { "success": "cleaned", "failure": "failed" } }
            },
            "endpoint": { "endpointId": "svc-tcp", "host": "127.0.0.1" },
            "stateRefs": [], "logRefs": [],
            "containment": "process-group",
            "identity": {
                "serviceAddressHash": "a", "endpointIdentityHash": "e", "stateIdentityHash": "s",
                "runtimeCompatibilityHash": "r", "targetIdentityHash": "t"
            }
        })
    }

    fn model_from(value: Value) -> Model {
        serde_json::from_value(value).expect("fixture should deserialize")
    }

    #[test]
    fn lowers_a_valid_model() {
        let em = lower(&model_from(model_value())).expect("valid model lowers");
        let svc = em.services.get("svc").expect("service lowered");
        assert_eq!(svc.endpoint.host.to_string(), "127.0.0.1");
        assert_eq!(svc.stop.signal, StopSignal::Term);
        assert!(svc.prepare.exec.is_none());
        // Start exec args are base ++ operation args.
        assert_eq!(svc.start.exec.args, vec!["serve", "--port", "${port}"]);
        assert_eq!(svc.start.exec.stdin, StdinPolicy::Null);
        assert_eq!(em.environment.services, vec![ServiceId::new("svc")]);
        assert_eq!(em.slot_windows[&0].start, 38080);
        let task = em.tasks.get("t").expect("task lowered");
        assert_eq!(task.success_codes, vec![0]);
        assert_eq!(task.exec.args, vec!["--port", "${port}"]);
        assert_eq!(task.depends_on_services_ready, vec![ServiceId::new("svc")]);
    }

    #[test]
    fn stdin_inherit_is_carried_into_resolved_exec() {
        // A model that declares `stdin: inherit` must lower to an exec that records
        // it, not silently collapse to null.
        let mut value = model_value();
        value["execs"]["svc-exec"]["stdin"] = json!("inherit");
        let em = lower(&model_from(value)).expect("inherit stdin lowers");
        assert_eq!(
            em.services.get("svc").unwrap().start.exec.stdin,
            StdinPolicy::Inherit
        );
    }

    #[test]
    fn rejects_a_missing_exec_reference() {
        let mut value = model_value();
        value["services"]["svc"]["lifecycle"]["start"]["execId"] = json!("ghost");
        let error = lower(&model_from(value)).expect_err("missing exec ref must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }

    fn reject_reason(value: Value) -> Rejection {
        prove_references(&model_from(value)).expect_err("references must not resolve")
    }

    #[test]
    fn execs_must_reference_declared_closures() {
        let mut value = model_value();
        value["execs"]["svc-exec"]["closureId"] = json!("ghost");
        assert_eq!(reject_reason(value), undeclared("exec.closureId", "ghost"));
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
    fn env_task_service_deps_must_be_started_by_the_env() {
        // Task `t` depends on `svc`, but the env no longer starts it.
        let mut value = model_value();
        value["environments"]["dev"]["services"] = json!([]);
        assert_eq!(
            reject_reason(value),
            undeclared("environment.task.dependsOnServicesReady", "svc")
        );
    }

    #[test]
    fn workflow_node_task_must_be_declared() {
        // Node-task existence is now resolved by `lower` (it mints the typed
        // handle), so a dangling `taskId` is rejected there, not in
        // `prove_references`.
        let mut value = model_value();
        value["workflows"]["flow"] = json!({
            "servicesRequired": [],
            "nodes": { "n": { "taskId": "ghost", "dependsOn": [] } }
        });
        let error = lower(&model_from(value)).expect_err("a dangling node task must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
        assert!(error.message.contains("ghost"));
    }

    #[test]
    fn workflow_node_task_deps_must_be_required() {
        // Node task `t` depends on `svc`, absent from the workflow's services.
        let mut value = model_value();
        value["workflows"]["flow"] = json!({
            "servicesRequired": [],
            "nodes": { "n": { "taskId": "t", "dependsOn": [] } }
        });
        assert_eq!(
            reject_reason(value),
            undeclared("workflow.node.task.dependsOnServicesReady", "svc")
        );
    }

    #[test]
    fn service_less_task_lowers() {
        // A task may depend on zero services (e.g. a lint/test task) as long as it
        // does not reference a service-derived placeholder.
        let mut value = model_value();
        value["tasks"]["t"]["dependsOnServicesReady"] = json!([]);
        value["tasks"]["t"]["args"] = json!(["--check"]);
        let em = lower(&model_from(value)).expect("a service-less task lowers");
        assert!(em.tasks["t"].depends_on_services_ready.is_empty());
    }

    #[test]
    fn service_less_task_using_port_is_rejected() {
        // The fixture task's args carry "${port}"; with no service to resolve it,
        // admission must reject rather than admit-then-fail.
        let mut value = model_value();
        value["tasks"]["t"]["dependsOnServicesReady"] = json!([]);
        let error =
            lower(&model_from(value)).expect_err("a service-less task using ${port} must reject");
        assert_eq!(error.code, ErrorCode::ModelAdmission);
    }
}
