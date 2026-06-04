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
                "maturity": "m0"
            })
        })
        .collect()
}

fn parse_valid_model() -> Model {
    serde_json::from_value(valid_model_json()).expect("valid model JSON should deserialize")
}

fn add_slot_one(model: &mut Model, start: u16, end: u16) {
    model.slot_policy.max = 1;
    model.runtime_constraints.slot_max = 1;
    model.capabilities.slots = vec![0, 1];
    let mut placement = model
        .placement
        .slot_placements
        .get("0")
        .expect("fixture has slot 0 placement")
        .clone();
    placement.slot = 1;
    placement.candidate_ports.start = start;
    placement.candidate_ports.end = end;
    model
        .placement
        .slot_placements
        .insert("1".to_string(), placement);
}

#[test]
fn parses_and_validates_m0_contract() {
    parse_valid_model()
        .validate_m0()
        .expect("valid M0 model should pass contract validation");
}

#[test]
fn old_single_surface_contract_is_rejected() {
    let mut model = parse_valid_model();
    model.capabilities.surfaces = vec!["model".to_string()];
    model.surfaces.retain(|surface| surface.name == "model");

    let error = model
        .validate_m0()
        .expect_err("M0 runtime surfaces must be explicit");
    match error {
        ValidationError::UnsupportedValue {
            field,
            expected,
            actual,
        } => {
            assert_eq!(field, "capabilities.surfaces");
            assert_eq!(expected, "exact M0 values");
            assert_eq!(actual, "[\"model\"]");
        }
        other => panic!("unexpected validation error: {other:?}"),
    }
}

#[test]
fn capabilities_surfaces_must_match_model_surface_names() {
    let mut model = parse_valid_model();
    model.capabilities.surfaces.push("view-only".to_string());

    let error = model
        .validate_m0()
        .expect_err("capabilities cannot add view-only surfaces");
    match error {
        ValidationError::UnsupportedValue {
            field,
            expected,
            actual,
        } => {
            assert_eq!(field, "capabilities.surfaces");
            assert_eq!(expected, "exact M0 values");
            assert!(actual.contains("view-only"));
        }
        other => panic!("unexpected validation error: {other:?}"),
    }
}

#[test]
fn validates_explicit_slot_placement_range() {
    let mut model = parse_valid_model();
    add_slot_one(&mut model, 38180, 38190);

    model
        .validate_m0()
        .expect("explicit slot placements should cover the slot range");
}

#[test]
fn capabilities_slots_must_match_slot_policy_range() {
    let mut model = parse_valid_model();
    add_slot_one(&mut model, 38180, 38190);
    model.capabilities.slots = vec![0];

    assert_eq!(
        model
            .validate_m0()
            .expect_err("capabilities slots must mirror slot policy"),
        ValidationError::UnsupportedValue {
            field: "capabilities.slots",
            expected: "slotPolicy range",
            actual: "[0]".to_string(),
        }
    );
}

#[test]
fn slot_placements_must_cover_slot_policy_range() {
    let mut model = parse_valid_model();
    model.slot_policy.max = 1;
    model.runtime_constraints.slot_max = 1;
    model.capabilities.slots = vec![0, 1];

    let error = model
        .validate_m0()
        .expect_err("slot placement for slot 1 is required");
    match error {
        ValidationError::UnsupportedValue {
            field,
            expected,
            actual,
        } => {
            assert_eq!(field, "placement.slotPlacements");
            assert_eq!(expected, "exact slotPolicy range");
            assert_eq!(actual, "{\"0\"}");
        }
        other => panic!("unexpected validation error: {other:?}"),
    }
}

#[test]
fn slot_candidate_windows_must_not_overlap() {
    let mut model = parse_valid_model();
    add_slot_one(&mut model, 38085, 38095);

    let error = model
        .validate_m0()
        .expect_err("slot candidate windows must be disjoint");
    match error {
        ValidationError::UnsupportedValue {
            field,
            expected,
            actual,
        } => {
            assert_eq!(field, "placement.slotPlacements.candidatePorts");
            assert_eq!(expected, "non-overlapping windows");
            assert!(actual.contains("38085"));
        }
        other => panic!("unexpected validation error: {other:?}"),
    }
}

#[test]
fn slot_candidate_windows_must_not_use_zero_port() {
    let mut model = parse_valid_model();
    model
        .placement
        .slot_placements
        .get_mut("0")
        .expect("fixture has slot 0 placement")
        .candidate_ports
        .start = 0;

    assert_eq!(
        model
            .validate_m0()
            .expect_err("slot port 0 must be rejected"),
        ValidationError::UnsupportedValue {
            field: "placement.slotPlacements.candidatePorts",
            expected: "ports in 1..65535 with start <= end",
            actual: "start=0, end=38090".to_string(),
        }
    );
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
    model.runtime_abi = "nixfied-runtime-abi:m0:1".to_string();

    assert_eq!(
        model.validate_m0().expect_err("ABI mismatch should fail"),
        ValidationError::RuntimeAbi {
            expected: RUNTIME_ABI,
            actual: "nixfied-runtime-abi:m0:1".to_string(),
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
