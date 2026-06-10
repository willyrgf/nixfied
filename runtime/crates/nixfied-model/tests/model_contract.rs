use nixfied_model::{MODEL_VERSION, Model, TOOLCHAIN_ID, Validate, ValidationError, runtime_abi};
use serde_json::{Value, json};

fn valid_model_json() -> Value {
    json!({
        "modelVersion": MODEL_VERSION,
        "toolchainId": TOOLCHAIN_ID,
        "runtimeAbi": runtime_abi(),
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
            "closureSystem": "aarch64-darwin"
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
                "services": ["synthetic"],
                "tasks": ["smoke"]
            }
        },
        "slotPolicy": {
            "min": 0,
            "default": 0,
            "max": 0
        },
        "placement": {
            "slotPlacements": {
                "0": {
                    "slot": 0,
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
        "closures": {
            "synthetic-helper": {
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
            }
        },
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
        "lifecycle": {
            "prepare": { "operationId": "service.synthetic.prepare", "execId": null, "execArgs": [], "terminal": { "success": "prepared", "failure": "failed" } },
            "start": { "operationId": "service.synthetic.start", "execId": "synthetic-helper", "execArgs": ["service", "--host", "127.0.0.1", "--port", "${port}"], "terminal": { "success": "spawned", "failure": "failed" } },
            "ready": { "operationId": "service.synthetic.ready", "probe": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "ready", "failure": "not-ready" } },
            "health": { "operationId": "service.synthetic.health", "probe": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "healthy", "failure": "unhealthy" } },
            "stop": { "operationId": "service.synthetic.stop", "signal": "TERM", "timeoutMs": 5000, "terminal": { "success": "stopped", "failure": "failed" } },
            "clean": { "operationId": "service.synthetic.clean", "terminal": { "success": "cleaned", "failure": "failed" } }
        },
        "endpoint": { "endpointId": "synthetic-tcp", "host": "127.0.0.1" },
        "stateRefs": ["slot"],
        "logRefs": ["service.synthetic"],
        "containment": "process-group",
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

fn parse_valid_model() -> Model {
    serde_json::from_value(valid_model_json()).expect("valid model JSON should deserialize")
}

/// Adds a second service named `worker` that reuses the helper exec/closure but
/// binds its own lifecycle, endpoint, and probe. Used to prove structural
/// validation accepts arbitrary service counts.
fn add_worker_service(value: &mut Value) {
    value["closures"]["synthetic-helper"]["operationBindings"]
        .as_array_mut()
        .unwrap()
        .push(json!("service.worker.start"));

    let mut worker = synthetic_service();
    worker["endpoint"]["endpointId"] = json!("worker-tcp");
    for (class, op) in worker["lifecycle"].as_object_mut().unwrap() {
        op["operationId"] = json!(format!("service.worker.{class}"));
    }
    value["services"]["worker"] = worker;
    value["environments"]["dev"]["services"] = json!(["synthetic", "worker"]);
}

fn synthetic_lifecycle_mut(model: &mut Model) -> &mut nixfied_model::Lifecycle {
    &mut model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .lifecycle
}

#[test]
fn parses_and_validates_contract() {
    let model = parse_valid_model();
    model
        .validate()
        .expect("valid model should pass structural validation");

    // The lifecycle is a per-class record: every class is present by construction.
    let lifecycle = &model.services["synthetic"].lifecycle;
    assert_eq!(lifecycle.start.operation_id, "service.synthetic.start");
    assert_eq!(lifecycle.stop.signal, nixfied_model::StopSignal::Term);
}

#[test]
fn accepts_arbitrary_service_and_exec_names() {
    // The synthetic/smoke names are not special. A second service and a second
    // exec validate as long as the structural contract holds.
    let mut value = valid_model_json();
    add_worker_service(&mut value);
    value["execs"]["aux-helper"] = helper_exec();

    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect("multi-service / multi-exec models are valid");
}

#[test]
fn prepare_operation_may_bind_an_exec() {
    // initdb-style preparation: the prepare class is allowed to bind an exec.
    let mut model = parse_valid_model();
    synthetic_lifecycle_mut(&mut model).prepare.exec_id = Some("synthetic-helper".to_string());
    model.validate().expect("prepare may bind a generic exec");
}

#[test]
fn validates_explicit_slot_placement_range() {
    let mut value = valid_model_json();
    value["slotPolicy"]["max"] = json!(1);
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
            expected: runtime_abi(),
            actual: "nixfied-runtime-abi:legacy".to_string(),
        }
    );
}

fn with_workflow(value: &mut Value, nodes: Value) {
    value["workflows"]["pipeline"] = json!({
        "servicesRequired": ["synthetic"],
        "nodes": nodes,
    });
}

#[test]
fn accepts_a_bounded_acyclic_workflow() {
    let mut value = valid_model_json();
    with_workflow(
        &mut value,
        json!({
            "first": { "taskId": "smoke", "dependsOn": [] },
            "second": { "taskId": "smoke", "dependsOn": ["first"] }
        }),
    );
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect("a bounded acyclic workflow is valid");
}

#[test]
fn non_loopback_endpoint_host_is_rejected_at_parse() {
    // The endpoint host is a typed loopback literal; a hostname or wildcard cannot
    // deserialize.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["endpoint"]["host"] = json!("localhost");
    serde_json::from_value::<Model>(value).expect_err("a non-loopback host must not parse");
}

#[test]
fn lifecycle_must_have_full_generic_class_set() {
    // The lifecycle is a per-class record: a missing class is a missing struct
    // field, rejected at parse rather than by a validation rule.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]
        .as_object_mut()
        .unwrap()
        .remove("clean");
    serde_json::from_value::<Model>(value).expect_err("a missing lifecycle class must not parse");
}

#[test]
fn clean_operation_stays_marker_gated_runtime_cleanup() {
    // clean binds nothing: an execId on it is an unknown field, rejected at parse.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]["clean"]["execId"] = json!("synthetic-helper");
    serde_json::from_value::<Model>(value).expect_err("a clean exec binding must not parse");
}
