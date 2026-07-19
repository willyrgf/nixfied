use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use nixfied_model::{Model, Validate};
use serde_json::{Value, json};

mod common;
use common::*;

static ENDPOINT_TESTS: Mutex<()> = Mutex::new(());

const HARNESS: &str = r#"
import pathlib, socket, sys, time

command = sys.argv[1]
if command == "prepare":
    sentinel = pathlib.Path(sys.argv[2])
    acknowledgement = pathlib.Path(sys.argv[3])
    sentinel.parent.mkdir(parents=True, exist_ok=True)
    sentinel.touch()
    if sys.argv[4] == "block":
        deadline = time.monotonic() + 15
        while not acknowledgement.exists():
            if time.monotonic() >= deadline:
                raise SystemExit(70)
            time.sleep(0.02)
elif command == "service":
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", int(sys.argv[2])))
    listener.listen(16)
    if sys.argv[3] == "close":
        time.sleep(0.6)
        listener.close()
        time.sleep(30)
    else:
        while True:
            connection, _ = listener.accept()
            connection.recv(1)
            connection.close()
elif command == "task":
    connection = socket.create_connection(("127.0.0.1", int(sys.argv[2])), timeout=5)
    connection.close()
"#;

#[test]
fn persistent_listener_blocks_an_independent_root_before_prepare_then_releases() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        false,
    );
    let root_a = temp.path.join("root-a");
    let root_b = temp.path.join("root-b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    let first = run_command(&model, &root_a).output().unwrap();
    assert_success(&first, "root A persistent start");
    assert!(find_named(&root_a, "endpoint-prepare-sentinel").is_some());

    let blocked = run_command(&model, &root_b).output().unwrap();
    let error = assert_port_conflict(&blocked, "listener-occupied", port);
    assert!(
        error["details"]["portConflict"]
            .get("nixfiedOwner")
            .is_none(),
        "a private registry in another state root must not be guessed"
    );
    assert!(
        find_named(&root_b, "endpoint-prepare-sentinel").is_none(),
        "preflight conflict must happen before root B prepare"
    );

    assert_success(
        &down_command(&model, &root_a).output().unwrap(),
        "root A down",
    );
    let second = run_command(&model, &root_b).output().unwrap();
    assert_success(&second, "root B start after root A down");
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_some());
    assert_success(
        &down_command(&model, &root_b).output().unwrap(),
        "root B down",
    );
}

#[test]
fn concurrent_roots_have_one_prepare_winner_and_one_lock_loser() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(&temp.path, &python, port, true, "run-scoped", false);
    let root_a = temp.path.join("root-a");
    let root_b = temp.path.join("root-b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    let winner = spawn_run(&model, &root_a);
    let sentinel = wait_for_named(&root_a, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("root A should enter prepare while retaining the lock");
    let loser = spawn_run(&model, &root_b);
    let loser_output = wait_for_output(loser, Duration::from_secs(5));
    assert_port_conflict(&loser_output, "startup-lock-contended", port);
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_none());

    fs::write(sentinel.with_file_name("endpoint-prepare-ack"), b"continue").unwrap();
    let winner_output = wait_for_output(winner, Duration::from_secs(20));
    assert_success(&winner_output, "lock winner");
}

#[test]
fn killing_runtime_during_prepare_releases_lock_not_inherited_by_child() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(&temp.path, &python, port, true, "run-scoped", false);
    let root_a = temp.path.join("root-a");
    let root_b = temp.path.join("root-b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    let runtime = spawn_run(&model, &root_a);
    let sentinel_a = wait_for_named(&root_a, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("first runtime should block in prepare");
    let runtime_pid = runtime.id();
    assert_eq!(
        unsafe { libc::kill(runtime_pid as libc::pid_t, libc::SIGKILL) },
        0
    );
    let killed = wait_for_output(runtime, Duration::from_secs(5));
    assert!(!killed.status.success());
    // Let the now-orphaned prepare child exit promptly. Its exec environment
    // never inherited the CLOEXEC startup-lock descriptor.
    fs::write(
        sentinel_a.with_file_name("endpoint-prepare-ack"),
        b"orphan-exit",
    )
    .unwrap();

    let successor = spawn_run(&model, &root_b);
    let sentinel_b = wait_for_named(&root_b, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("successor should acquire the released kernel lock");
    fs::write(
        sentinel_b.with_file_name("endpoint-prepare-ack"),
        b"continue",
    )
    .unwrap();
    assert_success(
        &wait_for_output(successor, Duration::from_secs(20)),
        "successor after runtime SIGKILL",
    );
}

#[test]
fn external_bind_after_preflight_overrides_early_exit_with_port_conflict() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(&temp.path, &python, port, true, "run-scoped", false);
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let runtime = spawn_run(&model, &root);
    let sentinel = wait_for_named(&root, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("runtime should finish preflight and enter prepare");
    let external = TcpListener::bind(("127.0.0.1", port))
        .expect("external harness should win the post-preflight bind race");
    fs::write(sentinel.with_file_name("endpoint-prepare-ack"), b"continue").unwrap();

    let output = wait_for_output(runtime, Duration::from_secs(10));
    let error = assert_port_conflict(&output, "listener-occupied", port);
    assert_eq!(error["details"]["failedService"], json!("synthetic"));
    assert!(error["details"]["runId"].is_string());
    assert_eq!(error["details"]["environment"], json!("dev"));
    assert_eq!(error["details"]["slot"], json!(0));
    assert!(
        error["details"]["portConflict"]
            .get("nixfiedOwner")
            .is_none()
    );
    drop(external);
}

#[test]
fn external_listener_fails_before_prepare_with_no_owner_attribution() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let external = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = external.local_addr().unwrap().port();
    let model = write_endpoint_model(&temp.path, &python, port, false, "run-scoped", false);
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let output = run_command(&model, &root).output().unwrap();
    let error = assert_port_conflict(&output, "listener-occupied", port);
    assert!(find_named(&root, "endpoint-prepare-sentinel").is_none());
    assert!(
        error["details"]["portConflict"]
            .get("nixfiedOwner")
            .is_none()
    );
}

#[test]
fn completed_persistent_owner_with_lost_listener_is_repaired_before_replace() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        true,
    );
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    assert_success(&first, "initial persistent owner");
    let first_json: Value = serde_json::from_slice(&first.stdout).unwrap();
    let first_process = first_json["services"][0]["processKey"]
        .as_str()
        .unwrap()
        .to_string();
    thread::sleep(Duration::from_millis(900));

    let second = run_command(&model, &root).output().unwrap();
    assert_success(&second, "persistent repair replacement");
    let second_json: Value = serde_json::from_slice(&second.stdout).unwrap();
    let second_process = second_json["services"][0]["processKey"].as_str().unwrap();
    assert_ne!(
        second_process, first_process,
        "missing exact ownership must terminate-before-replace, never borrow"
    );
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "replacement down",
    );
}

#[test]
fn active_borrower_blocks_persistent_repair_without_signaling_owner() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        true,
    );
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    assert_success(&first, "initial persistent owner");
    let (owner_pid, owner_process_key) = insert_active_borrower(&root, "run-active-borrower");
    thread::sleep(Duration::from_millis(900));

    let blocked = run_command(&model, &root).output().unwrap();
    assert_eq!(blocked.status.code(), Some(29), "LEASE_CONFLICT exit code");
    let error = stderr_json(&blocked.stderr);
    assert_eq!(error["code"], json!("LEASE_CONFLICT"));
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("unexpired or non-fenceable lease"),
        "unexpected lease conflict: {error:#}"
    );
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "repair refusal must not signal the tracked owner"
    );

    let registry = find_named(&root, "registry.sqlite3").expect("registry should exist");
    let connection = rusqlite::Connection::open(registry).expect("registry should open");
    let state: (String, String, String) = connection
        .query_row(
            "
            SELECT s.status, p.status, l.status
            FROM services s
            JOIN processes p ON p.service_instance_id = s.service_instance_id
            JOIN run_leases l ON l.service_instance_id = s.service_instance_id
            WHERE p.process_key = ?1 AND l.run_id = 'run-active-borrower'
            ",
            [&owner_process_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("blocked repair evidence should remain unchanged");
    assert_eq!(state, ("borrowed".into(), "ready".into(), "active".into()));
    connection
        .execute(
            "UPDATE run_leases SET status = 'completed' WHERE run_id = 'run-active-borrower'",
            [],
        )
        .expect("test borrower should release");
    connection
        .execute(
            "UPDATE runs SET status = 'completed' WHERE run_id = 'run-active-borrower'",
            [],
        )
        .expect("test borrower run should complete");
    drop(connection);
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "persistent owner down",
    );
}

#[test]
fn crash_after_repair_claim_recovers_exact_starting_service_for_borrow() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        false,
    );
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    assert_success(&first, "initial persistent owner");
    let first_json: Value = serde_json::from_slice(&first.stdout).unwrap();
    let first_process = first_json["services"][0]["processKey"]
        .as_str()
        .unwrap()
        .to_string();
    force_post_claim_starting(&root);

    let second = run_command(&model, &root).output().unwrap();
    assert_success(&second, "exact Starting recovery");
    let second_json: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(
        second_json["services"][0]["processKey"],
        json!(first_process),
        "an exact live Starting row should recover by guarded borrow"
    );
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "recovered owner down",
    );
}

#[test]
fn crash_after_repair_claim_with_missing_listener_terminates_before_replace() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port();
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        true,
    );
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    assert_success(&first, "initial persistent owner");
    let first_json: Value = serde_json::from_slice(&first.stdout).unwrap();
    let first_process = first_json["services"][0]["processKey"]
        .as_str()
        .unwrap()
        .to_string();
    thread::sleep(Duration::from_millis(900));
    force_post_claim_starting(&root);

    let second = run_command(&model, &root).output().unwrap();
    assert_success(&second, "broken Starting replacement");
    let second_json: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(
        second_json["services"][0]["processKey"],
        json!(first_process),
        "a live Starting row missing its listener must terminate before replacement"
    );
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "replacement down",
    );
}

#[test]
fn missing_second_endpoint_never_commits_partial_ready_and_releases_locks() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_two_port_window();
    let model = write_multi_endpoint_model(&temp.path, &python, port);
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    let first_error = stderr_json(&first.stderr);
    assert_eq!(
        first.status.code(),
        Some(26),
        "expected READINESS_TIMEOUT, got {first_error:#}"
    );
    assert_eq!(first_error["code"], json!("READINESS_TIMEOUT"));
    let registry_path = PathBuf::from(
        first_error["details"]["registryPath"]
            .as_str()
            .expect("runtime error should link the registry"),
    );
    let connection = rusqlite::Connection::open(registry_path).unwrap();
    let state: (i64, i64, i64) = connection
        .query_row(
            "
            SELECT
              (SELECT count(*) FROM ports WHERE status = 'active'),
              (SELECT count(*) FROM ports WHERE status = 'released'),
              (SELECT count(*) FROM events WHERE event_type = 'service.probe-ready')
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, (0, 2, 0));
    drop(connection);

    // A second full attempt reaches the same truthful readiness result rather
    // than startup-lock contention, proving all guards left the failed start.
    let second = run_command(&model, &root).output().unwrap();
    let second_error = stderr_json(&second.stderr);
    assert_eq!(
        second.status.code(),
        Some(26),
        "expected repeat READINESS_TIMEOUT, got {second_error:#}"
    );
    assert_eq!(second_error["code"], json!("READINESS_TIMEOUT"));
}

fn write_endpoint_model(
    directory: &Path,
    python: &Path,
    port: u16,
    blocking_prepare: bool,
    service_lifetime: &str,
    close_listener: bool,
) -> PathBuf {
    let closure_root = closure_root_for_store_executable(python)
        .expect("store executable should have a closure root");
    let mut value = synthetic_model(
        &python.to_string_lossy(),
        &["-c", HARNESS, "service", "${port}"],
        port,
        port,
    );
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["operationBindings"] = json!([
        "service.synthetic.start",
        "task.endpoint-prepare.run",
        "task.smoke.run"
    ]);
    value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({ "task": "endpoint-prepare" });
    value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"] = json!([
        python.file_name().unwrap().to_string_lossy(),
        "-c",
        HARNESS,
        "service",
        "${port}",
        if close_listener { "close" } else { "hold" }
    ]);
    value["tasks"]["smoke"]["serviceLifetime"] = json!(service_lifetime);
    value["tasks"]["smoke"]["invocation"]["run"] = json!([
        python.file_name().unwrap().to_string_lossy(),
        "-c",
        HARNESS,
        "task",
        "${port}"
    ]);
    let mut prepare = value["tasks"]["smoke"].clone();
    prepare["serviceLifetime"] = json!("run-scoped");
    prepare["operationId"] = json!("task.endpoint-prepare.run");
    prepare["requires"] = json!([]);
    prepare["servicesRequired"] = json!([]);
    prepare["logRefs"] = json!(["task.endpoint-prepare"]);
    prepare["invocation"]["run"] = json!([
        python.file_name().unwrap().to_string_lossy(),
        "-c",
        HARNESS,
        "prepare",
        "${stateDir}/endpoint-prepare-sentinel",
        "${stateDir}/endpoint-prepare-ack",
        if blocking_prepare { "block" } else { "pass" }
    ]);
    value["tasks"]["endpoint-prepare"] = prepare;

    let model: Model = serde_json::from_value(value).expect("endpoint model should parse");
    model.validate().expect("endpoint model should validate");
    let path = directory.join("endpoint-model.json");
    fs::write(&path, serde_json::to_vec_pretty(&model).unwrap()).unwrap();
    path
}

fn write_multi_endpoint_model(directory: &Path, python: &Path, port: u16) -> PathBuf {
    let path = write_endpoint_model(directory, python, port, false, "run-scoped", false);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
    value["services"]["synthetic"]["endpoints"]["admin"] = json!({
        "endpointId": "admin",
        "host": "127.0.0.1"
    });
    let model: Model = serde_json::from_value(value).expect("multi-endpoint model should parse");
    model
        .validate()
        .expect("two endpoints should fit the two-port window");
    let path = directory.join("multi-endpoint-model.json");
    fs::write(&path, serde_json::to_vec_pretty(&model).unwrap()).unwrap();
    path
}

fn run_command(model: &Path, state_root: &Path) -> Command {
    let mut command = Command::new(runtime_binary());
    command
        .arg("run")
        .arg("--task")
        .arg("smoke")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(model)
        .arg("--timeout-ms")
        .arg("20000")
        .arg("--json")
        .env("NIXFIED_STATE_DIR", state_root)
        .current_dir(model.parent().unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn down_command(model: &Path, state_root: &Path) -> Command {
    let mut command = Command::new(runtime_binary());
    command
        .arg("down")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(model)
        .env("NIXFIED_STATE_DIR", state_root)
        .current_dir(model.parent().unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn spawn_run(model: &Path, state_root: &Path) -> Child {
    run_command(model, state_root)
        .spawn()
        .expect("runtime should spawn")
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_port_conflict(output: &Output, reason: &str, port: u16) -> Value {
    assert_eq!(output.status.code(), Some(23), "PORT_CONFLICT exit code");
    assert!(output.stdout.is_empty());
    let error = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("PORT_CONFLICT"));
    assert_eq!(error["details"]["portConflict"]["reason"], json!(reason));
    assert_eq!(
        error["details"]["portConflict"]["projectId"],
        json!("runtime-test")
    );
    assert_eq!(
        error["details"]["portConflict"]["endpoint"],
        json!({
            "transport": "tcp",
            "family": "ipv4",
            "address": "127.0.0.1",
            "port": port,
            "endpointId": "synthetic-tcp"
        })
    );
    error
}

fn available_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn available_two_port_window() -> u16 {
    loop {
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = first.local_addr().unwrap().port();
        let Some(second_port) = port.checked_add(1) else {
            continue;
        };
        if let Ok(second) = TcpListener::bind(("127.0.0.1", second_port)) {
            drop(second);
            drop(first);
            return port;
        }
    }
}

fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_named(&path, name)
        {
            return Some(found);
        }
    }
    None
}

fn force_post_claim_starting(root: &Path) {
    let registry = find_named(root, "registry.sqlite3").expect("registry should exist");
    let connection = rusqlite::Connection::open(registry).expect("registry should open");
    assert_eq!(
        connection
            .execute("UPDATE services SET status = 'starting'", [])
            .expect("service should enter simulated repair claim"),
        1
    );
    assert!(
        connection
            .execute(
                "UPDATE run_leases SET status = 'stale' WHERE status IN ('active', 'canceling')",
                [],
            )
            .expect("claim should fence every open owner token")
            >= 1
    );
}

fn insert_active_borrower(root: &Path, borrower_run_id: &str) -> (libc::pid_t, String) {
    let registry = find_named(root, "registry.sqlite3").expect("registry should exist");
    let connection = rusqlite::Connection::open(registry).expect("registry should open");
    let (owner_run_id, owner_pid, owner_process_key): (String, libc::pid_t, String) = connection
        .query_row(
            "SELECT run_id, pid, process_key FROM processes WHERE status = 'ready'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("persistent owner process should exist");
    assert_eq!(
        connection
            .execute(
                "
                INSERT INTO runs (
                  run_id, environment, slot, status, model_path, computed_model_hash,
                  runtime_abi, toolchain_id, generator_json, target_json, source_json,
                  summary_path
                )
                SELECT ?1, environment, slot, 'service-starting', model_path,
                       computed_model_hash, runtime_abi, toolchain_id, generator_json,
                       target_json, source_json, summary_path
                FROM runs WHERE run_id = ?2
                ",
                rusqlite::params![borrower_run_id, owner_run_id],
            )
            .expect("borrower run should be inserted"),
        1
    );
    assert_eq!(
        connection
            .execute(
                "
                INSERT INTO run_leases (
                  run_id, environment, slot, service_instance_id, owner_token,
                  heartbeat_at, expires_at, status
                )
                SELECT ?1, environment, slot, service_instance_id, ?2,
                       strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                       strftime('%Y-%m-%dT%H:%M:%fZ','now','+60 seconds'),
                       'active'
                FROM run_leases WHERE run_id = ?3
                ",
                rusqlite::params![
                    borrower_run_id,
                    format!("owner-{borrower_run_id}"),
                    owner_run_id
                ],
            )
            .expect("borrower lease should be inserted"),
        1
    );
    (owner_pid, owner_process_key)
}

fn wait_for_named(root: &Path, name: &str, timeout: Duration) -> Option<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(path) = find_named(root, name) {
            return Some(path);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_output(mut child: Child, timeout: Duration) -> Output {
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .expect("child status should query")
            .is_some()
        {
            return child
                .wait_with_output()
                .expect("child output should collect");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .expect("timed-out output should collect");
            panic!(
                "runtime timed out\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}
