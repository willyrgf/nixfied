//! The total lowering `Model -> ExecutionModel`.
//!
//! Every source struct is destructured with **no `..`**, so adding a field to the
//! schema is a compile error here until the lowering consciously maps or refuses
//! it. Rejections are `ModelAdmission` errors — the model was inexpressible to the
//! runtime — never execution failures.

use std::collections::BTreeMap;
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
        .ok_or_else(|| reject("model declares no environment"))?;

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
    Ok(ExecTask {
        task_id: task_id.to_string(),
        exec: resolve_exec_ref(execs, exec_id, args)?,
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
        .ok_or_else(|| reject(format!("exec {exec_id} is missing")))?;
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

fn reject(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::ModelAdmission, message)
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
            "closures": {},
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
}
