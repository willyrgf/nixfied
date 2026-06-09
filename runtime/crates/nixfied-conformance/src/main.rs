//! Per-check conformance closure.
//!
//! This is the assertion logic behind the nixfied self-conformance workflow's
//! task nodes, not a standalone orchestrator. Each invocation runs exactly one
//! `--check <name>`, driving a subject through nixfied's public surfaces the way
//! an operator would, and asserting on operator-observable outputs (the run
//! JSON, the marker-gated clean result, and golden snapshots of the
//! schema/docs/capabilities views). The orchestration - ordering, readiness
//! gates, cancellation, summaries - is the nixfied runtime running the workflow.
//!
//! Two layers feed it:
//!   - capability checks (`minimal`, `postgres`, `workflow`, `polyglot`,
//!     `downstream`) drive a nix-built example model (passed by `--model`)
//!     through the nix-built runtime in an isolated state dir;
//!   - the `adoption` check scaffolds a throwaway git repo and runs the real
//!     `#install` + `#upgrade` against `path:<checkout>`, then builds and runs;
//!   - `negative` asserts the gate distinguishes pass from fail (an undeclared
//!     workflow must be refused).
//!
//! Every check writes a ground-truth artifact (its verdict) so results are
//! inspectable independently of "the workflow said ok". `--update-goldens`
//! rewrites the committed view snapshots instead of comparing them.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Verdict {
    check: String,
    status: &'static str,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    computed_model_hash: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    services: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nodes: Vec<String>,
}

struct Args {
    check: String,
    model: Option<PathBuf>,
    update_goldens: bool,
}

fn main() {
    let args = parse_args();
    let outcome = run_check(&args);
    let (status, reason) = match &outcome {
        Ok(_) => ("pass", "ok".to_string()),
        Err(reason) => ("fail", reason.clone()),
    };
    let success = outcome.is_ok();
    let observed = outcome.unwrap_or_default();
    let verdict = Verdict {
        check: args.check.clone(),
        status,
        reason,
        computed_model_hash: observed.computed_model_hash,
        services: observed.services,
        nodes: observed.nodes,
    };
    write_artifact(&args.check, &verdict);
    println!(
        "{}",
        serde_json::to_string_pretty(&verdict).expect("verdict serializes")
    );
    if !success {
        std::process::exit(1);
    }
}

#[derive(Default)]
struct Observed {
    computed_model_hash: Option<String>,
    services: Vec<String>,
    nodes: Vec<String>,
}

fn run_check(args: &Args) -> Result<Observed, String> {
    match args.check.as_str() {
        "minimal" => capability(args, None, &["synthetic"], &[]),
        "postgres" => capability(args, None, &["postgres"], &[]),
        "workflow" => capability(args, Some("pipeline"), &["synthetic"], &["probe", "verify"]),
        "polyglot" => capability(args, None, &["api", "worker"], &[]),
        "downstream" => capability(
            args,
            Some("release"),
            &[],
            &["db-check", "api-check", "worker-check"],
        ),
        "negative" => negative(args),
        "adoption" => adoption(),
        other => Err(format!("unknown check: {other}")),
    }
}

/// Drive one example model through the runtime and assert on the run JSON and the
/// marker-gated clean, then diff the generated views against the goldens.
fn capability(
    args: &Args,
    workflow: Option<&str>,
    expect_services: &[&str],
    expect_nodes: &[&str],
) -> Result<Observed, String> {
    let model_json = args
        .model
        .as_ref()
        .ok_or_else(|| format!("check {} requires --model", args.check))?;
    if !model_json.starts_with("/nix/store/") {
        return Err(format!(
            "model {} is not a store output",
            model_json.display()
        ));
    }
    let model_dir = model_json
        .parent()
        .ok_or_else(|| "model path has no parent".to_string())?;
    let project_id = read_project_id(model_json)?;

    let state = scratch_dir("state")?;
    let work = scratch_dir("work")?;

    let run = run_model(model_json, workflow, &state, &work)?;
    let hash = run
        .get("computedModelHash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let services = string_array(&run, "services", "serviceId");
    let nodes = string_array(&run, "workflowNodes", "nodeId");

    if !expect_services.is_empty() {
        assert_set(&services, expect_services, "services")?;
    }
    assert_tasks_succeeded(&run)?;
    if !expect_nodes.is_empty() {
        assert_order(&nodes, expect_nodes, "workflow nodes")?;
        assert_nodes_succeeded(&run)?;
    }

    clean_model(model_json, &state, &work)?;
    let state_root = state.join(&project_id).join("dev").join("0");
    if state_root.exists() {
        return Err(format!(
            "clean did not remove slot state root {}",
            state_root.display()
        ));
    }

    diff_goldens(&args.check, model_dir, args.update_goldens)?;

    let _ = std::fs::remove_dir_all(&state);
    let _ = std::fs::remove_dir_all(&work);
    Ok(Observed {
        computed_model_hash: hash,
        services,
        nodes,
    })
}

/// The standing proof that the gate distinguishes pass from fail: selecting an
/// undeclared workflow must be refused. The check passes only when the inner run
/// fails; if the runtime ever accepted it, this task fails the workflow.
fn negative(args: &Args) -> Result<Observed, String> {
    let model_json = args
        .model
        .as_ref()
        .ok_or_else(|| "negative check requires --model".to_string())?;
    let state = scratch_dir("state")?;
    let work = scratch_dir("work")?;
    let result = run_model(model_json, Some("does-not-exist"), &state, &work);
    let _ = std::fs::remove_dir_all(&state);
    let _ = std::fs::remove_dir_all(&work);
    match result {
        Ok(_) => {
            Err("expected the run to fail for an undeclared workflow, but it succeeded".into())
        }
        Err(_) => Ok(Observed::default()),
    }
}

/// Scaffold a throwaway git repo, run the real `#install` + `#upgrade` pinning
/// `path:<checkout>`, build, run, and rebuild - the full adoption loop.
fn adoption() -> Result<Observed, String> {
    let checkout = checkout()?;
    let pin = format!("path:{}", checkout.display());
    let project = scratch_dir("adopt")?;
    let result = adoption_inner(&checkout, &pin, &project);
    let _ = std::fs::remove_dir_all(&project);
    result
}

fn adoption_inner(checkout: &Path, pin: &str, project: &Path) -> Result<Observed, String> {
    git(project, &["init", "-q"])?;
    git(project, &["config", "user.email", "gate@nixfied"])?;
    git(project, &["config", "user.name", "nixfied gate"])?;

    // Real install surface: scaffold flake.nix + nixfied.nix pinning the checkout.
    nix_run(
        checkout,
        "install",
        &[
            "--root",
            &project.to_string_lossy(),
            "--project-id",
            "adopt",
            "--name",
            "adopt",
            "--nixfied-url",
            pin,
        ],
    )?;
    git(project, &["add", "-A"])?;
    git(project, &["commit", "-q", "-m", "scaffold"])?;

    let model = nix_build_model(project)?;
    let state = scratch_dir("adopt-state")?;
    let work = scratch_dir("adopt-work")?;
    let run = run_model(&model, None, &state, &work)?;
    let hash = run
        .get("computedModelHash")
        .and_then(Value::as_str)
        .map(str::to_string);
    assert_tasks_succeeded(&run)?;
    clean_model(&model, &state, &work)?;

    // Real upgrade surface: repin and refresh the lock without touching the
    // project-owned nixfied.nix.
    let before = std::fs::read_to_string(project.join("nixfied.nix"))
        .map_err(|error| format!("failed to read scaffolded nixfied.nix: {error}"))?;
    nix_run(
        checkout,
        "upgrade",
        &["--root", &project.to_string_lossy(), "--nixfied-url", pin],
    )?;
    let after = std::fs::read_to_string(project.join("nixfied.nix"))
        .map_err(|error| format!("failed to read nixfied.nix after upgrade: {error}"))?;
    if before != after {
        return Err("upgrade modified the project-owned nixfied.nix".into());
    }
    git(project, &["add", "-A"])?;
    git(project, &["commit", "-q", "-m", "upgrade"])?;

    // Rebuild after upgrade and run again.
    let model = nix_build_model(project)?;
    let state2 = scratch_dir("adopt-state")?;
    let work2 = scratch_dir("adopt-work")?;
    let rerun = run_model(&model, None, &state2, &work2)?;
    assert_tasks_succeeded(&rerun)?;
    clean_model(&model, &state2, &work2)?;

    for dir in [state, work, state2, work2] {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(Observed {
        computed_model_hash: hash,
        services: Vec::new(),
        nodes: Vec::new(),
    })
}

// ----- runtime + nix drivers -----------------------------------------------

fn run_model(
    model_json: &Path,
    workflow: Option<&str>,
    state: &Path,
    work: &Path,
) -> Result<Value, String> {
    let runtime = runtime_bin()?;
    let mut command = Command::new(&runtime);
    command
        .arg("run")
        .arg("--model")
        .arg(model_json)
        .arg("--timeout-ms")
        .arg("60000");
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

fn clean_model(model_json: &Path, state: &Path, work: &Path) -> Result<(), String> {
    let runtime = runtime_bin()?;
    let output = Command::new(&runtime)
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

fn nix_run(checkout: &Path, app: &str, app_args: &[&str]) -> Result<(), String> {
    let installable = format!("{}#{app}", checkout.display());
    let mut command = Command::new("nix");
    command.args(["run", &installable, "--"]).args(app_args);
    let output = command
        .output()
        .map_err(|error| format!("failed to invoke nix run #{app}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "nix run #{app} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn nix_build_model(project: &Path) -> Result<PathBuf, String> {
    let installable = format!("{}#model", project.display());
    let output = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths"])
        .arg(&installable)
        .output()
        .map_err(|error| format!("failed to invoke nix build: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "nix build #model failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(format!("{out}/model.json")))
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| format!("failed to invoke git {args:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

// ----- assertions ----------------------------------------------------------

fn assert_set(actual: &[String], expected: &[&str], label: &str) -> Result<(), String> {
    let mut actual_sorted = actual.to_vec();
    actual_sorted.sort();
    let mut expected_sorted = expected.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    expected_sorted.sort();
    if actual_sorted != expected_sorted {
        return Err(format!(
            "{label} {actual_sorted:?} did not match expected {expected_sorted:?}"
        ));
    }
    Ok(())
}

fn assert_order(actual: &[String], expected: &[&str], label: &str) -> Result<(), String> {
    let expected = expected.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if actual != expected.as_slice() {
        return Err(format!(
            "{label} {actual:?} did not match expected {expected:?}"
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
        if !task
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let id = task
                .get("taskId")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            return Err(format!("task {id} did not succeed"));
        }
    }
    Ok(())
}

fn assert_nodes_succeeded(run: &Value) -> Result<(), String> {
    let nodes = run
        .get("workflowNodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "run output has no workflowNodes".to_string())?;
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

fn string_array(run: &Value, array: &str, field: &str) -> Vec<String> {
    run.get(array)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get(field).and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

// ----- golden view snapshots -----------------------------------------------

const GOLDEN_VIEWS: [&str; 3] = ["schema.json", "capabilities.json", "docs.md"];

fn diff_goldens(check: &str, model_dir: &Path, update: bool) -> Result<(), String> {
    let checkout = checkout()?;
    let golden_dir = checkout
        .join("runtime/crates/nixfied-conformance/goldens")
        .join(check);
    let system = read_target_system(&model_dir.join("model.json"))?;
    if update {
        std::fs::create_dir_all(&golden_dir)
            .map_err(|error| format!("failed to create golden dir: {error}"))?;
    }
    for view in GOLDEN_VIEWS {
        let actual_path = model_dir.join("views").join(view);
        let actual = std::fs::read_to_string(&actual_path)
            .map_err(|error| format!("failed to read view {}: {error}", actual_path.display()))?;
        let normalized = actual.replace(&system, "<<system>>");
        let golden_path = golden_dir.join(view);
        if update {
            std::fs::write(&golden_path, &normalized).map_err(|error| {
                format!("failed to write golden {}: {error}", golden_path.display())
            })?;
            continue;
        }
        let golden = std::fs::read_to_string(&golden_path).map_err(|error| {
            format!(
                "missing golden {} ({error}); run --update-goldens",
                golden_path.display()
            )
        })?;
        if golden != normalized {
            return Err(format!(
                "view {view} drifted from golden {}",
                golden_path.display()
            ));
        }
    }
    Ok(())
}

// ----- helpers -------------------------------------------------------------

fn read_project_id(model_json: &Path) -> Result<String, String> {
    let model: Value = read_json(model_json)?;
    model
        .get("project")
        .and_then(|project| project.get("projectId"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "model has no project.projectId".to_string())
}

fn read_target_system(model_json: &Path) -> Result<String, String> {
    let model: Value = read_json(model_json)?;
    model
        .get("target")
        .and_then(|target| target.get("system"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "model has no target.system".to_string())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

/// The runtime binary: a sibling of this binary in the same nix-built package,
/// overridable for local runs.
fn runtime_bin() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("NIXFIED_RUNTIME_BIN") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()
        .map_err(|error| format!("failed to locate current executable: {error}"))?;
    let dir = current
        .parent()
        .ok_or_else(|| "current executable has no parent".to_string())?;
    let candidate = dir.join("nixfied-runtime");
    if candidate.exists() {
        Ok(candidate)
    } else {
        Err(format!(
            "nixfied-runtime not found next to {}; set NIXFIED_RUNTIME_BIN",
            current.display()
        ))
    }
}

/// The checkout under test: the runtime invokes conformance tasks with cwd set to
/// the admitted source root (the repo), so the working directory is the checkout.
fn checkout() -> Result<PathBuf, String> {
    let raw = match std::env::var("NIXFIED_CONFORMANCE_CHECKOUT") {
        Ok(value) => PathBuf::from(value),
        Err(_) => std::env::current_dir()
            .map_err(|error| format!("failed to inspect working directory: {error}"))?,
    };
    raw.canonicalize()
        .map_err(|error| format!("failed to canonicalize checkout {}: {error}", raw.display()))
}

fn artifacts_dir() -> PathBuf {
    match std::env::var("NIXFIED_CONFORMANCE_ARTIFACTS") {
        Ok(value) => PathBuf::from(value),
        Err(_) => std::env::temp_dir().join("nixfied-conformance"),
    }
}

fn write_artifact(check: &str, verdict: &Verdict) {
    let dir = artifacts_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{check}.json"));
    if let Ok(bytes) = serde_json::to_vec_pretty(verdict) {
        let _ = std::fs::write(path, bytes);
    }
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
    dir.canonicalize()
        .map_err(|error| format!("failed to canonicalize scratch dir: {error}"))
}

fn parse_args() -> Args {
    let mut check = None;
    let mut model = None;
    let mut update_goldens = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => {
                check = Some(args.next().unwrap_or_else(|| fail("missing --check value")));
            }
            "--model" => {
                model = Some(PathBuf::from(
                    args.next().unwrap_or_else(|| fail("missing --model value")),
                ));
            }
            "--update-goldens" => update_goldens = true,
            other => fail(&format!("unknown argument: {other}")),
        }
    }
    Args {
        check: check.unwrap_or_else(|| fail("missing --check <name>")),
        model,
        update_goldens,
    }
}

fn fail(message: &str) -> ! {
    eprintln!("nixfied-conformance: {message}");
    std::process::exit(2);
}
