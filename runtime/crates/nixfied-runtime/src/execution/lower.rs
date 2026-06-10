//! The total lowering `Model -> ExecutionModel`.
//!
//! Every source struct is destructured with **no `..`**, so adding a field to the
//! schema is a compile error here until the lowering consciously maps or refuses
//! it. Rejections are `ModelAdmission` errors — the model was inexpressible to the
//! runtime — never execution failures.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use nixfied_model::{
    ExecSpec, Lifecycle, Model, ProbeTiming, ServiceSpec, StopSpec, TaskSpec, TerminalSemantics,
    WorkflowSpec,
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
        .map(|(name, service)| Ok((name.clone(), lower_service(name, service, execs)?)))
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    let lowered_tasks = tasks
        .iter()
        .map(|(id, task)| Ok((id.clone(), lower_task(id, task, execs)?)))
        .collect::<RuntimeResult<BTreeMap<_, _>>>()?;

    let lowered_workflows = workflows
        .iter()
        .map(|(id, workflow)| (id.clone(), lower_workflow(workflow)))
        .collect::<BTreeMap<_, _>>();

    let environment = environments
        .values()
        .next()
        .map(lower_environment)
        .ok_or(Rejection::NoEnvironment)?;

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

fn lower_environment(environment: &nixfied_model::Environment) -> ExecEnvironment {
    let nixfied_model::Environment { services, tasks } = environment;
    ExecEnvironment {
        services: services.clone(),
        tasks: tasks.clone(),
    }
}

fn lower_workflow(workflow: &WorkflowSpec) -> ExecWorkflow {
    let WorkflowSpec {
        services_required,
        nodes,
    } = workflow;
    ExecWorkflow {
        services_required: services_required.clone(),
        nodes: nodes
            .iter()
            .map(|(node_id, node)| ExecWorkflowNode {
                node_id: node_id.clone(),
                task_id: node.task_id.clone(),
                depends_on: node.depends_on.clone(),
            })
            .collect(),
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
            Some(exec_id) => Some(resolve_exec_ref(execs, exec_id, &prepare.exec_args)?),
            None => None,
        },
    };
    let start = StartOp {
        meta: op_meta(&start.operation_id, &start.terminal),
        exec: resolve_exec_ref(execs, &start.exec_id, &start.exec_args)?,
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
        name: name.to_string(),
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

fn op_meta(operation_id: &str, terminal: &TerminalSemantics) -> OpMeta {
    OpMeta {
        operation_id: operation_id.to_string(),
        terminal_success: terminal.success.clone(),
        terminal_failure: terminal.failure.clone(),
    }
}

fn lower_task(
    task_id: &str,
    task: &TaskSpec,
    execs: &BTreeMap<String, ExecSpec>,
) -> RuntimeResult<ExecTask> {
    let TaskSpec {
        operation_id: _,
        exec_id,
        args,
        depends_on_services_ready,
        exit_policy,
        output_capture: _,
        artifact_refs: _,
        log_refs: _,
        summary_refs: _,
    } = task;
    let exec = resolve_exec_ref(execs, exec_id, args)?;
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
        task_id: task_id.to_string(),
        exec,
        depends_on_services_ready: depends_on_services_ready.clone(),
        success_codes: exit_policy.success_codes.clone(),
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
    let exec = execs
        .get(exec_id)
        .ok_or_else(|| Rejection::MissingExec {
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
        stdin: _,
        timeout_ms,
        output_capture: _,
        cancellation_mode: _,
    } = exec;
    ResolvedExec {
        executable: executable.clone(),
        args: args.iter().chain(extra_args).cloned().collect(),
        env: env.clone(),
        cwd: cwd.clone(),
        timeout: Duration::from_millis(timeout_ms.get()),
    }
}

/// The closed set of reasons the model cannot be lowered into an executable
/// program. Every relational/reference check the runtime needs lives here, so a
/// successful `lower` is a proof the references resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    NoEnvironment,
    UndeclaredReference { kind: &'static str, id: String },
    DuplicateOperationId { id: String },
    UnknownOperationBinding { binding: String },
    ClosureTargetMismatch {
        closure_id: String,
        target_system: String,
        closure_system: String,
    },
    MissingExec { exec_id: String },
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

/// Prove every cross-reference in the model resolves: execs to closures/codebases,
/// closures to the target, lifecycle/task operations to unique ids and declared
/// execs, environment and workflow programs to declared services/tasks, and
/// closure operation bindings to declared operations. A successful return is the
/// proof that the references the executor follows exist. Acyclicity is proven
/// separately by the planner over every workflow at admission.
fn prove_references(model: &Model) -> Result<(), Rejection> {
    let codebase_ids = model
        .codebases
        .iter()
        .map(|codebase| codebase.codebase_id.as_str())
        .collect::<BTreeSet<_>>();
    let closure_ids = model.closures.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let exec_ids = model.execs.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let service_ids = model.services.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let task_ids = model.tasks.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let mut declared_operations = BTreeSet::new();

    for exec in model.execs.values() {
        if !closure_ids.contains(exec.closure_id.as_str()) {
            return Err(undeclared("exec.closureId", exec.closure_id.clone()));
        }
        if !codebase_ids.contains(exec.codebase_id.as_str()) {
            return Err(undeclared("exec.codebaseId", exec.codebase_id.clone()));
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

    for service in model.services.values() {
        let lifecycle = &service.lifecycle;
        for operation_id in lifecycle_op_ids(lifecycle) {
            if !declared_operations.insert(operation_id) {
                return Err(Rejection::DuplicateOperationId {
                    id: operation_id.to_string(),
                });
            }
        }
        for exec_id in [
            lifecycle.prepare.exec_id.as_deref(),
            Some(lifecycle.start.exec_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if !exec_ids.contains(exec_id) {
                return Err(undeclared("lifecycle.execId", exec_id));
            }
        }
    }

    for task in model.tasks.values() {
        if !declared_operations.insert(task.operation_id.as_str()) {
            return Err(Rejection::DuplicateOperationId {
                id: task.operation_id.clone(),
            });
        }
        if !exec_ids.contains(task.exec_id.as_str()) {
            return Err(undeclared("task.execId", task.exec_id.clone()));
        }
        for service_id in &task.depends_on_services_ready {
            if !service_ids.contains(service_id.as_str()) {
                return Err(undeclared("task.dependsOnServicesReady", service_id.clone()));
            }
        }
    }

    for env in model.environments.values() {
        let env_services = env.services.iter().map(String::as_str).collect::<BTreeSet<_>>();
        for service_id in &env.services {
            if !service_ids.contains(service_id.as_str()) {
                return Err(undeclared("environment.services", service_id.clone()));
            }
        }
        for task_id in &env.tasks {
            if !task_ids.contains(task_id.as_str()) {
                return Err(undeclared("environment.tasks", task_id.clone()));
            }
            // An environment task can only depend on services the environment
            // starts, or the run fails mid-flight on a missing dependency.
            if let Some(task) = model.tasks.get(task_id) {
                for service in &task.depends_on_services_ready {
                    if !env_services.contains(service.as_str()) {
                        return Err(undeclared(
                            "environment.task.dependsOnServicesReady",
                            service.clone(),
                        ));
                    }
                }
            }
        }
    }

    for (id, workflow) in &model.workflows {
        let required = workflow
            .services_required
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for service in &workflow.services_required {
            if !service_ids.contains(service.as_str()) {
                return Err(undeclared("workflow.servicesRequired", service.clone()));
            }
        }
        if workflow.nodes.is_empty() {
            return Err(undeclared("workflow.nodes", format!("{id} has no nodes")));
        }
        for node in workflow.nodes.values() {
            if !task_ids.contains(node.task_id.as_str()) {
                return Err(undeclared("workflow.node.taskId", node.task_id.clone()));
            }
            for dependency in &node.depends_on {
                if !workflow.nodes.contains_key(dependency) {
                    return Err(undeclared("workflow.node.dependsOn", dependency.clone()));
                }
            }
            // The run plan starts only the workflow's servicesRequired, so a node
            // task may depend only on those.
            if let Some(task) = model.tasks.get(&node.task_id) {
                for service in &task.depends_on_services_ready {
                    if !required.contains(service.as_str()) {
                        return Err(undeclared(
                            "workflow.node.task.dependsOnServicesReady",
                            service.clone(),
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
                    binding: binding.clone(),
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
                    "stdin": "null", "timeoutMs": 1000, "outputCapture": "stdout-stderr",
                    "cancellationMode": "kill-process-group"
                },
                "t-exec": {
                    "closureId": "c", "executable": "/bin/task",
                    "args": [], "env": {}, "codebaseId": "main", "cwd": ".",
                    "stdin": "null", "timeoutMs": 1000, "outputCapture": "stdout-stderr",
                    "cancellationMode": "kill-process-group"
                }
            },
            "services": { "svc": service_value() },
            "tasks": {
                "t": {
                    "operationId": "task.t.run", "execId": "t-exec",
                    "args": ["--port", "${port}"], "dependsOnServicesReady": ["svc"],
                    "exitPolicy": { "successCodes": [0] }, "outputCapture": "stdout-stderr",
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
        assert_eq!(em.environment.services, vec!["svc".to_string()]);
        assert_eq!(em.slot_windows[&0].start, 38080);
        let task = em.tasks.get("t").expect("task lowered");
        assert_eq!(task.success_codes, vec![0]);
        assert_eq!(task.exec.args, vec!["--port", "${port}"]);
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
        assert_eq!(
            reject_reason(value),
            undeclared("exec.closureId", "ghost")
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
        let mut value = model_value();
        value["workflows"]["flow"] = json!({
            "servicesRequired": [],
            "nodes": { "n": { "taskId": "ghost", "dependsOn": [] } }
        });
        assert_eq!(
            reject_reason(value),
            undeclared("workflow.node.taskId", "ghost")
        );
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
