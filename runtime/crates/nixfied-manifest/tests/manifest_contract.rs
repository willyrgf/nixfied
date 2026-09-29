use nixfied_manifest::{
    MANIFEST_VERSION, Manifest, TOOLCHAIN_ID, ValidatedManifest, ValidationError, runtime_abi,
};
use serde_json::{Value, json};

fn valid_manifest_json() -> Value {
    json!({
        "manifestVersion": MANIFEST_VERSION,
        "toolchainId": TOOLCHAIN_ID,
        "runtimeAbi": runtime_abi(),
        "generator": {
            "name": "nixfied",
            "version": "1",
            "emitter": "nix/compiler/emit-manifest.nix"
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
            "persistence": "run-scoped"
        },
        "closures": {
            "synthetic-helper": {
                "kind": "executable",
                "storePath": "/nix/store/00000000000000000000000000000000-synthetic-helper",
                "executable": "/nix/store/00000000000000000000000000000000-synthetic-helper/bin/synthetic-helper",
                "targetSystem": "aarch64-darwin",

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
        "stdin": "null"
    })
}

fn synthetic_service() -> Value {
    json!({
        "lifecycle": {
            "start": { "operationId": "service.synthetic.start", "invocation": helper_invocation(json!(["synthetic-helper", "service", "--host", "127.0.0.1", "--port", "${port}"])), "terminal": { "success": "spawned", "failure": "failed" } },
            "ready": { "operationId": "service.synthetic.ready", "policy": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "ready", "failure": "not-ready" } },
            "health": { "operationId": "service.synthetic.health", "policy": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 }, "terminal": { "success": "healthy", "failure": "unhealthy" } },
            "stop": { "operationId": "service.synthetic.stop", "signal": "TERM", "timeoutMs": 5000, "terminal": { "success": "stopped", "failure": "failed" } },
            "clean": { "operationId": "service.synthetic.clean", "terminal": { "success": "cleaned", "failure": "failed" } }
        },
        "endpoints": { "synthetic-tcp": {
            "endpointId": "synthetic-tcp", "host": "127.0.0.1",
            "readyProbe": helper_invocation(json!(["synthetic-helper", "task", "${host}", "${port}"])),
            "healthProbe": helper_invocation(json!(["synthetic-helper", "task", "${host}", "${port}"]))
        } },
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
        "operationId": "task.smoke.run",
        "invocation": helper_invocation(json!(["synthetic-helper", "task", "--host", "127.0.0.1", "--port", "${port}"])),
        "requires": ["synthetic"],

        "exitPolicy": { "successCodes": [0] },
        "artifactRefs": [],
        "logRefs": ["task.smoke"],
        "summaryRefs": ["summary"]
    })
}

fn parse_valid_manifest() -> Manifest {
    serde_json::from_value(valid_manifest_json()).expect("valid manifest JSON should deserialize")
}

/// Adds a second service named `worker` that reuses the helper exec/closure but
/// binds its own lifecycle, endpoint, and probe. Used to prove structural
/// validation accepts arbitrary service counts.
fn add_worker_service(value: &mut Value) {
    let mut worker = synthetic_service();
    let mut endpoint = worker["endpoints"]["synthetic-tcp"].clone();
    endpoint["endpointId"] = json!("worker-tcp");
    worker["endpoints"] = json!({ "worker-tcp": endpoint });
    worker["primaryEndpoint"] = json!("worker-tcp");
    for (class, op) in worker["lifecycle"].as_object_mut().unwrap() {
        op["operationId"] = json!(format!("service.worker.{class}"));
    }
    value["services"]["worker"] = worker;
}

#[test]
fn parses_and_validates_contract() {
    let manifest = parse_valid_manifest();
    ValidatedManifest::try_from(manifest)
        .expect("valid manifest should pass structural validation");
}

#[test]
fn composite_task_default_output_is_rejected() {
    let mut value = valid_manifest_json();
    value["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "defaultOutput": "task-output",
        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });
    let manifest: Manifest = serde_json::from_value(value).expect("composite should deserialize");
    assert!(matches!(
        ValidatedManifest::try_from(manifest),
        Err(ValidationError::UnsupportedValue {
            field: "tasks.defaultOutput",
            ..
        })
    ));
}

#[test]
fn accepts_immutable_source_modes() {
    for mode in ["snapshot", "flake-input"] {
        let mut value = valid_manifest_json();
        value["codebases"][0]["sourceMode"] = json!(mode);
        value["codebases"][0]["sourceIdentity"] =
            json!("/nix/store/00000000000000000000000000000000-source");
        value["codebases"][0]["sourcePolicy"]["dirtyPolicy"] = json!("reject");
        let manifest: Manifest =
            serde_json::from_value(value).expect("manifest JSON should deserialize");

        ValidatedManifest::try_from(manifest)
            .expect("immutable source mode should pass structural validation");
    }
}

#[test]
fn accepts_arbitrary_service_names() {
    // The synthetic/smoke names are not special. A second service validates as
    // long as the structural contract holds.
    let mut value = valid_manifest_json();
    add_worker_service(&mut value);

    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest).expect("multi-service manifests are valid");
}

#[test]
fn rejects_path_hostile_unit_ids() {
    // Ids key filesystem artifacts (log files, registry keys); separators and
    // traversal segments must be refused before any path is built from them.
    for hostile in ["../escape", "a/b", "/abs", ".hidden"] {
        let mut value = valid_manifest_json();
        value["tasks"][hostile] = value["tasks"]["smoke"].clone();
        let manifest: Manifest =
            serde_json::from_value(value).expect("manifest should deserialize");
        ValidatedManifest::try_from(manifest).expect_err("path-hostile ids must be rejected");
    }
}

#[test]
fn accepts_task_only_manifests() {
    // The compiler admits a manifest with no services as long as bounded tasks
    // exist; the structural validator must honor the same contract.
    let mut value = valid_manifest_json();
    value["services"] = json!({});
    value["tasks"]["smoke"]["requires"] = json!([]);

    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest).expect("task-only manifests are valid");
}

#[test]
fn rejects_manifests_with_nothing_to_run() {
    let mut value = valid_manifest_json();
    value["services"] = json!({});
    value["tasks"] = json!({});

    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest)
        .expect_err("a manifest with no services and no tasks has nothing to run");
}

#[test]
fn prepare_may_bind_a_task_reference() {
    // initdb-style preparation is a task reference with full task semantics.
    let mut value = valid_manifest_json();
    value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({ "task": "smoke" });
    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest).expect("prepare may reference a task");
}

#[test]
fn validates_explicit_slot_placement_range() {
    let mut value = valid_manifest_json();
    value["slotPolicy"]["max"] = json!(1);
    let placement = value["placement"]["slotPlacements"]["0"].clone();
    let mut slot_one = placement;
    slot_one["slot"] = json!(1);
    slot_one["candidatePorts"] = json!({ "start": 23180, "end": 23190 });
    value["placement"]["slotPlacements"]["1"] = slot_one;

    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest)
        .expect("explicit slot placements should cover the slot range");
}

#[test]
fn duplicate_environment_is_refused_at_the_wire() {
    // `environments` is a set of isolation namespaces; a duplicate is
    // inexpressible at the wire.
    let mut value = valid_manifest_json();
    value["environments"] = json!(["dev", "dev"]);
    let error = serde_json::from_value::<Manifest>(value)
        .expect_err("a duplicate environment must be refused");
    assert!(error.to_string().contains("duplicate element"));
}

#[test]
fn duplicate_task_success_code_is_refused_at_the_wire() {
    let mut value = valid_manifest_json();
    value["tasks"]["smoke"]["exitPolicy"]["successCodes"] = json!([0, 0]);
    let error = serde_json::from_value::<Manifest>(value)
        .expect_err("a duplicate exit code must be refused");
    assert!(error.to_string().contains("duplicate element"));
}

#[test]
fn abi_mismatch_is_contract_error() {
    let mut manifest = parse_valid_manifest();
    manifest.runtime_abi = "nixfied-runtime-abi:legacy".to_string();

    assert_eq!(
        ValidatedManifest::try_from(manifest).expect_err("ABI mismatch should fail"),
        ValidationError::RuntimeAbi {
            expected: runtime_abi(),
            actual: "nixfied-runtime-abi:legacy".to_string(),
        }
    );
}

#[test]
fn accepts_a_bounded_acyclic_composite() {
    let mut value = valid_manifest_json();
    value["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "steps": {
            "first": { "task": "smoke" },
            "second": { "task": "smoke", "dependsOn": ["first"] }
        }
    });
    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest).expect("a bounded acyclic composite is valid");
}

#[test]
fn non_loopback_endpoint_host_is_rejected_at_parse() {
    // The endpoint host is a typed loopback literal; a hostname or wildcard cannot
    // deserialize.
    let mut value = valid_manifest_json();
    value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["host"] = json!("localhost");
    serde_json::from_value::<Manifest>(value).expect_err("a non-loopback host must not parse");
}

#[test]
fn lifecycle_must_have_full_generic_class_set() {
    // The lifecycle is a per-class record: a missing class is a missing struct
    // field, rejected at parse rather than by a validation rule.
    let mut value = valid_manifest_json();
    value["services"]["synthetic"]["lifecycle"]
        .as_object_mut()
        .unwrap()
        .remove("clean");
    serde_json::from_value::<Manifest>(value)
        .expect_err("a missing lifecycle class must not parse");
}

#[test]
fn removed_tcp_probe_record_rejects_on_the_wire() {
    // Old bytes cannot acquire the new readiness meaning.
    let mut value = valid_manifest_json();
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"] = json!({
        "kind": "tcp", "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20
    });
    serde_json::from_value::<Manifest>(value).expect_err("removed probe record must not parse");
}

#[test]
fn connects_to_chain_is_accepted() {
    let mut value = valid_manifest_json();
    add_worker_service(&mut value);
    value["services"]["worker"]["connectsTo"] = json!(["synthetic"]);
    let manifest: Manifest = serde_json::from_value(value).expect("manifest should deserialize");
    ValidatedManifest::try_from(manifest).expect("acyclic wiring should validate");
}

/// Every closed record refuses unknown fields, including removed ones: no
/// alias, migration, or null-as-absent reading.
#[test]
fn unknown_and_removed_fields_reject_at_the_wire_boundary() {
    for (parent, field, extra) in [
        ("", "computedManifestHash", json!("must-not-be-embedded")),
        (
            "",
            "docs",
            json!({ "title": "legacy", "summary": "legacy" }),
        ),
        ("", "modelVersion", json!(1)),
        ("/state", "stateEpoch", json!("1")),
        ("/state", "stateEpoch", Value::Null),
        ("/state", "cleanupPolicy", json!("protected")),
        ("/closures/synthetic-helper", "operationBindings", json!([])),
        ("/tasks/smoke", "servicesRequired", json!([])),
        ("/tasks/smoke", "serviceLifetime", json!("run-scoped")),
        ("/tasks/smoke/invocation", "cacheEnv", json!({})),
        (
            "/services/synthetic/endpoints/synthetic-tcp/readyProbe",
            "httpPath",
            json!("/health"),
        ),
        // clean binds nothing: runtime cleanup stays marker-gated.
        (
            "/services/synthetic/lifecycle/clean",
            "invocation",
            helper_invocation(json!(["synthetic-helper"])),
        ),
        (
            "/secrets/api-token",
            "value",
            json!("must-not-be-in-manifest"),
        ),
    ] {
        let mut value = valid_manifest_json();
        value["secrets"]["api-token"] = json!({
            "secretId": "api-token",
            "source": { "kind": "env-var", "envVar": "API_TOKEN" }
        });
        value
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{parent} should be a record"))
            .insert(field.into(), extra);
        let error = serde_json::from_value::<Manifest>(value)
            .expect_err("an unknown field must be refused")
            .to_string();
        assert!(
            error.contains(&format!("unknown field `{field}`")),
            "{parent}/{field}: {error}"
        );
    }
}

#[test]
fn endpoint_probe_coverage_is_required_and_non_null_for_every_endpoint() {
    for field in ["readyProbe", "healthProbe"] {
        for missing in [true, false] {
            let mut value = valid_manifest_json();
            let mut second = value["services"]["synthetic"]["endpoints"]["synthetic-tcp"].clone();
            second["endpointId"] = json!("second");
            if missing {
                second.as_object_mut().unwrap().remove(field);
            } else {
                second[field] = Value::Null;
            }
            value["services"]["synthetic"]["endpoints"]["second"] = second;
            assert!(
                serde_json::from_value::<Manifest>(value).is_err(),
                "{field}, missing={missing}"
            );
        }
    }
}

#[test]
fn scalar_probe_and_endpoint_attachments_are_disjoint() {
    for phase in ["ready", "health"] {
        let mut value = valid_manifest_json();
        value["services"]["synthetic"]["lifecycle"][phase]["probe"] =
            helper_invocation(json!(["synthetic-helper"]));
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        assert!(ValidatedManifest::try_from(manifest).is_err());
    }
    let mut endpointless = valid_manifest_json();
    let service = &mut endpointless["services"]["synthetic"];
    service.as_object_mut().unwrap().remove("endpoints");
    service.as_object_mut().unwrap().remove("primaryEndpoint");
    for phase in ["ready", "health"] {
        service["lifecycle"][phase]["probe"] = helper_invocation(json!(["synthetic-helper"]));
    }
    ValidatedManifest::try_from(serde_json::from_value::<Manifest>(endpointless.clone()).unwrap())
        .unwrap();
    for phase in ["ready", "health"] {
        for missing in [true, false] {
            let mut value = endpointless.clone();
            let op = &mut value["services"]["synthetic"]["lifecycle"][phase];
            if missing {
                op.as_object_mut().unwrap().remove("probe");
            } else {
                op["probe"] = Value::Null;
            }
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            assert!(ValidatedManifest::try_from(manifest).is_err());
        }
    }
}

#[test]
fn probe_invocations_reject_inherited_stdin_and_a_second_deadline() {
    for phase in ["readyProbe", "healthProbe"] {
        for (field, extra) in [("stdin", json!("inherit")), ("timeoutMs", json!(1))] {
            let mut value = valid_manifest_json();
            value["services"]["synthetic"]["endpoints"]["synthetic-tcp"][phase][field] = extra;
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            assert!(
                ValidatedManifest::try_from(manifest).is_err(),
                "{phase}.{field}"
            );
        }
    }
}

#[test]
fn phase_policy_is_required_closed_and_positive() {
    for phase in ["ready", "health"] {
        let mut absent = valid_manifest_json();
        absent["services"]["synthetic"]["lifecycle"][phase]
            .as_object_mut()
            .unwrap()
            .remove("policy");
        assert!(serde_json::from_value::<Manifest>(absent).is_err());
        for field in ["timeoutMs", "retryIntervalMs", "maxAttempts"] {
            for invalid in [Value::Null, json!(0), json!(-1), json!(1.5), json!("1")] {
                let mut value = valid_manifest_json();
                value["services"]["synthetic"]["lifecycle"][phase]["policy"][field] = invalid;
                assert!(
                    serde_json::from_value::<Manifest>(value).is_err(),
                    "{phase}.{field}"
                );
            }
            let mut absent = valid_manifest_json();
            absent["services"]["synthetic"]["lifecycle"][phase]["policy"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(serde_json::from_value::<Manifest>(absent).is_err());
        }
        let mut value = valid_manifest_json();
        value["services"]["synthetic"]["lifecycle"][phase]["policy"]["kind"] = json!("tcp");
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }
}
