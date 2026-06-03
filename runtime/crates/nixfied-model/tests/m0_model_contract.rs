use nixfied_model::{MODEL_VERSION, Model, RUNTIME_ABI, TOOLCHAIN_ID, ValidateM0, ValidationError};
use serde_json::{Value, json};

fn valid_model_json() -> Value {
    json!({
        "modelVersion": MODEL_VERSION,
        "toolchainId": TOOLCHAIN_ID,
        "runtimeAbi": RUNTIME_ABI,
        "generator": {
            "name": "nixfied",
            "version": "m0",
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
                "admissionFingerprintPolicy": "m0-placeholder"
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
            "surfaces": ["model"]
        },
        "runtimeConstraints": {
            "allowedEnvironments": ["dev"],
            "slotMin": 0,
            "slotDefault": 0,
            "slotMax": 0,
            "allowPortOverride": false,
            "collisionPolicy": "fail"
        },
        "surfaces": [{
            "name": "model",
            "aliases": [],
            "inputSchema": {},
            "outputSchema": {},
            "exitClasses": ["ok", "error"],
            "evaluationPermission": "never",
            "maturity": "m0"
        }],
        "placement": {
            "stateRootTemplate": "${projectId}/${environment}/${slot}",
            "registryDir": "registry",
            "runDirTemplate": "runs/${runId}",
            "logsDirTemplate": "runs/${runId}/logs",
            "artifactsDirTemplate": "runs/${runId}/artifacts",
            "candidatePorts": {
                "start": 38080,
                "end": 38090
            }
        },
        "state": {
            "markerIdentity": "nixfied-m0",
            "stateEpoch": "m0",
            "cleanupPolicy": "delete-on-clean",
            "persistence": "run-scoped"
        },
        "secrets": [],
        "closures": [{
            "closureId": "m0-helper",
            "kind": "executable",
            "storePath": "/nix/store/00000000000000000000000000000000-m0-helper",
            "executable": "/nix/store/00000000000000000000000000000000-m0-helper/bin/m0-helper",
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
            "m0-helper": {
                "execId": "m0-helper",
                "closureId": "m0-helper",
                "executable": "/nix/store/00000000000000000000000000000000-m0-helper/bin/m0-helper",
                "args": [],
                "env": {},
                "codebaseId": "main",
                "cwd": ".",
                "stdin": "null",
                "timeoutMs": 30000,
                "outputCapture": "stdout-stderr",
                "cancellationMode": "kill-process-group"
            }
        },
        "services": {
            "synthetic": {
                "serviceId": "synthetic",
                "foreground": true,
                "lifecycle": [
                    {
                        "operationId": "service.synthetic.start",
                        "class": "start",
                        "execId": "m0-helper",
                        "execArgs": ["service", "--host", "127.0.0.1", "--port", "${port}"],
                        "probeId": null,
                        "terminal": {
                            "success": "spawned",
                            "failure": "failed"
                        }
                    },
                    {
                        "operationId": "service.synthetic.ready",
                        "class": "ready",
                        "execId": null,
                        "execArgs": [],
                        "probeId": "synthetic-tcp",
                        "terminal": {
                            "success": "ready",
                            "failure": "not-ready"
                        }
                    },
                    {
                        "operationId": "service.synthetic.stop",
                        "class": "stop",
                        "execId": "m0-helper",
                        "execArgs": ["stop"],
                        "probeId": null,
                        "terminal": {
                            "success": "stopped",
                            "failure": "failed"
                        }
                    }
                ],
                "endpoints": [{
                    "endpointId": "synthetic-tcp",
                    "protocol": "tcp",
                    "host": "127.0.0.1",
                    "port": {
                        "kind": "candidate-window",
                        "start": 38080,
                        "end": 38090
                    },
                    "ownershipVerification": "required",
                    "socketActivation": "disabled"
                }],
                "probes": [{
                    "probeId": "synthetic-tcp",
                    "target": {
                        "kind": "tcp-connect",
                        "endpointId": "synthetic-tcp"
                    },
                    "timeoutMs": 1000,
                    "retryIntervalMs": 100,
                    "maxAttempts": 20
                }],
                "readinessProbe": "synthetic-tcp",
                "stopPolicy": {
                    "signal": "TERM",
                    "timeoutMs": 5000
                },
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
            }
        },
        "tasks": {
            "smoke": {
                "taskId": "smoke",
                "operationId": "task.smoke.run",
                "execId": "m0-helper",
                "args": ["task", "--host", "127.0.0.1", "--port", "${port}"],
                "dependsOnServicesReady": ["synthetic"],
                "exitPolicy": {
                    "successCodes": [0]
                },
                "outputCapture": "stdout-stderr",
                "artifactRefs": [],
                "logRefs": ["task.smoke"],
                "summaryRefs": ["summary"]
            }
        },
        "workflows": {},
        "docs": {
            "title": "M0 Example",
            "summary": "Minimal M0 model contract fixture."
        }
    })
}

fn parse_valid_model() -> Model {
    serde_json::from_value(valid_model_json()).expect("valid model JSON should deserialize")
}

#[test]
fn parses_and_validates_m0_contract() {
    parse_valid_model()
        .validate_m0()
        .expect("valid M0 model should pass contract validation");
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
    model.runtime_abi = "nixfied-runtime-abi:m0:2".to_string();

    assert_eq!(
        model.validate_m0().expect_err("ABI mismatch should fail"),
        ValidationError::RuntimeAbi {
            expected: RUNTIME_ABI,
            actual: "nixfied-runtime-abi:m0:2".to_string(),
        }
    );
}

#[test]
fn non_empty_secrets_are_rejected_in_m0() {
    let mut model = parse_valid_model();
    model.secrets.push(nixfied_model::SecretRef {
        secret_id: "db".to_string(),
        target: "env:DB_PASSWORD".to_string(),
        required: true,
    });

    assert_eq!(
        model
            .validate_m0()
            .expect_err("secrets are unsupported in M0"),
        ValidationError::MustBeEmpty { field: "secrets" }
    );
}

#[test]
fn host_absolute_placement_is_rejected() {
    let mut model = parse_valid_model();
    model.placement.state_root_template = "/tmp/nixfied".to_string();

    assert_eq!(
        model
            .validate_m0()
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
            .validate_m0()
            .expect_err("undeclared operation binding should fail"),
        ValidationError::UnsupportedValue {
            field: "closures[0].operationBindings",
            expected: "exact M0 values",
            actual: "[\"service.synthetic.start\", \"service.synthetic.stop\", \"task.smoke.run\", \"workflow.deferred.run\"]".to_string(),
        }
    );
}

#[test]
fn extra_execs_are_rejected_in_m0() {
    let mut model = parse_valid_model();
    let exec = model
        .execs
        .get("m0-helper")
        .expect("fixture has helper exec")
        .clone();
    model.execs.insert("second-helper".to_string(), exec);

    assert_eq!(
        model.validate_m0().expect_err("M0 has exactly one exec"),
        ValidationError::ExpectedLen {
            field: "execs",
            expected: 1,
            actual: 2,
        }
    );
}

#[test]
fn runtime_constraints_must_remain_m0() {
    let mut model = parse_valid_model();
    model.runtime_constraints.collision_policy = nixfied_model::CollisionPolicy::ProbeInRange;

    assert_eq!(
        model
            .validate_m0()
            .expect_err("M0 supports fail collision policy only"),
        ValidationError::UnsupportedValue {
            field: "runtimeConstraints.collisionPolicy",
            expected: "fail",
            actual: "ProbeInRange".to_string(),
        }
    );
}

#[test]
fn lifecycle_must_have_ready_probe_binding() {
    let mut model = parse_valid_model();
    model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .lifecycle[1]
        .probe_id = None;

    assert_eq!(
        model
            .validate_m0()
            .expect_err("ready lifecycle must bind the readiness probe"),
        ValidationError::UnsupportedValue {
            field: "lifecycle.probeId",
            expected: "synthetic-tcp",
            actual: "null".to_string(),
        }
    );
}
