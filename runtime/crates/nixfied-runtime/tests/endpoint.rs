use std::fs;
use std::io;
use std::net::TcpListener;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value, json};

mod common;
use common::*;

/// Endpoint scenarios race real host ports; run them one at a time.
fn serial() -> MutexGuard<'static, ()> {
    static ENDPOINT_TESTS: Mutex<()> = Mutex::new(());
    ENDPOINT_TESTS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[test]
fn session_listener_blocks_an_independent_root_before_prepare_then_releases() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, false, true, "hold");
    let root_a = fixture.tmp.path.join("root-a");
    let root_b = fixture.tmp.path.join("root-b");
    let first = HeldSession::start(&fixture, &root_a);
    assert!(find_named(&root_a, "endpoint-prepare-sentinel").is_some());
    let blocked = run_command(&fixture, &root_b).output().unwrap();
    let error = assert_port_conflict(&blocked, "listener-occupied", port);
    assert!(
        error["details"]["portConflict"]
            .get("nixfiedOwner")
            .is_none()
    );
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_none());
    first.finish();
    let second = HeldSession::start(&fixture, &root_b);
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_some());
    second.finish();
    // Run-scoped data ends with its session; the application tree is gone.
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_none());
}

#[test]
fn concurrent_roots_have_one_prepare_winner_and_one_lock_loser() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, true, false, "hold");
    let root_a = fixture.tmp.path.join("root-a");
    let root_b = fixture.tmp.path.join("root-b");

    let winner = spawn_run(&fixture, &root_a);
    let sentinel = wait_for_named(&root_a, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("root A should enter prepare while retaining the lock");
    let loser = spawn_run(&fixture, &root_b);
    let loser_output = wait_for_child_output(loser, Duration::from_secs(5));
    assert_port_conflict(&loser_output, "startup-lock-contended", port);
    assert!(find_named(&root_b, "endpoint-prepare-sentinel").is_none());

    fs::write(sentinel.with_file_name("endpoint-prepare-ack"), b"continue").unwrap();
    let winner_output = wait_for_child_output(winner, Duration::from_secs(20));
    assert_success(&winner_output);
}

#[test]
fn killing_runtime_during_prepare_releases_lock_not_inherited_by_child() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, true, false, "hold");
    let root_a = fixture.tmp.path.join("root-a");
    let root_b = fixture.tmp.path.join("root-b");

    let runtime = spawn_run(&fixture, &root_a);
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

    let successor = spawn_run(&fixture, &root_b);
    let sentinel_b = wait_for_named(&root_b, "endpoint-prepare-sentinel", Duration::from_secs(5))
        .expect("successor should acquire the released kernel lock");
    fs::write(
        sentinel_b.with_file_name("endpoint-prepare-ack"),
        b"continue",
    )
    .unwrap();
    assert_success(&wait_for_child_output(successor, Duration::from_secs(20)));
}

#[test]
fn external_bind_after_preflight_overrides_early_exit_with_port_conflict() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, true, false, "hold");
    let root = fixture.tmp.path.join("root");

    let runtime = spawn_run(&fixture, &root);
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
    let _serial = serial();
    for address in ["127.0.0.1", "0.0.0.0"] {
        let external = TcpListener::bind((address, 0)).unwrap();
        enable_address_reuse(&external);
        let port = external.local_addr().unwrap().port();
        let fixture = endpoint_fixture(port, false, false, "hold");
        let root = fixture.tmp.path.join("root");

        let output = run_command(&fixture, &root).output().unwrap();
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
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, false, false, "active-close");
    let root = fixture.tmp.path.join("root");

    let first = run_command(&fixture, &root).output().unwrap();
    assert_success(&first);
    assert_nonreusable_bind_is_occupied(port);

    let second = run_command(&fixture, &root).output().unwrap();
    assert_success(&second);
}

#[test]
fn lost_listener_does_not_grant_a_second_session_execution_rights() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, false, true, "close");
    let root = fixture.tmp.path.join("root");
    let first = HeldSession::start(&fixture, &root);
    let (pid, first_key) = service_process(&root);
    let blocked = run_command(&fixture, &root).output().unwrap();
    assert_error_code(&blocked, "CLEANUP_REFUSED", 22);
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        0,
        "refusal must not signal the owner"
    );
    first.finish();
    let second = HeldSession::start(&fixture, &root);
    assert_ne!(service_process(&root).1, first_key);
    second.finish();
}

#[test]
fn active_session_blocks_replacement_without_mutating_owner_evidence() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, false, true, "hold");
    let root = fixture.tmp.path.join("root");
    let owner = HeldSession::start(&fixture, &root);
    let (pid, key) = service_process(&root);
    let before = service_evidence(&root, &key);
    assert_error_code(
        &run_command(&fixture, &root).output().unwrap(),
        "CLEANUP_REFUSED",
        22,
    );
    assert_eq!(service_evidence(&root, &key), before);
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    owner.finish();
}

#[test]
fn interrupted_starting_service_is_cleaned_before_fresh_start_not_adopted() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, false, true, "hold");
    let root = fixture.tmp.path.join("root");
    let mut predecessor = HeldSession::start(&fixture, &root);
    let (_, old_key) = service_process(&root);
    predecessor.crash();
    rusqlite::Connection::open(find_named(&root, "registry.sqlite3").unwrap())
        .unwrap()
        .execute(
            "UPDATE processes SET status = 'starting' WHERE process_key = ?1",
            [&old_key],
        )
        .unwrap();
    let successor = HeldSession::start(&fixture, &root);
    assert_ne!(service_process(&root).1, old_key);
    assert_eq!(
        service_evidence(&root, &old_key),
        ("stopped".into(), "released".into())
    );
    successor.finish();
}

#[test]
fn outside_listener_survives_predecessor_recovery_and_reports_conflict() {
    let _serial = serial();
    let port = available_port_window(1);
    let fixture = endpoint_fixture(port, false, true, "close");
    let root = fixture.tmp.path.join("root");
    let mut predecessor = HeldSession::start(&fixture, &root);
    let (_, old_key) = service_process(&root);
    let external = TcpListener::bind(("127.0.0.1", port)).unwrap();
    predecessor.crash();
    let blocked = run_command(&fixture, &root).output().unwrap();
    let error = assert_port_conflict(&blocked, "listener-occupied", port);
    assert!(
        error["details"]["portConflict"]
            .get("nixfiedOwner")
            .is_none()
    );
    assert_eq!(
        service_evidence(&root, &old_key),
        ("stopped".into(), "released".into())
    );
    assert_eq!(external.local_addr().unwrap().port(), port);
    drop(external);
    HeldSession::start(&fixture, &root).finish();
}

#[test]
fn missing_second_endpoint_never_commits_partial_ready_and_releases_locks() {
    let _serial = serial();
    let port = available_port_window(2);
    let mut manifest = endpoint_manifest(port, false, false, "hold");
    manifest["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
    manifest["services"]["synthetic"]["endpoints"]["admin"] = json!({
        "endpointId": "admin",
        "host": "127.0.0.1"
    });
    let fixture = RuntimeFixture::new(manifest);
    let root = fixture.tmp.path.join("root");

    let first = run_command(&fixture, &root).output().unwrap();
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
    let second = run_command(&fixture, &root).output().unwrap();
    let second_error = stderr_json(&second.stderr);
    assert_eq!(
        second.status.code(),
        Some(26),
        "expected repeat READINESS_TIMEOUT, got {second_error:#}"
    );
    assert_eq!(second_error["code"], json!("READINESS_TIMEOUT"));
}

/// The same manifest shared by independent state roots under one fixture.
fn endpoint_fixture(
    port: u16,
    blocking_prepare: bool,
    hold_session: bool,
    listener_behavior: &str,
) -> RuntimeFixture {
    RuntimeFixture::new(endpoint_manifest(
        port,
        blocking_prepare,
        hold_session,
        listener_behavior,
    ))
}

fn endpoint_manifest(
    port: u16,
    blocking_prepare: bool,
    hold_session: bool,
    listener_behavior: &str,
) -> Value {
    let (start_args, task_args): (&[&str], &[&str]) = match listener_behavior {
        "hold" => (LISTEN_HOLD, &["connect", "127.0.0.1", "${port}", "close"]),
        "active-close" => (
            &["listen", "127.0.0.1", "${port}", "active-close"],
            &["connect", "127.0.0.1", "${port}", "wait-eof"],
        ),
        "close" => (
            &[
                "listen",
                "127.0.0.1",
                "${port}",
                "close-on-marker",
                "${stateDir}/endpoint-listener-close",
                "${stateDir}/endpoint-listener-closed",
            ],
            &[
                "connect",
                "127.0.0.1",
                "${port}",
                "close-and-signal",
                "${stateDir}/endpoint-listener-close",
                "${stateDir}/endpoint-listener-closed",
            ],
        ),
        behavior => panic!("unsupported endpoint listener behavior {behavior:?}"),
    };
    let mut value = test_child_service(start_args, port, port);
    set_task_run_args(&mut value, task_args);
    let sentinel = "${stateDir}/endpoint-prepare-sentinel";
    if blocking_prepare {
        add_endpoint_prepare(
            &mut value,
            &["prepare", sentinel, "${stateDir}/endpoint-prepare-ack"],
        );
    } else {
        add_endpoint_prepare(&mut value, &["prepare", sentinel]);
    }
    if hold_session {
        add_task_clone(&mut value, "endpoint-client", &["synthetic"], task_args);
        add_task_clone(
            &mut value,
            "endpoint-wait",
            &["synthetic"],
            &[
                "prepare",
                "${stateDir}/endpoint-session-active",
                "${stateDir}/endpoint-session-ack",
            ],
        );
        clear_task_deadline(&mut value, "endpoint-wait");
        value["tasks"]["smoke"] = json!({
            "kind": "composite",
            "steps": {"client": {"task": "endpoint-client"},
                      "wait": {"task": "endpoint-wait", "dependsOn": ["client"]}}
        });
    }
    value
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

fn run_command(fixture: &RuntimeFixture, state_root: &Path) -> Command {
    fixture.command_at(
        state_root,
        "run",
        &[
            "--task",
            "smoke",
            "--timeout-ms",
            "20000",
            "--output",
            "json",
        ],
    )
}

fn spawn_run(fixture: &RuntimeFixture, state_root: &Path) -> Child {
    run_command(fixture, state_root)
        .spawn()
        .expect("runtime should spawn")
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

fn assert_error_code(output: &Output, code: &str, exit_code: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit_code), "{code} exit code");
    assert!(output.stdout.is_empty());
    let error = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!(code));
    error
}

/// Keep the session (and therefore its dependencies) alive until the test
/// explicitly completes or interrupts it. Unwinding still cleans owned children.
struct HeldSession {
    runtime: Option<Child>,
    down: Command,
    data: PathBuf,
}

impl HeldSession {
    fn start(fixture: &RuntimeFixture, root: &Path) -> Self {
        let data = root.join("data/runtime-test/dev/0");
        for marker in [
            "endpoint-session-active",
            "endpoint-session-ack",
            "endpoint-listener-close",
            "endpoint-listener-closed",
        ] {
            match fs::remove_file(data.join(marker)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => panic!("remove prior test marker: {error}"),
            }
        }
        let mut session = Self {
            runtime: Some(spawn_run(fixture, root)),
            down: fixture.command_at(root, "down", &[]),
            data,
        };
        if !wait_for_path(
            &session.data.join("endpoint-session-active"),
            Duration::from_secs(5),
        ) {
            let runtime = session.runtime.as_mut().unwrap();
            if runtime.try_wait().unwrap().is_some() {
                let output = session.runtime.take().unwrap().wait_with_output().unwrap();
                panic!(
                    "session did not reach held task: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            panic!("session never reached held task");
        }
        // The application marker can precede process registration until gated
        // spawn is implemented. These recovery scenarios deliberately interrupt
        // after durable registration, not in that still-unclosed interval.
        poll_until(Duration::from_secs(5), "held task registration", || {
            let registered: i64 = registry_ro(root)
                .query_row(
                    "SELECT count(*) FROM processes WHERE service_instance_id IS NULL AND status = 'running'",
                    [], |row| row.get(0),
                ).unwrap();
            (registered == 1).then_some(())
        });
        session
    }

    fn finish(mut self) {
        fs::write(self.data.join("endpoint-session-ack"), b"finish").unwrap();
        let output = wait_for_child_output(self.runtime.take().unwrap(), Duration::from_secs(20));
        assert_success(&output);
    }

    fn crash(&mut self) {
        let runtime = self.runtime.take().unwrap();
        assert_eq!(
            unsafe { libc::kill(runtime.id() as libc::pid_t, libc::SIGKILL) },
            0
        );
        let output = wait_for_child_output(runtime, Duration::from_secs(5));
        assert!(!output.status.success());
    }
}

impl Drop for HeldSession {
    fn drop(&mut self) {
        if let Some(mut runtime) = self.runtime.take() {
            unsafe {
                libc::kill(runtime.id() as libc::pid_t, libc::SIGTERM);
            }
            if poll(Duration::from_secs(10), || {
                runtime.try_wait().ok().flatten()
            })
            .is_none()
            {
                let _ = runtime.kill();
            }
            let _ = runtime.wait();
        }
        // A crashed runtime can leave recorded children. This dedicated fixture
        // root is ours; control must reacquire its slot before signaling them.
        let _ = self.down.output();
    }
}

fn service_process(root: &Path) -> (libc::pid_t, String) {
    registry_ro(root).query_row(
        "SELECT pid, process_key FROM processes WHERE service_instance_id IS NOT NULL AND status = 'ready'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap()
}

fn service_evidence(root: &Path, process: &str) -> (String, String) {
    registry_ro(root).query_row(
        "SELECT p.status, ep.status FROM processes p JOIN ports ep ON ep.owner_process_key = p.process_key
         WHERE p.process_key = ?1", [process], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap()
}
