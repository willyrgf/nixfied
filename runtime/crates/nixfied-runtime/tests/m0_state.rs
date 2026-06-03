use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use nixfied_model::{CleanupPolicy, Model};
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::state::{
    MARKER_FILE_NAME, StateIdentity, StateMarker, clean_marked_state, derive_host_placement,
    inspect_cleanup_target, materialize_run_roots, write_slot_marker,
};
use nixfied_runtime::{Admission, ErrorCode};
use serde_json::{Value, json};

#[test]
fn materializes_m0_roots_and_slot_marker() {
    let fixture = StateFixture::new();

    assert_eq!(
        fixture.layout.state_root,
        fixture.tmp.path.join("runtime-test/dev/0")
    );
    assert_eq!(
        fixture.layout.registry_dir,
        fixture.layout.state_root.join("registry")
    );
    assert_eq!(
        fixture.layout.registry_path(),
        fixture.layout.registry_dir.join("registry.sqlite3")
    );
    assert_eq!(
        fixture.layout.run_dir,
        fixture.layout.state_root.join("runs/run-1")
    );
    assert_eq!(fixture.layout.logs_dir, fixture.layout.run_dir.join("logs"));
    assert_eq!(
        fixture.layout.artifacts_dir,
        fixture.layout.run_dir.join("artifacts")
    );
    assert_eq!(
        fixture.layout.summary_path,
        fixture.layout.run_dir.join("summary.json")
    );
    assert!(fixture.layout.registry_dir.is_dir());
    assert!(fixture.layout.logs_dir.is_dir());
    assert!(fixture.layout.artifacts_dir.is_dir());
    assert!(!fixture.layout.summary_path.exists());

    let marker_path = fixture.layout.state_root.join(MARKER_FILE_NAME);
    let marker: StateMarker =
        serde_json::from_slice(&fs::read(marker_path).expect("marker should be readable"))
            .expect("marker should parse");
    assert!(marker.matches_identity(&fixture.identity));
    assert_eq!(marker.marker_version, 1);
    assert_eq!(marker.project_id, "runtime-test");
    assert_eq!(marker.environment, "dev");
    assert_eq!(marker.slot, 0);
    assert_eq!(marker.state_epoch, "m0");
    assert_eq!(marker.cleanup_policy, CleanupPolicy::DeleteOnClean);
}

#[test]
fn placement_refuses_dot_state_root_template() {
    let tmp = TempDir::new();
    let mut value = fixture_model();
    value["placement"]["stateRootTemplate"] = json!(".");
    let model: Model = serde_json::from_value(value).expect("model should parse");

    let error = derive_host_placement(&model, "run-1", &tmp.path)
        .expect_err("dot placement must be outside M0");

    assert_eq!(error.code, ErrorCode::ModelAdmission);
}

#[cfg(unix)]
#[test]
fn materialization_refuses_symlinked_roots() {
    let tmp = TempDir::new();
    let model = model();
    let layout = derive_host_placement(&model, "run-1", &tmp.path).expect("layout should derive");
    fs::create_dir_all(&layout.state_root).expect("state root should be created");
    let outside = tmp.path.join("outside-registry");
    fs::create_dir_all(&outside).expect("outside dir should be created");
    std::os::unix::fs::symlink(&outside, &layout.registry_dir)
        .expect("registry symlink should be created");

    let error = materialize_run_roots(&layout).expect_err("symlinked roots must be refused");

    assert_eq!(error.code, ErrorCode::StateUnwritable);
}

#[test]
fn marker_write_refuses_existing_marker_mismatch() {
    let fixture = StateFixture::new();
    let mut other = fixture.identity.clone();
    other.project_id = "other-project".to_string();

    let error = write_slot_marker(&fixture.layout, &other)
        .expect_err("marker mismatch must not be overwritten");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    let marker: StateMarker = serde_json::from_slice(
        &fs::read(fixture.layout.state_root.join(MARKER_FILE_NAME))
            .expect("marker should still be readable"),
    )
    .expect("marker should parse");
    assert_eq!(marker.project_id, "runtime-test");
}

#[test]
fn cleanup_refuses_unmarked_roots() {
    let fixture = StateFixture::new();
    let target = fixture.tmp.path.join("runtime-test/dev/unmarked");
    fs::create_dir_all(&target).expect("unmarked root should be created");

    let error = inspect_cleanup_target(&fixture.tmp.path, &target, &fixture.identity)
        .expect_err("unmarked target should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(target.exists());
}

#[test]
fn cleanup_refuses_path_escape() {
    let fixture = StateFixture::new();
    let outside = fixture.tmp.path.join("../outside-owned-state");
    fs::create_dir_all(&outside).expect("outside target should be created");
    let marker = StateMarker::slot(&fixture.identity);
    fs::write(
        outside.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("outside marker should be written");

    let error = inspect_cleanup_target(&fixture.tmp.path, &outside, &fixture.identity)
        .expect_err("path escape should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn cleanup_refuses_marker_mismatch() {
    let fixture = StateFixture::new();
    let mut marker = StateMarker::slot(&fixture.identity);
    marker.project_id = "other-project".to_string();
    fs::write(
        fixture.layout.state_root.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("marker should be replaced");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect_err("marker mismatch should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(fixture.layout.state_root.exists());
}

#[test]
fn cleanup_refuses_protected_state() {
    let fixture = StateFixture::new();
    let mut marker = StateMarker::slot(&fixture.identity);
    marker.cleanup_policy = CleanupPolicy::Protected;
    fs::write(
        fixture.layout.state_root.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("marker should be replaced");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect_err("protected state should be refused");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

#[cfg(unix)]
#[test]
fn cleanup_refuses_symlink_traversal() {
    let fixture = StateFixture::new();
    let outside = fixture.tmp.path.join("outside-file");
    fs::write(&outside, b"outside").expect("outside file should exist");
    std::os::unix::fs::symlink(&outside, fixture.layout.state_root.join("escape-link"))
        .expect("symlink should be created");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect_err("symlink in cleanup tree should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(fixture.layout.state_root.exists());
}

#[test]
fn cleanup_refuses_active_registry_refs() {
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO run_leases (run_id, owner_token, heartbeat_at, expires_at)
         VALUES ('run-1', 'owner', 'now', 'later')",
    );
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO processes (
           process_key, pid, pgid, start_identity, command_json, run_id, status
         ) VALUES ('process-1', 1, 1, 'start', '{}', 'run-1', 'running')",
    );
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO ports (
           endpoint_key, service_instance_id, address, port, status, owner_process_key
         ) VALUES ('endpoint-1', 'service-1', '127.0.0.1', 38080, 'reserved', NULL)",
    );
}

#[test]
fn cleanup_deletes_matching_inactive_state() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();

    let outcome = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut registry,
    )
    .expect("inactive marked state should be deleted");

    assert!(outcome.cleanup_id.starts_with("cleanup-"));
    let cleanup_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM cleanups WHERE cleanup_id = ?1",
            [&outcome.cleanup_id],
            |row| row.get(0),
        )
        .expect("cleanup status should be recorded");
    assert_eq!(cleanup_status, "deleted");
    let mut statement = registry
        .connection()
        .prepare(
            "
            SELECT event_type FROM events
            WHERE payload_json LIKE ?1
            ORDER BY seq
            ",
        )
        .expect("events should be queryable");
    let events = statement
        .query_map([format!("%{}%", outcome.cleanup_id)], |row| {
            row.get::<_, String>(0)
        })
        .expect("events should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("events should collect");
    assert_eq!(events, ["cleanup.intent", "cleanup.deleted"]);
    assert!(!fixture.layout.state_root.exists());
}

fn assert_cleanup_refused_with_active_ref(sql: &str) {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(sql)
        .expect("active ref should be inserted");

    let error = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut registry,
    )
    .expect_err("active registry refs should refuse cleanup");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

struct StateFixture {
    tmp: TempDir,
    layout: nixfied_runtime::state::HostPlacement,
    identity: StateIdentity,
}

impl StateFixture {
    fn new() -> Self {
        let tmp = TempDir::new();
        let model = model();
        let admission = admission(&model);
        let layout =
            derive_host_placement(&model, "run-1", &tmp.path).expect("layout should derive");
        materialize_run_roots(&layout).expect("roots should materialize");
        let identity = StateIdentity::from_model(&model, &admission);
        write_slot_marker(&layout, &identity).expect("marker should be written");
        Self {
            tmp,
            layout,
            identity,
        }
    }

    fn registry(&self) -> Registry {
        Registry::open_or_create(
            self.layout.registry_path(),
            &RegistryIdentity::m0(
                &self.identity.project_id,
                &self.identity.runtime_abi,
                &self.identity.toolchain_id,
            ),
        )
        .expect("registry should open")
    }
}

fn admission(model: &Model) -> Admission {
    Admission {
        model_path: PathBuf::from("/nix/store/test-model/model.json"),
        computed_model_hash: "computed-hash".to_string(),
        raw_len: 100,
        project_id: model.project.project_id.clone(),
        runtime_abi: model.runtime_abi.clone(),
        toolchain_id: model.toolchain_id.clone(),
        target_system: model.target.system.clone(),
    }
}

fn model() -> Model {
    serde_json::from_value(fixture_model()).expect("fixture model should parse")
}

fn fixture_model() -> Value {
    json!({
        "modelVersion": 1,
        "toolchainId": "nixfied-toolchain:m0:1",
        "runtimeAbi": "nixfied-runtime-abi:m0:1",
        "generator": {
            "name": "nixfied",
            "version": "m0",
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
            "closureSystem": host_system(),
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
            "storePath": "/nix/store/test-m0-helper",
            "executable": "/nix/store/test-m0-helper/bin/m0-helper",
            "targetSystem": host_system(),
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
                "executable": "/nix/store/test-m0-helper/bin/m0-helper",
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
            "title": "Runtime Test",
            "summary": "Runtime admission fixture."
        }
    })
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
            "nixfied-state-test-{}-{}",
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
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be available")
        .as_nanos();
    now + u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}
