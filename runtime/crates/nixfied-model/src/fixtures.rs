//! Test-only model fixtures, behind the `test-fixtures` feature. The single
//! source of truth for the synthetic model shape consumers test against; keeping
//! it next to the types means a contract change updates the fixture in the same
//! crate that broke it.

use serde_json::{Value, json};

use crate::constants::{MODEL_VERSION, TOOLCHAIN_ID, runtime_abi};

/// The synthetic-service executable the default fixture binds.
pub const SYNTHETIC_EXECUTABLE: &str = "/nix/store/test-synthetic-helper/bin/synthetic-helper";
/// The synthetic service's start arguments.
pub const SYNTHETIC_START_ARGS: &[&str] = &["service", "--host", "127.0.0.1", "--port", "${port}"];

/// Knobs for [`synthetic_model`]. `..Default::default()` gives the canonical
/// fixture: the synthetic helper on the host system, slot 0, ports 42000-42063.
pub struct SyntheticModelOptions {
    pub executable: String,
    pub start_args: Vec<String>,
    pub port_start: u16,
    pub port_end: u16,
    pub project_id: String,
    pub project_name: String,
    pub system: String,
    pub os: String,
    pub arch: String,
    pub docs_title: String,
    pub docs_summary: String,
}

impl Default for SyntheticModelOptions {
    fn default() -> Self {
        Self {
            executable: SYNTHETIC_EXECUTABLE.to_string(),
            start_args: SYNTHETIC_START_ARGS.iter().map(|s| s.to_string()).collect(),
            port_start: 42000,
            port_end: 42063,
            project_id: "runtime-test".to_string(),
            project_name: "Runtime Test".to_string(),
            system: host_system(),
            os: host_os().to_string(),
            arch: host_arch().to_string(),
            docs_title: "Runtime Test".to_string(),
            docs_summary: "Runtime admission fixture.".to_string(),
        }
    }
}

/// The canonical admission fixture — a `synthetic` foreground service plus a
/// `smoke` task in slot 0 over the given candidate port window. A valid model
/// by construction: it deserializes into [`crate::Model`] and passes
/// [`crate::Validate`].
pub fn synthetic_model(options: &SyntheticModelOptions) -> Value {
    // The fixture binds one closure; its store path is the executable's
    // grandparent (…/store-path/bin/exe), matching what Nix emits.
    let store_path = std::path::Path::new(&options.executable)
        .ancestors()
        .nth(2)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/nix/store/test-synthetic-helper".to_string());
    let program = std::path::Path::new(&options.executable)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "synthetic-helper".to_string());
    let mut start_run = vec![program.clone()];
    start_run.extend(options.start_args.iter().cloned());
    let mut task_run = vec![program];
    task_run.extend(
        ["task", "--host", "127.0.0.1", "--port", "${port}"]
            .iter()
            .map(|s| s.to_string()),
    );
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
            "projectId": options.project_id,
            "name": options.project_name
        },
        "target": {
            "system": options.system,
            "os": options.os,
            "arch": options.arch,
            "closureSystem": options.system
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
                        "start": options.port_start,
                        "end": options.port_end
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
                "storePath": store_path,
                "executable": options.executable,
                "targetSystem": options.system,
                "operationBindings": [
                    "service.synthetic.start",
                    "task.smoke.run"
                ],
                "requiresExecutable": true,
                "effects": ["process", "network-listener"]
            }
        },
        "services": {
            "synthetic": {
                "lifecycle": {
                    "prepare": {
                        "operationId": "service.synthetic.prepare",
                        "terminal": { "success": "prepared", "failure": "failed" }
                    },
                    "start": {
                        "operationId": "service.synthetic.start",
                        "invocation": invocation(&options.executable, start_run),
                        "terminal": { "success": "spawned", "failure": "failed" }
                    },
                    "ready": {
                        "operationId": "service.synthetic.ready",
                        "probe": { "kind": "tcp", "timeoutMs": 250, "retryIntervalMs": 25, "maxAttempts": 40 },
                        "terminal": { "success": "ready", "failure": "not-ready" }
                    },
                    "health": {
                        "operationId": "service.synthetic.health",
                        "probe": { "kind": "tcp", "timeoutMs": 250, "retryIntervalMs": 25, "maxAttempts": 40 },
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
                "endpoints": { "synthetic-tcp": { "endpointId": "synthetic-tcp", "host": "127.0.0.1" } },
                "primaryEndpoint": "synthetic-tcp",
                "connectsTo": [],
                "stateRefs": ["slot"],
                "logRefs": ["service.synthetic"],
                "containment": "process-group"
            }
        },
        "tasks": {
            "smoke": {
                "kind": "leaf",
                "operationId": "task.smoke.run",
                "invocation": invocation(&options.executable, task_run),
                "requires": ["synthetic"],
                "servicesRequired": ["synthetic"],
                "exitPolicy": {
                    "successCodes": [0]
                },
                "artifactRefs": [],
                "logRefs": ["task.smoke"],
                "summaryRefs": ["summary"]
            }
        },
        "docs": {
            "title": options.docs_title,
            "summary": options.docs_summary
        }
    })
}

/// An inline invocation over the fixture's single tool closure.
fn invocation(executable: &str, run: Vec<String>) -> Value {
    json!({
        "tools": ["synthetic-helper"],
        "run": run,
        "executable": executable,
        "env": {},
        "codebaseId": "main",
        "cwd": ".",
        "stdin": "null",
        "timeoutMs": 30000
    })
}

pub fn host_system() -> String {
    format!("{}-{}", host_arch(), host_os())
}

pub fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

pub fn host_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    /// The fixture is valid by construction: it round-trips through the typed
    /// contract and passes the model's own validation.
    #[test]
    fn default_fixture_is_a_valid_model() {
        let value = synthetic_model(&SyntheticModelOptions::default());
        let model: crate::Model = serde_json::from_value(value).expect("fixture deserializes");
        model.validate().expect("fixture validates");
    }
}
