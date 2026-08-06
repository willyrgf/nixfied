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
        "secrets": {},
        "environments": ["dev"],
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
                        "start": 23080,
                        "end": 23090
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
                    "task.smoke.run"
                ],
                "requiresExecutable": true,
                "effects": ["process", "network-listener"]
            }
        },
        "services": {
            "synthetic": synthetic_service()
        },
        "tasks": {
            "smoke": smoke_task()
        }
    })
}

fn helper_invocation(run: Value) -> Value {
    json!({
        "tools": ["synthetic-helper"],
        "run": run,
        "executable": "/nix/store/00000000000000000000000000000000-synthetic-helper/bin/synthetic-helper",
        "env": {},
        "codebaseId": "main",
        "cwd": ".",
        "stdin": "null",
        "timeoutMs": 30000
    })
}

fn synthetic_service() -> Value {
    json!({
        "lifecycle": {
            "start": { "operationId": "service.synthetic.start", "invocation": helper_invocation(json!(["synthetic-helper", "service", "--host", "127.0.0.1", "--port", "${port}"])), "terminal": { "success": "spawned", "failure": "failed" } },
            "ready": { "operationId": "service.synthetic.ready", "probe": { "kind": "tcp", "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "ready", "failure": "not-ready" } },
            "health": { "operationId": "service.synthetic.health", "probe": { "kind": "tcp", "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "healthy", "failure": "unhealthy" } },
            "stop": { "operationId": "service.synthetic.stop", "signal": "TERM", "timeoutMs": 5000, "terminal": { "success": "stopped", "failure": "failed" } },
            "clean": { "operationId": "service.synthetic.clean", "terminal": { "success": "cleaned", "failure": "failed" } }
        },
        "endpoints": { "synthetic-tcp": { "endpointId": "synthetic-tcp", "host": "127.0.0.1" } },
        "primaryEndpoint": "synthetic-tcp",
                "connectsTo": [],
        "stateRefs": ["slot"],
        "logRefs": ["service.synthetic"],
        "containment": "process-group"
    })
}

fn smoke_task() -> Value {
    json!({
        "kind": "leaf",
        "defaultOutput": "summary",
        "serviceLifetime": "run-scoped",
        "operationId": "task.smoke.run",
        "invocation": helper_invocation(json!(["synthetic-helper", "task", "--host", "127.0.0.1", "--port", "${port}"])),
        "requires": ["synthetic"],
        "servicesRequired": ["synthetic"],
        "exitPolicy": { "successCodes": [0] },
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
    value["closures"]["synthetic-helper"]["operationBindings"] = json!([
        "service.synthetic.start",
        "service.worker.start",
        "task.smoke.run"
    ]);

    let mut worker = synthetic_service();
    worker["endpoints"] =
        json!({ "worker-tcp": { "endpointId": "worker-tcp", "host": "127.0.0.1" } });
    worker["primaryEndpoint"] = json!("worker-tcp");
    for (class, op) in worker["lifecycle"].as_object_mut().unwrap() {
        op["operationId"] = json!(format!("service.worker.{class}"));
    }
    value["services"]["worker"] = worker;
}

#[test]
fn parses_and_validates_contract() {
    let model = parse_valid_model();
    model
        .validate()
        .expect("valid model should pass structural validation");

    // The lifecycle is a per-class record: every class is present by construction.
    let lifecycle = &model.services["synthetic"].lifecycle;
    assert_eq!(
        lifecycle.start.operation_id.as_str(),
        "service.synthetic.start"
    );
    assert_eq!(lifecycle.stop.signal, nixfied_model::StopSignal::Term);
}

#[test]
fn task_default_output_round_trips_for_a_leaf() {
    let mut value = valid_model_json();
    value["tasks"]["smoke"]["defaultOutput"] = json!("task-output");
    let model: Model = serde_json::from_value(value).expect("task default should parse");
    model.validate().expect("leaf task default should validate");
    assert_eq!(
        model.tasks["smoke"].default_output,
        nixfied_model::TaskDefaultOutput::TaskOutput
    );
    let emitted = serde_json::to_value(model).expect("task default should serialize");
    assert_eq!(
        emitted["tasks"]["smoke"]["defaultOutput"],
        json!("task-output")
    );
}

#[test]
fn composite_task_default_output_is_rejected() {
    let mut value = valid_model_json();
    value["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "defaultOutput": "task-output",
        "serviceLifetime": "run-scoped",
        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });
    let model: Model = serde_json::from_value(value).expect("composite should deserialize");
    assert!(matches!(
        model.validate(),
        Err(ValidationError::UnsupportedValue {
            field: "tasks.defaultOutput",
            ..
        })
    ));
}

#[test]
fn secret_descriptors_round_trip_without_values() {
    let mut value = valid_model_json();
    value["secrets"] = json!({
        "api-token": {
            "secretId": "api-token",
            "source": {
                "kind": "env-var",
                "envVar": "API_TOKEN"
            }
        },
        "tls-key": {
            "secretId": "tls-key",
            "source": {
                "kind": "file",
                "path": "tls/key"
            }
        }
    });
    value["tasks"]["smoke"]["serviceLifetime"] = json!("until-idle");
    value["tasks"]["smoke"]["invocation"]["env"]["API_TOKEN"] = json!("${secret:api-token}");

    let model: Model = serde_json::from_value(value).expect("secret descriptors should parse");
    let api_token = &model.secrets["api-token"];
    assert_eq!(api_token.secret_id.as_str(), "api-token");
    assert_eq!(
        api_token.source.kind,
        nixfied_model::SecretSourceKind::EnvVar
    );
    assert_eq!(api_token.source.env_var.as_deref(), Some("API_TOKEN"));
    assert_eq!(api_token.source.path, None);
    assert_eq!(
        model.tasks["smoke"].service_lifetime,
        nixfied_model::ServiceLifetime::UntilIdle
    );

    let emitted = serde_json::to_value(&model).expect("model should serialize");
    assert_eq!(
        emitted["secrets"]["tls-key"]["source"]["kind"],
        json!("file")
    );
    assert_eq!(
        emitted["tasks"]["smoke"]["invocation"]["env"]["API_TOKEN"],
        json!("${secret:api-token}")
    );
}

#[test]
fn cache_env_is_an_unknown_invocation_field() {
    let mut value = valid_model_json();
    value["tasks"]["smoke"]["invocation"]["cacheEnv"] = json!({
        "CARGO_TARGET_DIR": {
            "family": "cargo-target",
            "mode": "fast-dev",
            "scope": "slot",
            "key": { "parts": ["cache-v1"] }
        }
    });

    let error = serde_json::from_value::<Model>(value)
        .expect_err("removed cacheEnv field must fail deserialization");
    assert!(error.to_string().contains("unknown field `cacheEnv`"));
}

#[test]
fn service_lifetime_variants_round_trip() {
    for (wire, expected) in [
        ("run-scoped", nixfied_model::ServiceLifetime::RunScoped),
        ("until-idle", nixfied_model::ServiceLifetime::UntilIdle),
        (
            "persistent-until-down",
            nixfied_model::ServiceLifetime::PersistentUntilDown,
        ),
    ] {
        let mut value = valid_model_json();
        value["tasks"]["smoke"]["serviceLifetime"] = json!(wire);
        let model: Model = serde_json::from_value(value).expect("service lifetime should parse");
        assert_eq!(model.tasks["smoke"].service_lifetime, expected);
    }
}

#[test]
fn accepts_immutable_source_modes() {
    for mode in ["snapshot", "flake-input"] {
        let mut value = valid_model_json();
        value["codebases"][0]["sourceMode"] = json!(mode);
        value["codebases"][0]["sourceIdentity"] =
            json!("/nix/store/00000000000000000000000000000000-source");
        value["codebases"][0]["sourcePolicy"]["dirtyPolicy"] = json!("reject");
        let model: Model = serde_json::from_value(value).expect("model JSON should deserialize");

        model
            .validate()
            .expect("immutable source mode should pass structural validation");
    }
}

#[test]
fn accepts_arbitrary_service_names() {
    // The synthetic/smoke names are not special. A second service validates as
    // long as the structural contract holds.
    let mut value = valid_model_json();
    add_worker_service(&mut value);

    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model.validate().expect("multi-service models are valid");
}

#[test]
fn rejects_path_hostile_unit_ids() {
    // Ids key filesystem artifacts (log files, registry keys); separators and
    // traversal segments must be refused before any path is built from them.
    for hostile in ["../escape", "a/b", "/abs", ".hidden"] {
        let mut value = valid_model_json();
        value["tasks"][hostile] = value["tasks"]["smoke"].clone();
        let model: Model = serde_json::from_value(value).expect("model should deserialize");
        model
            .validate()
            .expect_err("path-hostile ids must be rejected");
    }
}

#[test]
fn accepts_task_only_models() {
    // The compiler admits a model with no services as long as bounded tasks
    // exist; the structural validator must honor the same contract.
    let mut value = valid_model_json();
    value["services"] = json!({});
    value["tasks"]["smoke"]["requires"] = json!([]);
    value["tasks"]["smoke"]["servicesRequired"] = json!([]);

    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model.validate().expect("task-only models are valid");
}

#[test]
fn rejects_models_with_nothing_to_run() {
    let mut value = valid_model_json();
    value["services"] = json!({});
    value["tasks"] = json!({});

    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect_err("a model with no services and no tasks has nothing to run");
}

#[test]
fn prepare_may_bind_a_task_reference() {
    // initdb-style preparation is a task reference with full task semantics.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({ "task": "smoke" });
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model.validate().expect("prepare may reference a task");
}

#[test]
fn validates_explicit_slot_placement_range() {
    let mut value = valid_model_json();
    value["slotPolicy"]["max"] = json!(1);
    let placement = value["placement"]["slotPlacements"]["0"].clone();
    let mut slot_one = placement;
    slot_one["slot"] = json!(1);
    slot_one["candidatePorts"] = json!({ "start": 23180, "end": 23190 });
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
fn removed_docs_field_is_invalid() {
    let mut value = valid_model_json();
    value["docs"] = json!({ "title": "legacy", "summary": "legacy" });

    let error = serde_json::from_value::<Model>(value)
        .expect_err("the removed docs model section must be refused");
    assert!(error.to_string().contains("docs"));
}

#[test]
fn duplicate_environment_is_refused_at_the_wire() {
    // `environments` is a set of isolation namespaces; a duplicate is
    // inexpressible at the wire.
    let mut value = valid_model_json();
    value["environments"] = json!(["dev", "dev"]);
    let error = serde_json::from_value::<Model>(value)
        .expect_err("a duplicate environment must be refused");
    assert!(error.to_string().contains("duplicate element"));
}

#[test]
fn duplicate_task_success_code_is_refused_at_the_wire() {
    let mut value = valid_model_json();
    value["tasks"]["smoke"]["exitPolicy"]["successCodes"] = json!([0, 0]);
    let error =
        serde_json::from_value::<Model>(value).expect_err("a duplicate exit code must be refused");
    assert!(error.to_string().contains("duplicate element"));
}

#[test]
fn secret_descriptor_rejects_unknown_fields() {
    let mut value = valid_model_json();
    value["secrets"]["api-token"] = json!({
        "secretId": "api-token",
        "source": {
            "kind": "env-var",
            "envVar": "API_TOKEN"
        },
        "value": "must-not-be-in-model"
    });
    let error = serde_json::from_value::<Model>(value).expect_err("secret values must not parse");
    assert!(error.to_string().contains("value"));
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

#[test]
fn accepts_a_bounded_acyclic_composite() {
    let mut value = valid_model_json();
    value["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "serviceLifetime": "run-scoped",
        "steps": {
            "first": { "task": "smoke" },
            "second": { "task": "smoke", "dependsOn": ["first"] }
        }
    });
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model
        .validate()
        .expect("a bounded acyclic composite is valid");
}

#[test]
fn non_loopback_endpoint_host_is_rejected_at_parse() {
    // The endpoint host is a typed loopback literal; a hostname or wildcard cannot
    // deserialize.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["host"] = json!("localhost");
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
fn probe_kind_is_required_on_the_wire() {
    // The emitter always writes the discriminator; a probe without it is an
    // out-of-contract document, rejected at parse.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"] = json!({
        "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20
    });
    serde_json::from_value::<Model>(value).expect_err("a kind-less probe must not parse");
}

#[test]
fn probe_rejects_unknown_fields() {
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"]["httpPath"] = json!("/health");
    serde_json::from_value::<Model>(value).expect_err("an unknown probe field must not parse");
}

#[test]
fn exec_probe_round_trips() {
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"] = json!({
        "kind": "exec",
        "invocation": helper_invocation(json!(["synthetic-helper", "ping", "-p", "${port}"])),
        "timeoutMs": 2000, "retryIntervalMs": 200, "maxAttempts": 30
    });
    let model: Model = serde_json::from_value(value).expect("an exec probe should parse");
    let probe = &model.services["synthetic"].lifecycle.ready.probe;
    assert_eq!(probe.kind, nixfied_model::ProbeKind::Exec);
    let emitted = serde_json::to_value(&model).expect("model should serialize");
    assert_eq!(
        emitted["services"]["synthetic"]["lifecycle"]["ready"]["probe"]["invocation"]["run"][0],
        json!("synthetic-helper")
    );
    // A tcp probe round-trips without invocation noise (serde skip rules the
    // Nix emitter mirrors).
    let health = &emitted["services"]["synthetic"]["lifecycle"]["health"]["probe"];
    assert_eq!(health["kind"], json!("tcp"));
    assert!(health.get("invocation").is_none());
}

#[test]
fn clean_operation_stays_marker_gated_runtime_cleanup() {
    // clean binds nothing: an invocation on it is an unknown field, rejected at
    // parse.
    let mut value = valid_model_json();
    value["services"]["synthetic"]["lifecycle"]["clean"]["invocation"] =
        helper_invocation(json!(["synthetic-helper"]));
    serde_json::from_value::<Model>(value).expect_err("a clean invocation binding must not parse");
}

#[test]
fn connects_to_undeclared_service_is_rejected() {
    let mut value = valid_model_json();
    value["services"]["synthetic"]["connectsTo"] = json!(["missing"]);
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    let error = model.validate().expect_err("undeclared target must fail");
    assert!(error.to_string().contains("connectsTo"));
}

#[test]
fn connects_to_cycle_is_rejected() {
    let mut value = valid_model_json();
    add_worker_service(&mut value);
    value["services"]["synthetic"]["connectsTo"] = json!(["worker"]);
    value["services"]["worker"]["connectsTo"] = json!(["synthetic"]);
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    let error = model.validate().expect_err("cycle must fail");
    assert!(error.to_string().contains("acyclic"));
}

#[test]
fn connects_to_chain_is_accepted() {
    let mut value = valid_model_json();
    add_worker_service(&mut value);
    value["services"]["worker"]["connectsTo"] = json!(["synthetic"]);
    let model: Model = serde_json::from_value(value).expect("model should deserialize");
    model.validate().expect("acyclic wiring should validate");
}
