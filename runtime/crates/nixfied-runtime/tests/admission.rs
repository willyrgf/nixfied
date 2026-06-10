use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use nixfied_model::{CleanupPolicy, PersistencePolicy};
use nixfied_runtime::{
    Admission, AdmissionContext, ErrorCode, StoreOriginPolicy, load_model, read_raw_model,
};
use serde_json::{Value, json};

fn fixture_model() -> Value {
    json!({
        "modelVersion": 1,
        "toolchainId": "nixfied-toolchain:1",
        "runtimeAbi": "nixfied-runtime-abi:1",
        "generator": {
            "name": "nixfied",
            "version": "1",
            "emitter": "nix/compiler/emit-model.nix"
        },
        "project": {
            "projectId": "runtime-test",
            "name": "Runtime Test"
        },
        "target": {
            "system": host_system(),
            "os": host_os(),
            "arch": host_arch(),
            "closureSystem": host_system()
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
        "closures": {
            "synthetic-helper": {
                "kind": "executable",
                "storePath": "/nix/store/test-synthetic-helper",
                "executable": "/nix/store/test-synthetic-helper/bin/synthetic-helper",
                "targetSystem": host_system(),
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
            "synthetic-helper": {
                "closureId": "synthetic-helper",
                "executable": "/nix/store/test-synthetic-helper/bin/synthetic-helper",
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
                "lifecycle": {
                    "prepare": {
                        "operationId": "service.synthetic.prepare",
                        "execId": null,
                        "execArgs": [],
                        "terminal": { "success": "prepared", "failure": "failed" }
                    },
                    "start": {
                        "operationId": "service.synthetic.start",
                        "execId": "synthetic-helper",
                        "execArgs": ["service", "--host", "127.0.0.1", "--port", "${port}"],
                        "terminal": { "success": "spawned", "failure": "failed" }
                    },
                    "ready": {
                        "operationId": "service.synthetic.ready",
                        "probe": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 },
                        "terminal": { "success": "ready", "failure": "not-ready" }
                    },
                    "health": {
                        "operationId": "service.synthetic.health",
                        "probe": { "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 20 },
                        "terminal": { "success": "healthy", "failure": "unhealthy" }
                    },
                    "stop": {
                        "operationId": "service.synthetic.stop",
                        "signal": "TERM",
                        "timeoutMs": 5000,
                        "terminal": { "success": "stopped", "failure": "failed" }
                    },
                    "clean": {
                        "operationId": "service.synthetic.clean",
                        "terminal": { "success": "cleaned", "failure": "failed" }
                    }
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
            }
        },
        "tasks": {
            "smoke": {
                "operationId": "task.smoke.run",
                "execId": "synthetic-helper",
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
            "title": "Runtime Test",
            "summary": "Runtime admission fixture."
        }
    })
}



#[test]
fn load_model_hashes_raw_bytes() {
    let (_tmp, model_path, _closure) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let raw = fs::read(&model_path).expect("fixture should be readable");
    let expected = sha256_hex(&raw);

    assert_eq!(loaded.raw_len, raw.len());
    assert_eq!(loaded.computed_model_hash, expected);
}

#[test]
fn normal_admission_refuses_non_store_model() {
    let (_tmp, model_path, _closure) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let error = Admission::check(
        &loaded,
        &AdmissionContext::current(StoreOriginPolicy::RequireStore),
    )
    .expect_err("non-store model must be refused");

    assert_eq!(error.code, ErrorCode::ModelNotStoreOutput);
    assert_eq!(
        error.computed_model_hash.as_deref(),
        Some(loaded.computed_model_hash.as_str())
    );
}

#[test]
fn normal_origin_refuses_non_store_before_parse() {
    let tmp = TempDir::new();
    let outside = tmp.path.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let model_path = outside.join("model.json");
    fs::write(&model_path, b"{not json").unwrap();
    let raw = read_raw_model(&model_path).expect("raw bytes should be readable");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::RequireStore,
        store_root: tmp.path.join("store"),
        host_system: host_system(),
    };
    fs::create_dir_all(&context.store_root).unwrap();
    let error = nixfied_runtime::admission::origin::check_raw_store_origin(&raw, &context)
        .expect_err("origin should gate before JSON parse");

    assert_eq!(error.code, ErrorCode::ModelNotStoreOutput);
}

#[test]
fn origin_rejects_canonical_path_escape() {
    let tmp = TempDir::new();
    let store = tmp.path.join("store");
    let outside = tmp.path.join("outside");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let real_model = outside.join("model.json");
    fs::write(&real_model, serde_json::to_vec(&fixture_model()).unwrap()).unwrap();
    let escaped_model = store.join("../outside/model.json");
    let raw = read_raw_model(&escaped_model).expect("escaped path should read");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::RequireStore,
        store_root: store,
        host_system: host_system(),
    };
    let error = nixfied_runtime::admission::origin::check_raw_store_origin(&raw, &context)
        .expect_err("canonical path outside store must fail");

    assert_eq!(error.code, ErrorCode::ModelNotStoreOutput);
}

#[test]
fn unstable_escape_hatch_admits_non_store_model() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root
            .parent()
            .expect("fixture closure has a parent")
            .to_path_buf(),
        host_system: host_system(),
    };
    let admission = Admission::check(&loaded, &context).expect("escape hatch should admit fixture");

    assert_eq!(admission.computed_model_hash, loaded.computed_model_hash);
    assert_eq!(admission.project_id, "runtime-test");
}

#[test]
fn source_admission_records_invocation_root() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let admission = Admission::check(&loaded, &context).expect("source should admit");
    let invocation_root = std::env::current_dir()
        .expect("current dir should exist")
        .canonicalize()
        .expect("current dir should canonicalize");

    assert_eq!(admission.source.codebase_id, "main");
    assert_eq!(admission.source.logical_root, ".");
    assert_eq!(admission.source.observed_root, invocation_root);
}

#[test]
fn dirty_policy_reject_fails_closed() {
    let mut model = fixture_model();
    model["codebases"][0]["sourcePolicy"]["dirtyPolicy"] = json!("reject");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error =
        Admission::check(&loaded, &context).expect_err("dirtyPolicy=reject cannot be proven in M0");

    assert_eq!(error.code, ErrorCode::SourceMismatch);
    assert_eq!(
        error.computed_model_hash.as_deref(),
        Some(loaded.computed_model_hash.as_str())
    );
}

#[test]
fn logical_root_escape_is_rejected() {
    let mut model = fixture_model();
    model["codebases"][0]["logicalRoot"] = json!("..");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context)
        .expect_err("logicalRoot must not escape invocation root");

    assert_eq!(error.code, ErrorCode::SourceMismatch);
}

#[test]
fn protected_persistent_state_is_valid_model_data() {
    let mut model = fixture_model();
    model["state"]["cleanupPolicy"] = json!("protected");
    model["state"]["persistence"] = json!("persistent");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("protected persistent state should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    Admission::check(&loaded, &context).expect("protected persistent state should admit");

    assert_eq!(loaded.model.state.cleanup_policy, CleanupPolicy::Protected);
    assert_eq!(
        loaded.model.state.persistence,
        PersistencePolicy::Persistent
    );
}

#[test]
fn abi_mismatch_is_runtime_abi_error() {
    let mut model = fixture_model();
    model["runtimeAbi"] = json!("nixfied-runtime-abi:legacy");
    let (_tmp, model_path, _closure) = write_fixture_model(model, true);
    let error = load_model(&model_path).expect_err("ABI mismatch should fail during load");

    assert_eq!(error.code, ErrorCode::RuntimeAbiMismatch);
    assert!(error.computed_model_hash.is_some());
}

#[test]
fn missing_closure_is_rejected() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), false);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root
            .parent()
            .expect("fixture closure has a parent")
            .to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context).expect_err("missing closure should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
}

#[test]
fn closure_store_path_escape_is_rejected() {
    let tmp = TempDir::new();
    let store = tmp.path.join("store");
    let outside_closure = tmp.path.join("outside-closure/test-synthetic-helper");
    let executable = outside_closure.join("bin/synthetic-helper");
    fs::create_dir_all(&store).unwrap();
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&executable).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&executable, perms).unwrap();

    let mut model = fixture_model();
    model["closures"]["synthetic-helper"]["storePath"] = json!(
        store
            .join("../outside-closure/test-synthetic-helper")
            .to_string_lossy()
    );
    model["closures"]["synthetic-helper"]["executable"] = json!(
        store
            .join("../outside-closure/test-synthetic-helper/bin/synthetic-helper")
            .to_string_lossy()
    );
    model["execs"]["synthetic-helper"]["executable"] = model["closures"]["synthetic-helper"]["executable"].clone();
    let model_path = tmp.path.join("model.json");
    fs::write(&model_path, serde_json::to_vec(&model).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: store,
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context).expect_err("escaped closure path should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
}

#[test]
fn exec_executable_must_match_declared_closure() {
    let (_tmp, model_path, closure_root) = write_fixture_model(fixture_model(), true);
    let mut value: Value =
        serde_json::from_slice(&fs::read(&model_path).unwrap()).expect("fixture JSON");
    value["execs"]["synthetic-helper"]["executable"] =
        json!(closure_root.join("bin/other-helper").to_string_lossy());
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error =
        Admission::check(&loaded, &context).expect_err("exec executable mismatch should fail");

    assert_eq!(error.code, ErrorCode::ClosureMissing);
}

#[test]
fn target_os_and_arch_must_match_host() {
    let mut model = fixture_model();
    model["target"]["os"] = json!("definitely-not-this-os");
    let (_tmp, model_path, closure_root) = write_fixture_model(model, true);
    let loaded = load_model(&model_path).expect("fixture should load");
    let context = AdmissionContext {
        policy: StoreOriginPolicy::AllowNonStoreForTests,
        store_root: closure_root.parent().unwrap().to_path_buf(),
        host_system: host_system(),
    };
    let error = Admission::check(&loaded, &context).expect_err("target OS mismatch should fail");

    assert_eq!(error.code, ErrorCode::PlatformUnsupported);
}

fn write_fixture_model(mut value: Value, create_executable: bool) -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new();
    let closure_root = tmp.path.join("store/test-synthetic-helper");
    let executable = closure_root.join("bin/synthetic-helper");
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["executable"] = json!(executable.to_string_lossy());
    value["execs"]["synthetic-helper"]["executable"] = json!(executable.to_string_lossy());
    let model_path = tmp.path.join("model.json");
    if create_executable {
        fs::create_dir_all(executable.parent().expect("executable parent")).unwrap();
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = fs::metadata(&executable).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&executable, perms).unwrap();
    }
    fs::write(&model_path, serde_json::to_vec(&value).unwrap()).unwrap();
    (tmp, model_path, closure_root)
}

fn sha256_hex(raw: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    hex::encode(Sha256::digest(raw))
}

fn host_system() -> String {
    format!("{}-{}", host_arch(), host_os())
}

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn host_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "nixfied-runtime-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn unique_suffix() -> u128 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be monotonic enough for tests")
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    (now << 16) | count
}
