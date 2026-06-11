//! Shared fixtures and helpers for the integration-test binaries. Each `tests/*.rs`
//! pulls this in with `mod common;`; no single binary uses every item, so dead
//! code is expected here rather than a sign of rot.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

/// The synthetic-service executable the default fixture binds.
pub const SYNTHETIC_EXECUTABLE: &str = "/nix/store/test-synthetic-helper/bin/synthetic-helper";
/// The synthetic service's start arguments.
pub const SYNTHETIC_START_ARGS: &[&str] = &["service", "--host", "127.0.0.1", "--port", "${port}"];

/// The canonical admission fixture — a `synthetic` foreground service plus a
/// `smoke` task in slot 0 over the given candidate port window. This is the single
/// source of truth for the current model shape across the integration tests; keep
/// it in step with the `Model` serde contract.
pub fn synthetic_model(
    executable: &str,
    start_args: &[&str],
    port_start: u16,
    port_end: u16,
) -> Value {
    json!({
        "modelVersion": 1,
        "toolchainId": "nixfied-toolchain:1",
        "runtimeAbi": nixfied_model::runtime_abi(),
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
            "slotPlacements": {
                "0": {
                    "slot": 0,
                    "candidatePorts": {
                        "start": port_start,
                        "end": port_end
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
                "executable": executable,
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
                "executable": executable,
                "args": [],
                "env": {},
                "codebaseId": "main",
                "cwd": ".",
                "stdin": "null",
                "timeoutMs": 30000
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
                        "execArgs": start_args,
                        "terminal": { "success": "spawned", "failure": "failed" }
                    },
                    "ready": {
                        "operationId": "service.synthetic.ready",
                        "probe": { "timeoutMs": 250, "retryIntervalMs": 25, "maxAttempts": 40 },
                        "terminal": { "success": "ready", "failure": "not-ready" }
                    },
                    "health": {
                        "operationId": "service.synthetic.health",
                        "probe": { "timeoutMs": 250, "retryIntervalMs": 25, "maxAttempts": 40 },
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
                "connectsTo": [],
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

/// [`synthetic_model`] with the default executable and start arguments.
pub fn synthetic_model_default(port_start: u16, port_end: u16) -> Value {
    synthetic_model(
        SYNTHETIC_EXECUTABLE,
        SYNTHETIC_START_ARGS,
        port_start,
        port_end,
    )
}

pub fn host_system() -> String {
    format!("{}-{}", host_arch(), host_os())
}

pub fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    }
}

pub fn host_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
}

/// A unique temporary directory removed on drop.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "nixfied-test-{}-{}",
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

/// The trailing JSON document a runtime command writes to stderr, after any
/// human-readable progress lines (which never contain `{`).
pub fn stderr_json(bytes: &[u8]) -> Value {
    let text = String::from_utf8_lossy(bytes);
    let start = text
        .find('{')
        .expect("stderr should contain a JSON document");
    serde_json::from_str(&text[start..]).expect("stderr JSON should parse")
}

/// A unique temporary path (not created) with the given prefix.
pub fn temp_marker(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    path
}

pub fn unique_suffix() -> u128 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be available")
        .as_nanos();
    now + u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}
