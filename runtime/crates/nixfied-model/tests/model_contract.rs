use nixfied_model::{MODEL_VERSION, Model, RUNTIME_ABI, TOOLCHAIN_ID, Validate, ValidationError};
use serde_json::{Value, json};

fn valid_model_json() -> Value {
    json!({
        "modelVersion": MODEL_VERSION,
        "toolchainId": TOOLCHAIN_ID,
        "runtimeAbi": RUNTIME_ABI,
        "generator": {
            "name": "nixfied",
            "version": "1",
            "emitter": "nix/compiler/emit-model.nix"
        },
        "project": {
            "projectId": "example",
            "name": "M0 Example"
        },
        "target": {
            "system": "aarch64-darwin",
            "os": "darwin",
            "arch": "aarch64",
            "closureSystem": "aarch64-darwin",
            "requiredRuntimeCapabilities": {
                "processGroup": true,
                "tcpPortOwnership": true,
                "sqliteWal": true
            }
        },
        "codebases": [{
            "codebaseId": "main",
            "logicalRoot": ".",
            "sourceMode": "live-workspace",
            "sourceIdentity": "live",
            "sourcePolicy": {
                "dirtyPolicy": "warn",
                "admissionFingerprintPolicy": "live-fingerprint"
            }
        }],
        "environments": {
            "dev": {
                "environmentId": "dev",
                "services": ["synthetic"],
                "tasks": ["smoke"]
            }
        },
        "slotPolicy": {
            "min": 0,
            "default": 0,
            "max": 0
        },
        "capabilities": {
            "environments": ["dev"],
            "slots": [0],
            "services": ["synthetic"],
            "tasks": ["smoke"],
            "workflows": [],
            "surfaces": m0_surface_names()
        },
        "runtimeConstraints": {
            "allowedEnvironments": ["dev"],
            "slotMin": 0,
            "slotDefault": 0,
            "slotMax": 0,
            "allowPortOverride": false,
            "collisionPolicy": "fail"
        },
        "surfaces": m0_surfaces(),
        "placement": {
            "stateRootTemplate": "${projectId}/${environment}/${slot}",
            "registryDir": "registry",
            "runDirTemplate": "runs/${runId}",
            "logsDirTemplate": "runs/${runId}/logs",
            "artifactsDirTemplate": "runs/${runId}/artifacts",
            "candidatePorts": {
                "start": 38080,
                "end": 38090
            },
            "slotPlacements": {
                "0": {
                    "slot": 0,
                    "stateRootTemplate": "${projectId}/${environment}/${slot}",
                    "registryDir": "registry",
                    "runDirTemplate": "runs/${runId}",
                    "logsDirTemplate": "runs/${runId}/logs",
                    "artifactsDirTemplate": "runs/${runId}/artifacts",
                    "candidatePorts": {
                        "start": 38080,
                        "end": 38090
                    }
                }
            }
        },
        "state": {
            "markerIdentity": "nixfied-state",
            "stateEpoch": "1",
            "cleanupPolicy": "delete-on-clean",
            "persistence": "run-scoped"
        },
        "secrets": [],
        "closures": [{
            "closureId": "synthetic-helper",
            "kind": "executable",
            "storePath": "/nix/store/00000000000000000000000000000000-synthetic-helper",
            "executable": "/nix/store/00000000000000000000000000000000-synthetic-helper/bin/synthetic-helper",
            "targetSystem": "aarch64-darwin",
            "operationBindings": [
                "service.synthetic.start",
                "service.synthetic.stop",
                "task.smoke.run"
            ],
            "requiresExecutable": true,
            "effects": ["process", "network-listener"]
        }],
        "execs": {
            "synthetic-helper": helper_exec()
        },
        "services": {
            "synthetic": synthetic_service()
        },
        "tasks": {
            "smoke": smoke_task()
        },
        "workflows": {},
        "docs": {
            "title": "M0 Example",
            "summary": "Minimal M0 model contract fixture."
        }
    })
}

fn helper_exec() -> Value {
    json!({
        "execId": "synthetic-helper",
        "closureId": "synthetic-helper",
        "executable": "/nix/store/00000000000000000000000000000000-synthetic-helper/bin/synthetic-helper",
        "args": [],
        "env": {},
        "codebaseId": "main",
        "cwd": ".",
        "stdin": "null",
        "timeoutMs": 30000,
        "outputCapture": "stdout-stderr",
        "cancellationMode": "kill-process-group"
    })
}

fn synthetic_service() -> Value {
    json!({
        "serviceId": "synthetic",
        "foreground": true,
        "lifecycle": [
            {
                "operationId": "service.synthetic.prepare",
                "class": "prepare",
                "execId": null,
                "execArgs": [],
                "probeId": null,
                "terminal": { "success": "prepared", "failure": "failed" }
            },
            {
                "operationId": "service.synthetic.start",
                "class": "start",
                "execId": "synthetic-helper",
                "execArgs": ["service", "--host", "127.0.0.1", "--port", "${port}"],
                "probeId": null,
                "terminal": { "success": "spawned", "failure": "failed" }
            },
            {
                "operationId": "service.synthetic.ready",
                "class": "ready",
                "execId": null,
                "execArgs": [],
                "probeId": "synthetic-tcp",
                "terminal": { "success": "ready", "failure": "not-ready" }
            },
            {
                "operationId": "service.synthetic.health",
                "class": "health",
                "execId": null,
                "execArgs": [],
                "probeId": "synthetic-tcp",
                "terminal": { "success": "healthy", "failure": "unhealthy" }
            },
            {
                "operationId": "service.synthetic.stop",
                "class": "stop",
                "execId": null,
                "execArgs": [],
                "probeId": null,
                "terminal": { "success": "stopped", "failure": "failed" }
            },
            {
                "operationId": "service.synthetic.clean",
                "class": "clean",
                "execId": null,
                "execArgs": [],
                "probeId": null,
                "terminal": { "success": "cleaned", "failure": "failed" }
            }
        ],
        "endpoints": [{
            "endpointId": "synthetic-tcp",
            "protocol": "tcp",
            "host": "127.0.0.1",
            "port": { "kind": "candidate-window", "start": 38080, "end": 38090 },
            "ownershipVerification": "required",
            "socketActivation": "disabled"
        }],
        "probes": [{
            "probeId": "synthetic-tcp",
            "target": { "kind": "tcp-connect", "endpointId": "synthetic-tcp" },
            "timeoutMs": 1000,
            "retryIntervalMs": 100,
            "maxAttempts": 20
        }],
        "readinessProbe": "synthetic-tcp",
        "healthPolicy": "explicit",
        "stopPolicy": { "signal": "TERM", "timeoutMs": 5000 },
        "stateRefs": ["slot"],
        "logRefs": ["service.synthetic"],
        "containment": "process-group",
        "lifetime": "run-scoped",
        "identity": {
            "serviceAddressHash": "service-address",
            "endpointIdentityHash": "endpoint",
            "stateIdentityHash": "state",
            "runtimeCompatibilityHash": "runtime",
            "targetIdentityHash": "target"
        }
    })
}

fn smoke_task() -> Value {
    json!({
        "taskId": "smoke",
        "operationId": "task.smoke.run",
        "execId": "synthetic-helper",
        "args": ["task", "--host", "127.0.0.1", "--port", "${port}"],
        "dependsOnServicesReady": ["synthetic"],
        "exitPolicy": { "successCodes": [0] },
        "outputCapture": "stdout-stderr",
        "artifactRefs": [],
        "logRefs": ["task.smoke"],
        "summaryRefs": ["summary"]
    })
}

fn m0_surface_names() -> Vec<&'static str> {
    vec![
        "model",
        "schema",
        "docs",
        "capabilities",
        "check",
        "run",
        "ps",
        "down",
        "clean",
    ]
}

fn m0_surfaces() -> Vec<Value> {
    m0_surface_names()
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "aliases": [],
                "inputSchema": {},
                "outputSchema": {},
                "exitClasses": ["ok", "error"],
                "evaluationPermission": "never",
                "maturity": "stable"
            })
        })
        .collect()
}

fn parse_valid_model() -> Model {
    serde_json::from_value(valid_model_json()).expect("valid model JSON should deserialize")
}

/// Adds a second service named `worker` that reuses the helper exec/closure but
/// binds its own lifecycle, endpoint, and probe. Used to prove structural
/// validation accepts arbitrary service counts.
fn add_worker_service(value: &mut Value) {
    value["closures"][0]["operationBindings"]
        .as_array_mut()
        .unwrap()
        .extend([json!("service.worker.start"), json!("service.worker.stop")]);

    let mut worker = synthetic_service();
    worker["serviceId"] = json!("worker");
    worker["readinessProbe"] = json!("worker-tcp");
    worker["endpoints"][0]["endpointId"] = json!("worker-tcp");
    worker["probes"][0]["probeId"] = json!("worker-tcp");
    worker["probes"][0]["target"]["endpointId"] = json!("worker-tcp");
    for op in worker["lifecycle"].as_array_mut().unwrap() {
        let class = op["class"].as_str().unwrap();
        op["operationId"] = json!(format!("service.worker.{class}"));
        if op["probeId"].is_string() {
            op["probeId"] = json!("worker-tcp");
        }
    }
    value["services"]["worker"] = worker;
    value["environments"]["dev"]["services"] = json!(["synthetic", "worker"]);
    value["capabilities"]["services"] = json!(["synthetic", "worker"]);
}

fn lifecycle_op_mut<'a>(
    model: &'a mut Model,
    operation_id: &str,
) -> &'a mut nixfied_model::LifecycleOpSpec {
    model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .lifecycle
        .iter_mut()
        .find(|op| op.operation_id == operation_id)
        .expect("fixture has lifecycle operation")
}

#[test]
fn parses_and_validates_contract() {
    let model = parse_valid_model();
    model
        .validate()
        .expect("valid model should pass structural validation");

    let classes = model.services["synthetic"]
        .lifecycle
        .iter()
        .map(|op| op.class.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        classes,
        [
            nixfied_model::LifecycleOpClass::Prepare,
            nixfied_model::LifecycleOpClass::Start,
            nixfied_model::LifecycleOpClass::Ready,
            nixfied_model::LifecycleOpClass::Health,
            nixfied_model::LifecycleOpClass::Stop,
            nixfied_model::LifecycleOpClass::Clean,
        ]
    );
}

#[test]
fn accepts_arbitrary_service_and_exec_names() {
    // The synthetic/smoke names are not special. A second service and a second
    // exec validate as long as the structural contract holds.
    let mut value = valid_model_json();
    add_worker_service(&mut value);
    let mut second_exec = helper_exec();
    second_exec["execId"] = json!("aux-helper");
    value["execs"]["aux-helper"] = second_exec;

    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect("multi-service / multi-exec models are valid");
}

#[test]
fn service_may_not_reference_another_services_probe() {
    // Probe ids are service-local. A second service pointing its readinessProbe at
    // the first service's probe must be rejected at admission (fail closed), not
    // admitted and then fail when the runtime resolves it within the ServiceSpec.
    let mut value = valid_model_json();
    add_worker_service(&mut value);
    // Point the worker's readiness and ready/health probes at the synthetic
    // service's probe (self-consistent within the worker, but cross-service).
    value["services"]["worker"]["readinessProbe"] = json!("synthetic-tcp");
    for op in value["services"]["worker"]["lifecycle"]
        .as_array_mut()
        .unwrap()
    {
        if op["probeId"].is_string() {
            op["probeId"] = json!("synthetic-tcp");
        }
    }
    let model: Model = serde_json::from_value(value).expect("model should deserialize");

    assert_eq!(
        model
            .validate()
            .expect_err("cross-service probe reference must be rejected"),
        ValidationError::UndeclaredReference {
            reference_kind: "lifecycle.probeId",
            id: "synthetic-tcp".to_string(),
        }
    );
}

#[test]
fn probe_may_not_target_another_services_endpoint() {
    // Endpoint ids are service-local too: a probe may only target an endpoint its
    // own service declares.
    let mut value = valid_model_json();
    add_worker_service(&mut value);
    value["services"]["worker"]["probes"][0]["target"]["endpointId"] = json!("synthetic-tcp");
    let model: Model = serde_json::from_value(value).expect("model should deserialize");

    assert_eq!(
        model
            .validate()
            .expect_err("cross-service endpoint reference must be rejected"),
        ValidationError::UndeclaredReference {
            reference_kind: "probe.endpointId",
            id: "synthetic-tcp".to_string(),
        }
    );
}

#[test]
fn prepare_operation_may_bind_an_exec() {
    // initdb-style preparation: the prepare class is allowed to bind an exec.
    let mut model = parse_valid_model();
    lifecycle_op_mut(&mut model, "service.synthetic.prepare").exec_id =
        Some("synthetic-helper".to_string());
    model.validate().expect("prepare may bind a generic exec");
}

#[test]
fn capabilities_services_must_mirror_model() {
    let mut value = valid_model_json();
    value["capabilities"]["services"] = json!(["synthetic", "ghost"]);
    let model: Model = serde_json::from_value(value).expect("model should deserialize");

    match model
        .validate()
        .expect_err("capabilities must mirror declared services")
    {
        ValidationError::UnsupportedValue { field, .. } => {
            assert_eq!(field, "capabilities.services");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn old_single_surface_contract_is_rejected() {
    let mut model = parse_valid_model();
    model.capabilities.surfaces = vec!["model".to_string()];
    model.surfaces.retain(|surface| surface.name == "model");

    match model
        .validate()
        .expect_err("runtime surfaces must be the full set")
    {
        ValidationError::UnsupportedValue { field, .. } => assert_eq!(field, "surfaces"),
        other => panic!("unexpected validation error: {other:?}"),
    }
}

#[test]
fn validates_explicit_slot_placement_range() {
    let mut value = valid_model_json();
    value["slotPolicy"]["max"] = json!(1);
    value["runtimeConstraints"]["slotMax"] = json!(1);
    value["capabilities"]["slots"] = json!([0, 1]);
    let placement = value["placement"]["slotPlacements"]["0"].clone();
    let mut slot_one = placement;
    slot_one["slot"] = json!(1);
    slot_one["candidatePorts"] = json!({ "start": 38180, "end": 38190 });
    value["placement"]["slotPlacements"]["1"] = slot_one;

    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect("explicit slot placements should cover the slot range");
}

#[test]
fn unknown_top_level_field_is_invalid() {
    let mut value = valid_model_json();
    value["computedModelHash"] = json!("must-not-be-embedded");

    let error = serde_json::from_value::<Model>(value).expect_err("unknown field must be refused");
    assert!(error.to_string().contains("computedModelHash"));
}

#[test]
fn abi_mismatch_is_contract_error() {
    let mut model = parse_valid_model();
    model.runtime_abi = "nixfied-runtime-abi:legacy".to_string();

    assert_eq!(
        model.validate().expect_err("ABI mismatch should fail"),
        ValidationError::RuntimeAbi {
            expected: RUNTIME_ABI,
            actual: "nixfied-runtime-abi:legacy".to_string(),
        }
    );
}

#[test]
fn non_empty_secrets_are_rejected() {
    let mut model = parse_valid_model();
    model.secrets.push(nixfied_model::SecretRef {
        secret_id: "db".to_string(),
        target: "env:DB_PASSWORD".to_string(),
        required: true,
    });

    assert_eq!(
        model.validate().expect_err("secrets are still deferred"),
        ValidationError::MustBeEmpty { field: "secrets" }
    );
}

fn with_workflow(value: &mut Value, nodes: Value) {
    value["workflows"]["pipeline"] = json!({
        "workflowId": "pipeline",
        "servicesRequired": ["synthetic"],
        "nodes": nodes,
    });
    value["capabilities"]["workflows"] = json!(["pipeline"]);
}

#[test]
fn accepts_a_bounded_acyclic_workflow() {
    let mut value = valid_model_json();
    with_workflow(
        &mut value,
        json!([
            { "nodeId": "first", "taskId": "smoke", "dependsOn": [] },
            { "nodeId": "second", "taskId": "smoke", "dependsOn": ["first"] }
        ]),
    );
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect("a bounded acyclic workflow is valid");
}

#[test]
fn workflow_node_task_service_deps_must_be_required() {
    let mut value = valid_model_json();
    // The node's task `smoke` depends on `synthetic`, but the workflow does not
    // declare it in servicesRequired, so the run plan would never start it.
    value["workflows"]["pipeline"] = json!({
        "workflowId": "pipeline",
        "servicesRequired": [],
        "nodes": [{ "nodeId": "first", "taskId": "smoke", "dependsOn": [] }],
    });
    value["capabilities"]["workflows"] = json!(["pipeline"]);
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    assert_eq!(
        model
            .validate()
            .expect_err("node task service dependency must be in servicesRequired"),
        ValidationError::UndeclaredReference {
            reference_kind: "workflow.node.task.dependsOnServicesReady",
            id: "synthetic".to_string(),
        }
    );
}

#[test]
fn rejects_cyclic_workflow() {
    let mut value = valid_model_json();
    with_workflow(
        &mut value,
        json!([
            { "nodeId": "a", "taskId": "smoke", "dependsOn": ["b"] },
            { "nodeId": "b", "taskId": "smoke", "dependsOn": ["a"] }
        ]),
    );
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    match model
        .validate()
        .expect_err("cyclic workflow must be rejected")
    {
        ValidationError::UnsupportedValue { field, .. } => {
            assert_eq!(field, "workflows.nodes.dependsOn")
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn workflow_nodes_must_reference_declared_tasks() {
    let mut value = valid_model_json();
    with_workflow(
        &mut value,
        json!([{ "nodeId": "n", "taskId": "ghost", "dependsOn": [] }]),
    );
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    assert_eq!(
        model
            .validate()
            .expect_err("workflow node task must be declared"),
        ValidationError::UndeclaredReference {
            reference_kind: "workflow.node.taskId",
            id: "ghost".to_string(),
        }
    );
}

#[test]
fn env_task_service_deps_must_be_declared_in_env_services() {
    let mut value = valid_model_json();
    // The env runs `smoke` (which depends on `synthetic`) but does not start it.
    value["environments"]["dev"]["services"] = json!([]);
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    assert_eq!(
        model
            .validate()
            .expect_err("env task dependency must be in env services"),
        ValidationError::UndeclaredReference {
            reference_kind: "environment.task.dependsOnServicesReady",
            id: "synthetic".to_string(),
        }
    );
}

#[test]
fn rejects_http_get_probe_for_tcp_only_abi() {
    let mut value = valid_model_json();
    value["services"]["synthetic"]["probes"][0]["target"] = json!({
        "kind": "http-get",
        "endpointId": "synthetic-tcp",
        "path": "/health",
    });
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    match model
        .validate()
        .expect_err("http-get probes are not supported by the current ABI")
    {
        ValidationError::UnsupportedValue { field, .. } => {
            assert_eq!(field, "services.probes.target.kind")
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn host_absolute_placement_is_rejected() {
    let mut model = parse_valid_model();
    model.placement.state_root_template = "/tmp/nixfied".to_string();

    assert_eq!(
        model
            .validate()
            .expect_err("host absolute placement should fail"),
        ValidationError::HostAbsolutePath {
            field: "placement.stateRootTemplate",
            value: "/tmp/nixfied".to_string(),
        }
    );
}

#[test]
fn closure_bindings_must_reference_declared_operations() {
    let mut model = parse_valid_model();
    model.closures[0]
        .operation_bindings
        .push("workflow.deferred.run".to_string());

    assert_eq!(
        model
            .validate()
            .expect_err("undeclared operation binding should fail"),
        ValidationError::UnknownOperationBinding {
            binding: "workflow.deferred.run".to_string(),
        }
    );
}

#[test]
fn execs_must_reference_declared_closures() {
    let mut model = parse_valid_model();
    model
        .execs
        .get_mut("synthetic-helper")
        .expect("fixture has helper exec")
        .closure_id = "ghost-closure".to_string();

    assert_eq!(
        model.validate().expect_err("exec closure must be declared"),
        ValidationError::UndeclaredReference {
            reference_kind: "exec.closureId",
            id: "ghost-closure".to_string(),
        }
    );
}

#[test]
fn runtime_constraints_must_use_fail_collision_policy() {
    let mut model = parse_valid_model();
    model.runtime_constraints.collision_policy = nixfied_model::CollisionPolicy::ProbeInRange;

    assert_eq!(
        model
            .validate()
            .expect_err("only fail collision policy is supported"),
        ValidationError::UnsupportedValue {
            field: "runtimeConstraints.collisionPolicy",
            expected: "fail",
            actual: "ProbeInRange".to_string(),
        }
    );
}

#[test]
fn lifecycle_must_bind_readiness_probe_on_ready() {
    let mut model = parse_valid_model();
    lifecycle_op_mut(&mut model, "service.synthetic.ready").probe_id = None;

    match model
        .validate()
        .expect_err("ready lifecycle must bind the readiness probe")
    {
        ValidationError::UnsupportedValue { field, .. } => {
            assert_eq!(field, "lifecycle.ready.probeId")
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn lifecycle_must_have_full_generic_class_set() {
    let mut model = parse_valid_model();
    model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .lifecycle
        .retain(|op| op.operation_id != "service.synthetic.clean");

    match model
        .validate()
        .expect_err("full lifecycle contract requires clean declaration")
    {
        ValidationError::UnsupportedValue { field, actual, .. } => {
            assert_eq!(field, "lifecycle.class");
            assert!(actual.contains("clean"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn ready_and_health_must_remain_distinct_classes() {
    let mut model = parse_valid_model();
    lifecycle_op_mut(&mut model, "service.synthetic.health").class =
        nixfied_model::LifecycleOpClass::Ready;

    // Two ops now share the Ready class; the per-class uniqueness check fires.
    match model
        .validate()
        .expect_err("health must not be conflated with readiness")
    {
        ValidationError::UnsupportedValue { field, .. } => assert_eq!(field, "lifecycle.class"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn health_policy_must_be_explicit() {
    let mut model = parse_valid_model();
    model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .health_policy = nixfied_model::HealthPolicy::Unsupported;

    assert_eq!(
        model
            .validate()
            .expect_err("declared health uses an explicit typed policy"),
        ValidationError::UnsupportedValue {
            field: "services.healthPolicy",
            expected: "explicit",
            actual: "Unsupported".to_string(),
        }
    );
}

#[test]
fn clean_operation_stays_marker_gated_runtime_cleanup() {
    let mut model = parse_valid_model();
    lifecycle_op_mut(&mut model, "service.synthetic.clean").exec_id =
        Some("synthetic-helper".to_string());

    assert_eq!(
        model
            .validate()
            .expect_err("clean must stay a runtime cleanup primitive"),
        ValidationError::UnsupportedValue {
            field: "lifecycle.clean.execId",
            expected: "null",
            actual: "synthetic-helper".to_string(),
        }
    );
}

#[test]
fn lifecycle_operation_ids_must_be_unique() {
    let mut model = parse_valid_model();
    lifecycle_op_mut(&mut model, "service.synthetic.health").operation_id =
        "service.synthetic.ready".to_string();

    match model
        .validate()
        .expect_err("duplicate lifecycle operation IDs must be rejected")
    {
        ValidationError::UnsupportedValue { field, actual, .. } => {
            assert_eq!(field, "lifecycle.operationId");
            assert_eq!(actual, "service.synthetic.ready");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
