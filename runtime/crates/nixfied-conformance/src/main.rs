//! Downstream conformance harness.
//!
//! A black-box suite that drives Nixfied only through its public surfaces, the
//! way a downstream operator would: it builds each example model from the
//! checkout under test with `nix build`, runs the real `nixfied-runtime` binary
//! against the store model, and asserts on operator-observable outputs (the run
//! JSON, the written summary, and the marker-gated clean result). Scenarios are
//! declarative data; results are emitted as a machine-readable JSON report.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

/// One declarative scenario over a compiled example model.
struct Scenario {
    name: &'static str,
    project_id: &'static str,
    model_attr: &'static str,
    workflow: Option<&'static str>,
    expect_services: &'static [&'static str],
    expect_nodes: &'static [&'static str],
    /// When true the run is expected to fail (negative self-test).
    expect_run_failure: bool,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "minimal-service",
            project_id: "m0-minimal",
            model_attr: "m0-minimal-model",
            workflow: None,
            expect_services: &["synthetic"],
            expect_nodes: &[],
            expect_run_failure: false,
        },
        Scenario {
            name: "postgres-adapter",
            project_id: "postgres-example",
            model_attr: "postgres-model",
            workflow: None,
            expect_services: &["postgres"],
            expect_nodes: &[],
            expect_run_failure: false,
        },
        Scenario {
            name: "workflow-graph",
            project_id: "workflow-example",
            model_attr: "workflow-model",
            workflow: Some("pipeline"),
            expect_services: &["synthetic"],
            expect_nodes: &["probe", "verify"],
            expect_run_failure: false,
        },
        Scenario {
            name: "polyglot-stack",
            project_id: "polyglot-stack",
            model_attr: "polyglot-stack-model",
            workflow: None,
            expect_services: &["api", "worker"],
            expect_nodes: &[],
            expect_run_failure: false,
        },
        // Negative self-test: selecting an undeclared workflow must fail, proving
        // the harness distinguishes pass from fail with a structured reason.
        Scenario {
            name: "negative-unknown-workflow",
            project_id: "m0-minimal",
            model_attr: "m0-minimal-model",
            workflow: Some("does-not-exist"),
            expect_services: &[],
            expect_nodes: &[],
            expect_run_failure: true,
        },
    ]
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioReport {
    name: String,
    status: &'static str,
    reason: String,
    duration_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    computed_model_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    scenarios: Vec<ScenarioReport>,
    passed: usize,
    failed: usize,
}

fn main() {
    let mut checkout = PathBuf::from(".");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--checkout" => {
                checkout = args
                    .next()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| fail("missing --checkout value"));
            }
            other => fail(&format!("unknown argument: {other}")),
        }
    }
    let checkout = checkout
        .canonicalize()
        .unwrap_or_else(|error| fail(&format!("invalid --checkout: {error}")));

    let runtime_bin = match build_runtime(&checkout) {
        Ok(path) => path,
        Err(error) => fail(&format!("failed to build nixfied-runtime: {error}")),
    };

    let mut reports = Vec::new();
    for scenario in scenarios() {
        let started = Instant::now();
        let (status, reason, hash) = run_scenario(&checkout, &runtime_bin, &scenario);
        reports.push(ScenarioReport {
            name: scenario.name.to_string(),
            status,
            reason,
            duration_ms: started.elapsed().as_millis(),
            computed_model_hash: hash,
        });
    }

    let failed = reports.iter().filter(|r| r.status == "fail").count();
    let passed = reports.len() - failed;
    let report = Report {
        scenarios: reports,
        passed,
        failed,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("report serializes")
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

fn run_scenario(
    checkout: &Path,
    runtime_bin: &Path,
    scenario: &Scenario,
) -> (&'static str, String, Option<String>) {
    match execute(checkout, runtime_bin, scenario) {
        Ok(hash) => ("pass", "ok".to_string(), hash),
        Err(reason) => ("fail", reason, None),
    }
}

/// Execute a scenario through public surfaces, returning the computed model hash
/// on success.
fn execute(
    checkout: &Path,
    runtime_bin: &Path,
    scenario: &Scenario,
) -> Result<Option<String>, String> {
    let model = nix_build(checkout, scenario.model_attr)?;
    if !model.starts_with("/nix/store/") {
        return Err(format!(
            "model for {} is not a store output: {model}",
            scenario.name
        ));
    }
    let model_json = format!("{model}/model.json");

    let work = scratch_dir("work")?;
    let state = scratch_dir("state")?;

    let run = run_runtime(runtime_bin, &model_json, scenario.workflow, &state, &work);

    if scenario.expect_run_failure {
        return match run {
            Ok(_) => Err("expected the run to fail but it succeeded".to_string()),
            Err(_) => Ok(None),
        };
    }

    let run = run?;
    let hash = run
        .get("computedModelHash")
        .and_then(Value::as_str)
        .map(str::to_string);

    assert_services(&run, scenario.expect_services)?;
    assert_tasks_succeeded(&run)?;
    if !scenario.expect_nodes.is_empty() {
        assert_workflow_nodes(&run, scenario.expect_nodes)?;
    }

    clean_runtime(runtime_bin, &model_json, &state, &work)?;
    let state_root = state.join(scenario.project_id).join("dev").join("0");
    if state_root.exists() {
        return Err(format!(
            "clean did not remove slot state root {}",
            state_root.display()
        ));
    }

    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(&state);
    Ok(hash)
}

fn assert_services(run: &Value, expected: &[&str]) -> Result<(), String> {
    let actual = run
        .get("services")
        .and_then(Value::as_array)
        .map(|services| {
            services
                .iter()
                .filter_map(|service| service.get("serviceId").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let expected = expected.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if actual != expected {
        return Err(format!(
            "services {actual:?} did not match expected {expected:?}"
        ));
    }
    Ok(())
}

fn assert_tasks_succeeded(run: &Value) -> Result<(), String> {
    let tasks = run
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| "run output has no tasks array".to_string())?;
    if tasks.is_empty() {
        return Err("run executed no tasks".to_string());
    }
    for task in tasks {
        let success = task
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !success {
            let id = task
                .get("taskId")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            return Err(format!("task {id} did not succeed"));
        }
    }
    Ok(())
}

fn assert_workflow_nodes(run: &Value, expected: &[&str]) -> Result<(), String> {
    let nodes = run
        .get("workflowNodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "run output has no workflowNodes".to_string())?;
    let actual = nodes
        .iter()
        .filter_map(|node| node.get("nodeId").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let expected = expected.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if actual != expected {
        return Err(format!(
            "workflow node order {actual:?} did not match expected {expected:?}"
        ));
    }
    for node in nodes {
        if !node
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err("a workflow node did not succeed".to_string());
        }
    }
    Ok(())
}

fn build_runtime(checkout: &Path) -> Result<PathBuf, String> {
    let manifest = checkout.join("runtime/Cargo.toml");
    let output = Command::new("cargo")
        .args(["build", "-p", "nixfied-runtime", "--bin", "nixfied-runtime"])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .map_err(|error| format!("failed to invoke cargo: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let bin = checkout.join("runtime/target/debug/nixfied-runtime");
    if !bin.exists() {
        return Err(format!("runtime binary not found at {}", bin.display()));
    }
    Ok(bin)
}

fn nix_build(checkout: &Path, attr: &str) -> Result<String, String> {
    let installable = format!("{}#{attr}", checkout.display());
    let output = Command::new("nix")
        .args([
            "build",
            "--no-link",
            "--no-write-lock-file",
            "--print-out-paths",
        ])
        .arg(&installable)
        .output()
        .map_err(|error| format!("failed to invoke nix: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "nix build {attr} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_runtime(
    runtime_bin: &Path,
    model_json: &str,
    workflow: Option<&str>,
    state: &Path,
    work: &Path,
) -> Result<Value, String> {
    let mut command = Command::new(runtime_bin);
    command
        .arg("run")
        .arg("--model")
        .arg(model_json)
        .arg("--timeout-ms")
        .arg("30000");
    if let Some(workflow) = workflow {
        command.arg("--workflow").arg(workflow);
    }
    let output = command
        .current_dir(work)
        .env("NIXFIED_STATE_DIR", state)
        .output()
        .map_err(|error| format!("failed to invoke runtime: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "runtime run failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("run output was not valid JSON: {error}"))
}

fn clean_runtime(
    runtime_bin: &Path,
    model_json: &str,
    state: &Path,
    work: &Path,
) -> Result<(), String> {
    let output = Command::new(runtime_bin)
        .arg("clean")
        .arg("--model")
        .arg(model_json)
        .current_dir(work)
        .env("NIXFIED_STATE_DIR", state)
        .output()
        .map_err(|error| format!("failed to invoke runtime clean: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "runtime clean failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn scratch_dir(kind: &str) -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "nixfied-conformance-{}-{kind}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create scratch dir {}: {error}", dir.display()))?;
    Ok(dir)
}

fn fail(message: &str) -> ! {
    eprintln!("nixfied-conformance: {message}");
    std::process::exit(2);
}
