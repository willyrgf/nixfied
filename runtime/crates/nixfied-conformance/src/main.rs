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
use std::process::{Child, Command, Stdio};

use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Verdict {
    check: String,
    status: &'static str,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    computed_model_hash: Option<String>,
    /// What the check actually observed - recorded so the depth (which services
    /// started on which ports/PIDs, which steps ran) is inspectable in the
    /// ground-truth artifact without re-running, not just "pass".
    #[serde(skip_serializing_if = "Map::is_empty")]
    observed: Map<String, Value>,
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
        observed: observed.observed,
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
    observed: Map<String, Value>,
}

impl Observed {
    fn record(&mut self, key: &str, value: Value) {
        self.observed.insert(key.to_string(), value);
    }
}

/// Per-service ground truth from a run's JSON: the live identity that proves a
/// service is a distinct running instance (its selected port, runtime-assigned
/// process key/PID, and layered service-instance id), not just a config value.
fn service_instances(run: &Value) -> Vec<Value> {
    run.get("services")
        .and_then(Value::as_array)
        .map(|services| {
            services
                .iter()
                .map(|service| {
                    json!({
                        "serviceId": service.get("serviceId"),
                        "port": service
                            .get("selectedEndpoint")
                            .and_then(|endpoint| endpoint.get("port")),
                        "processKey": service.get("processKey"),
                        "serviceInstanceId": service.get("serviceInstanceId"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Per-task ground truth: id, success, and observed exit code.
fn task_results(run: &Value) -> Vec<Value> {
    run.get("tasks")
        .and_then(Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .map(|task| {
                    json!({
                        "taskId": task.get("taskId"),
                        "success": task.get("success"),
                        "exitCode": task.get("exitCode"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
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
            // `gate` is a task that depends on all three services (api/worker/postgres).
            &["db-check", "api-check", "worker-check", "gate"],
        ),
        "slots" => slots(args),
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

    let instances = service_instances(&run);
    let tasks = task_results(&run);

    clean_model(model_json, &state, &work)?;
    let state_root = state.join(&project_id).join("dev").join("0");
    if state_root.exists() {
        return Err(format!(
            "clean did not remove slot state root {}",
            state_root.display()
        ));
    }

    assert_views_project_model(model_json, model_dir)?;
    let goldens = if args.update_goldens {
        "updated"
    } else {
        "matched"
    };
    diff_goldens(&args.check, model_dir, args.update_goldens)?;

    let _ = std::fs::remove_dir_all(&state);
    let _ = std::fs::remove_dir_all(&work);
    let mut observed = Observed {
        computed_model_hash: hash,
        observed: Map::new(),
    };
    if let Some(workflow) = workflow {
        observed.record("workflow", json!(workflow));
    }
    observed.record("services", json!(instances));
    observed.record("tasks", json!(tasks));
    if !nodes.is_empty() {
        observed.record("nodeOrder", json!(nodes));
    }
    observed.record("markerGatedCleanRemovedState", json!(true));
    observed.record("viewsProjectModel", json!(true));
    observed.record("goldens", json!(goldens));
    Ok(observed)
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
        Err(reason) => {
            let mut observed = Observed::default();
            observed.record("selectedWorkflow", json!("does-not-exist"));
            observed.record("expectedFailure", json!(true));
            observed.record("refusedWith", json!(reason));
            Ok(observed)
        }
    }
}

/// Slot isolation: run two slots of a multi-slot model concurrently against one
/// shared state base and assert disjoint placement (distinct ports, service
/// instances, and state roots), then clean each slot independently.
fn slots(args: &Args) -> Result<Observed, String> {
    let model_json = args
        .model
        .as_ref()
        .ok_or_else(|| "slots check requires --model".to_string())?;
    let project_id = read_project_id(model_json)?;
    let state = scratch_dir("slots-state")?;
    let work0 = scratch_dir("slots-work0")?;
    let work1 = scratch_dir("slots-work1")?;

    let child0 = spawn_run_slot(model_json, 0, &state, &work0)?;
    let child1 = spawn_run_slot(model_json, 1, &state, &work1)?;
    let run0 = wait_run(child0, 0)?;
    let run1 = wait_run(child1, 1)?;

    assert_tasks_succeeded(&run0)?;
    assert_tasks_succeeded(&run1)?;

    // Two genuinely distinct live instances per slot: distinct selected ports,
    // distinct runtime process keys (PIDs), and distinct layered service-instance
    // ids - not the same process behind two config values.
    let svc0 = service_instances(&run0);
    let svc1 = service_instances(&run1);
    let ports0 = service_ports(&run0);
    let ports1 = service_ports(&run1);
    if ports0.is_empty() || ports1.is_empty() {
        return Err("a slot run reported no service endpoints".into());
    }
    if ports0.iter().any(|port| ports1.contains(port)) {
        return Err(format!(
            "slot port windows overlapped: {ports0:?} vs {ports1:?}"
        ));
    }
    let instances0 = string_array(&run0, "services", "serviceInstanceId");
    let instances1 = string_array(&run1, "services", "serviceInstanceId");
    if instances0.iter().any(|id| instances1.contains(id)) {
        return Err("slots shared a serviceInstanceId".into());
    }
    let keys0 = string_array(&run0, "services", "processKey");
    let keys1 = string_array(&run1, "services", "processKey");
    if keys0.iter().any(|key| keys1.contains(key)) {
        return Err("slots shared a service processKey (same OS process)".into());
    }

    let root0 = state.join(&project_id).join("dev").join("0");
    let root1 = state.join(&project_id).join("dev").join("1");
    if !root0.exists() || !root1.exists() {
        return Err("a slot state root was not materialized".into());
    }
    // The downstream subject runs Postgres; each slot must own a separate on-disk
    // data cluster (its own files), captured before the marker-gated clean.
    let data0 = root0.join("pgdata");
    let data1 = root1.join("pgdata");
    if !data0.join("PG_VERSION").is_file() || !data1.join("PG_VERSION").is_file() {
        return Err("each slot must own a separate Postgres data cluster".into());
    }

    clean_model_slot(model_json, 0, &state, &work0)?;
    if root0.exists() || !root1.exists() {
        return Err("cleaning slot 0 disturbed slot 1 or left slot 0 behind".into());
    }
    clean_model_slot(model_json, 1, &state, &work1)?;
    if root1.exists() {
        return Err("clean did not remove slot 1 state root".into());
    }

    let mut observed = Observed::default();
    observed.record(
        "slots",
        json!([
            {
                "slot": 0,
                "services": svc0,
                "stateRoot": root0.to_string_lossy(),
                "postgresDataDir": data0.to_string_lossy(),
            },
            {
                "slot": 1,
                "services": svc1,
                "stateRoot": root1.to_string_lossy(),
                "postgresDataDir": data1.to_string_lossy(),
            },
        ]),
    );
    observed.record("ranConcurrently", json!(true));
    observed.record("portsDisjoint", json!(true));
    observed.record("serviceInstancesDistinct", json!(true));
    observed.record("processKeysDistinct", json!(true));
    observed.record("separatePostgresDataClusters", json!(true));
    observed.record("cleanIsolatedPerSlot", json!(true));

    for dir in [state, work0, work1] {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(observed)
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

    // Re-running install must refuse to clobber the project-owned flake.nix.
    if nix_run(checkout, "install", &["--root", &project.to_string_lossy()]).is_ok() {
        return Err("re-running install did not refuse an existing flake.nix".into());
    }

    let model = nix_build_model(project)?;
    let state = scratch_dir("adopt-state")?;
    let work = scratch_dir("adopt-work")?;
    let run = run_model(&model, None, &state, &work)?;
    let hash = run
        .get("computedModelHash")
        .and_then(Value::as_str)
        .map(str::to_string);
    assert_tasks_succeeded(&run)?;
    let run_tasks = task_results(&run);
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
    let rebuilt_hash = rerun
        .get("computedModelHash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let rerun_tasks = task_results(&rerun);
    clean_model(&model, &state2, &work2)?;

    for dir in [state, work, state2, work2] {
        let _ = std::fs::remove_dir_all(dir);
    }
    let mut observed = Observed {
        computed_model_hash: hash.clone(),
        observed: Map::new(),
    };
    observed.record("project", json!("adopt"));
    observed.record("nixfiedPin", json!(pin));
    observed.record(
        "steps",
        json!([
            "install",
            "reinstall-refused",
            "build",
            "run",
            "clean",
            "upgrade",
            "rebuild",
            "run",
            "clean"
        ]),
    );
    observed.record("reinstallRefused", json!(true));
    observed.record("installModelHash", json!(hash));
    observed.record("adoptedRunTasks", json!(run_tasks));
    observed.record("nixfiedNixUnchangedAcrossUpgrade", json!(true));
    observed.record("rebuiltModelHash", json!(rebuilt_hash));
    observed.record("rebuiltRunTasks", json!(rerun_tasks));
    Ok(observed)
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
    clean_model_inner(model_json, None, state, work)
}

fn clean_model_slot(model_json: &Path, slot: u32, state: &Path, work: &Path) -> Result<(), String> {
    clean_model_inner(model_json, Some(slot), state, work)
}

fn clean_model_inner(
    model_json: &Path,
    slot: Option<u32>,
    state: &Path,
    work: &Path,
) -> Result<(), String> {
    let runtime = runtime_bin()?;
    let mut command = Command::new(&runtime);
    command.arg("clean").arg("--model").arg(model_json);
    if let Some(slot) = slot {
        command.arg("--slot").arg(slot.to_string());
    }
    let output = command
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

/// Spawn `run --slot <slot>` without blocking, so two slots can race.
fn spawn_run_slot(
    model_json: &Path,
    slot: u32,
    state: &Path,
    work: &Path,
) -> Result<Child, String> {
    let runtime = runtime_bin()?;
    Command::new(&runtime)
        .arg("run")
        .arg("--model")
        .arg(model_json)
        .arg("--slot")
        .arg(slot.to_string())
        .arg("--timeout-ms")
        .arg("60000")
        .current_dir(work)
        .env("NIXFIED_STATE_DIR", state)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn runtime for slot {slot}: {error}"))
}

fn wait_run(child: Child, slot: u32) -> Result<Value, String> {
    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for slot {slot}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "slot {slot} run failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("slot {slot} run output was not valid JSON: {error}"))
}

fn service_ports(run: &Value) -> Vec<u64> {
    run.get("services")
        .and_then(Value::as_array)
        .map(|services| {
            services
                .iter()
                .filter_map(|service| {
                    service
                        .get("selectedEndpoint")
                        .and_then(|endpoint| endpoint.get("port"))
                        .and_then(Value::as_u64)
                })
                .collect()
        })
        .unwrap_or_default()
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

/// The generated views must stay disposable projections of `model.json`: the
/// schema view names model.json as its source and mirrors the contract identity,
/// and the capabilities view equals `model.capabilities` exactly. (Folded in from
/// the former `tests/views/prove-view-surfaces.sh`, now run for every example.)
/// The framework-owned public command surfaces. Mirrors the runtime's surface set
/// and the Nix view producer; the gate asserts both views agree with it, so a
/// drift in any one is caught.
const FRAMEWORK_SURFACES: [&str; 9] = [
    "model",
    "schema",
    "docs",
    "capabilities",
    "check",
    "run",
    "ps",
    "down",
    "clean",
];

/// The `schema` and `capabilities` views are pure projections of the model, so the
/// gate re-derives them here and cross-checks the emitted views — no checked-in
/// snapshot. (`docs.md` is the human-readable rendering and stays a golden.)
fn assert_views_project_model(model_json: &Path, model_dir: &Path) -> Result<(), String> {
    let model = read_json(model_json)?;

    let schema = read_json(&model_dir.join("views").join("schema.json"))?;
    if schema.get("source").and_then(Value::as_str) != Some("model.json") {
        return Err("schema view source is not model.json".into());
    }
    let model_types = schema
        .get("modelTypes")
        .ok_or_else(|| "schema view has no modelTypes".to_string())?;
    for field in ["modelVersion", "runtimeAbi", "toolchainId"] {
        if model_types.get(field) != model.get(field) {
            return Err(format!(
                "schema view modelTypes.{field} is not the model's {field}"
            ));
        }
    }
    if schema.get("surfaces") != Some(&json!(FRAMEWORK_SURFACES)) {
        return Err("schema view surfaces are not the framework surface set".into());
    }

    let capabilities = read_json(&model_dir.join("views").join("capabilities.json"))?;
    let expected = derive_capabilities(&model)?;
    if capabilities != expected {
        return Err(format!(
            "capabilities view does not project the model: expected {expected}, got {capabilities}"
        ));
    }
    Ok(())
}

/// Re-derive the capabilities projection from the model: the keys of each section,
/// the slot range, and the framework surfaces.
fn derive_capabilities(model: &Value) -> Result<Value, String> {
    let keys = |field: &str| -> Result<Vec<String>, String> {
        model
            .get(field)
            .and_then(Value::as_object)
            .map(|object| object.keys().cloned().collect())
            .ok_or_else(|| format!("model.{field} is not an object"))
    };
    let slot_policy = model
        .get("slotPolicy")
        .ok_or_else(|| "model has no slotPolicy".to_string())?;
    let slot = |field: &str| -> Result<u64, String> {
        slot_policy
            .get(field)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("slotPolicy.{field} is not an integer"))
    };
    Ok(json!({
        "environments": keys("environments")?,
        "slots": (slot("min")?..=slot("max")?).collect::<Vec<_>>(),
        "services": keys("services")?,
        "tasks": keys("tasks")?,
        "workflows": keys("workflows")?,
        "surfaces": FRAMEWORK_SURFACES,
    }))
}

/// Only the human-readable rendering is snapshotted; `schema.json` and
/// `capabilities.json` are derivations, cross-checked in `assert_views_project_model`.
const GOLDEN_VIEWS: [&str; 1] = ["docs.md"];

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
