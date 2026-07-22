use std::fs;
use std::io;
use std::net::TcpListener;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

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
            if sys.argv[3] == "active-close":
                connection.shutdown(socket.SHUT_WR)
            else:
                connection.recv(1)
            connection.close()
elif command == "task":
    connection = socket.create_connection(("127.0.0.1", int(sys.argv[2])), timeout=5)
    if sys.argv[3] == "active-close":
        assert connection.recv(1) == b""
    connection.close()
"#;

#[test]
fn persistent_listener_blocks_an_independent_root_before_prepare_then_releases() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        "hold",
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
    let port = available_port_window(1);
    let model = write_endpoint_model(&temp.path, &python, port, true, "run-scoped", "hold");
    let root_a = temp.path.join("root-a");
    let root_b = temp.path.join("root-b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    let winner = spawn_run(&model, &root_a);
    let sentinel = wait_for_named(&root_a, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("root A should enter prepare while retaining the lock");
    let loser = spawn_run(&model, &root_b);
    let loser_output = wait_for_child_output(loser, Duration::from_secs(5));
    assert_port_conflict(&loser_output, "startup-lock-contended", port);
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_none());

    fs::write(sentinel.with_file_name("endpoint-prepare-ack"), b"continue").unwrap();
    let winner_output = wait_for_child_output(winner, Duration::from_secs(20));
    assert_success(&winner_output, "lock winner");
}

#[test]
fn killing_runtime_during_prepare_releases_lock_not_inherited_by_child() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(&temp.path, &python, port, true, "run-scoped", "hold");
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
    let killed = wait_for_child_output(runtime, Duration::from_secs(5));
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
        &wait_for_child_output(successor, Duration::from_secs(20)),
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
    let port = available_port_window(1);
    let model = write_endpoint_model(&temp.path, &python, port, true, "run-scoped", "hold");
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let runtime = spawn_run(&model, &root);
    let sentinel = wait_for_named(&root, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("runtime should finish preflight and enter prepare");
    let external = TcpListener::bind(("127.0.0.1", port))
        .expect("external harness should win the post-preflight bind race");
    fs::write(sentinel.with_file_name("endpoint-prepare-ack"), b"continue").unwrap();

    let output = wait_for_child_output(runtime, Duration::from_secs(10));
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
fn external_exact_and_wildcard_listeners_fail_before_prepare() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    for address in ["127.0.0.1", "0.0.0.0"] {
        let temp = TempDir::new();
        let external = TcpListener::bind((address, 0)).unwrap();
        enable_address_reuse(&external);
        let port = external.local_addr().unwrap().port();
        let model = write_endpoint_model(&temp.path, &python, port, false, "run-scoped", "hold");
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
}

#[test]
fn immediate_lifecycle_repeat_ignores_server_side_time_wait() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "run-scoped",
        "active-close",
    );
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    assert_success(&first, "first active-close lifecycle");
    assert_nonreusable_bind_is_occupied(port);

    let second = run_command(&model, &root).output().unwrap();
    assert_success(&second, "immediate lifecycle repeat");
}

#[test]
fn lost_listener_is_preserved_until_explicit_down_then_retry_succeeds() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        "close",
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
    let owner_pid = process_pid(&root, &first_process);
    thread::sleep(Duration::from_millis(900));

    let lease_blocked = run_command(&model, &root).output().unwrap();
    assert_error_code(&lease_blocked, "LEASE_CONFLICT", 29);
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "an open owner lease must block without signaling the live process"
    );
    mark_open_leases_stale(&root);

    let ownership_blocked = run_command(&model, &root).output().unwrap();
    let error = assert_error_code(&ownership_blocked, "PORT_UNVERIFIABLE", 24);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("missing its expected listener"),
        "unexpected missing-listener error: {error:#}"
    );
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "missing ownership must not trigger a replacement signal"
    );

    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "preserved owner down",
    );
    let retry = run_command(&model, &root).output().unwrap();
    assert_success(&retry, "retry after explicit down");
    let retry_json: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_ne!(
        retry_json["services"][0]["processKey"],
        json!(first_process),
        "retry after down must start a new process"
    );
    assert_success(&down_command(&model, &root).output().unwrap(), "retry down");
}

#[test]
fn active_borrower_blocks_nonreusable_service_without_signaling_owner() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        "close",
    );
    let root = temp.path.join("root");
    fs::create_dir_all(&root).unwrap();

    let first = run_command(&model, &root).output().unwrap();
    assert_success(&first, "initial persistent owner");
    let (owner_pid, owner_process_key) = insert_active_borrower(&root, "run-active-borrower");
    thread::sleep(Duration::from_millis(900));

    let blocked = run_command(&model, &root).output().unwrap();
    let error = assert_error_code(&blocked, "LEASE_CONFLICT", 29);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("authoritative open lease"),
        "unexpected lease conflict: {error:#}"
    );
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "replacement refusal must not signal the tracked owner"
    );

    let registry = find_named(&root, "registry.sqlite3").expect("registry should exist");
    let connection = rusqlite::Connection::open(registry).expect("registry should open");
    let state: (String, String) = connection
        .query_row(
            "
            SELECT p.status, l.status
            FROM services s
            JOIN processes p ON p.service_instance_id = s.service_instance_id
            JOIN run_leases l ON l.service_instance_id = s.service_instance_id
            WHERE p.process_key = ?1 AND l.run_id = 'run-active-borrower'
            ",
            [&owner_process_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("blocked ownership evidence should remain unchanged");
    assert_eq!(state, ("ready".into(), "active".into()));
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
fn live_starting_service_is_not_promoted_or_borrowed() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        "hold",
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
    let owner_pid = process_pid(&root, &first_process);
    force_live_starting_process_without_open_lease(&root);

    let second = run_command(&model, &root).output().unwrap();
    assert_error_code(&second, "PORT_UNVERIFIABLE", 24);
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "a live Starting process must be preserved"
    );
    let registry = find_named(&root, "registry.sqlite3").expect("registry should exist");
    let connection = rusqlite::Connection::open(registry).expect("registry should open");
    let evidence: (String, String) = connection
        .query_row(
            "
            SELECT p.status, o.status
            FROM services s
            JOIN processes p ON p.service_instance_id = s.service_instance_id
            JOIN ports o ON o.owner_process_key = p.process_key
            WHERE p.process_key = ?1
            ",
            [&first_process],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Starting evidence should remain unchanged");
    assert_eq!(evidence, ("starting".into(), "active".into()));
    drop(connection);
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "Starting owner down",
    );
}

#[test]
fn outside_listener_preserves_recorded_process_and_reports_conflict() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(1);
    let model = write_endpoint_model(
        &temp.path,
        &python,
        port,
        false,
        "persistent-until-down",
        "close",
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
    let owner_pid = process_pid(&root, &first_process);
    thread::sleep(Duration::from_millis(900));
    let external = TcpListener::bind(("127.0.0.1", port))
        .expect("external listener should replace the service socket");
    mark_open_leases_stale(&root);

    let blocked = run_command(&model, &root).output().unwrap();
    let error = assert_port_conflict(&blocked, "listener-occupied", port);
    assert!(
        error["details"]["portConflict"]
            .get("nixfiedOwner")
            .is_none(),
        "an external listener must not be attributed to the recorded service"
    );
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "wrong listener ownership must not signal the recorded process"
    );
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "wrong-owner service down",
    );
    drop(external);
    let retry = run_command(&model, &root).output().unwrap();
    assert_success(&retry, "retry after wrong-owner down");
    assert_success(
        &down_command(&model, &root).output().unwrap(),
        "wrong-owner retry down",
    );
}

#[test]
fn missing_second_endpoint_never_commits_partial_ready_and_releases_locks() {
    let _serial = ENDPOINT_TESTS.lock().unwrap();
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let temp = TempDir::new();
    let port = available_port_window(2);
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
    listener_behavior: &str,
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
        listener_behavior
    ]);
    value["tasks"]["smoke"]["serviceLifetime"] = json!(service_lifetime);
    value["tasks"]["smoke"]["invocation"]["run"] = json!([
        python.file_name().unwrap().to_string_lossy(),
        "-c",
        HARNESS,
        "task",
        "${port}",
        listener_behavior
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
    let path = write_endpoint_model(directory, python, port, false, "run-scoped", "hold");
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

fn enable_address_reuse(listener: &TcpListener) {
    let enabled: libc::c_int = 1;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                listener.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                std::ptr::from_ref(&enabled).cast(),
                std::mem::size_of_val(&enabled) as libc::socklen_t,
            )
        },
        0,
        "test listener should enable address reuse: {}",
        io::Error::last_os_error()
    );
}

fn assert_nonreusable_bind_is_occupied(port: u16) {
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, libc::IPPROTO_TCP) };
    assert!(
        raw >= 0,
        "test socket should open: {}",
        io::Error::last_os_error()
    );
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    #[cfg(target_os = "macos")]
    {
        address.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
    }
    address.sin_family = libc::AF_INET as libc::sa_family_t;
    address.sin_port = port.to_be();
    address.sin_addr.s_addr = u32::from_ne_bytes([127, 0, 0, 1]);
    let result = unsafe {
        libc::bind(
            socket.as_raw_fd(),
            std::ptr::from_ref(&address).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    assert_ne!(result, 0, "non-reuse bind unexpectedly succeeded");
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::EADDRINUSE),
        "non-reuse bind should be blocked by server-side TIME_WAIT"
    );
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

fn force_live_starting_process_without_open_lease(root: &Path) {
    let registry = find_named(root, "registry.sqlite3").expect("registry should exist");
    let connection = rusqlite::Connection::open(registry).expect("registry should open");
    assert_eq!(
        connection
            .execute(
                "UPDATE processes SET status = 'starting' WHERE service_instance_id IS NOT NULL AND status = 'ready'",
                [],
            )
            .expect("process should enter simulated Starting state"),
        1
    );
    assert!(
        connection
            .execute(
                "UPDATE run_leases SET status = 'stale' WHERE status IN ('active', 'canceling')",
                [],
            )
            .expect("test should close every owner token")
            >= 1
    );
}

fn process_pid(root: &Path, process_key: &str) -> libc::pid_t {
    let registry = find_named(root, "registry.sqlite3").expect("registry should exist");
    rusqlite::Connection::open(registry)
        .expect("registry should open")
        .query_row(
            "SELECT pid FROM processes WHERE process_key = ?1",
            [process_key],
            |row| row.get(0),
        )
        .expect("process pid should query")
}

fn mark_open_leases_stale(root: &Path) {
    let registry = find_named(root, "registry.sqlite3").expect("registry should exist");
    rusqlite::Connection::open(registry)
        .expect("registry should open")
        .execute(
            "UPDATE run_leases SET status = 'stale' WHERE status IN ('active', 'canceling')",
            [],
        )
        .expect("test should close open leases");
}

fn assert_error_code(output: &Output, code: &str, exit_code: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit_code), "{code} exit code");
    assert!(output.stdout.is_empty());
    let error = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!(code));
    error
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
