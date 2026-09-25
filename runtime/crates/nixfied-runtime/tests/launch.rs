//! Independent raw-wire proofs for the sterile runtime workload gate.
mod common;
use common::*;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn gate() -> (Child, UnixStream) {
    let (child, channel, _) = gate_with_descriptor();
    (child, channel)
}

fn gate_with_descriptor() -> (Child, UnixStream, i32) {
    let (owner, child_socket) = UnixStream::pair().unwrap();
    let fd = child_socket.as_raw_fd();
    let writer = owner.as_raw_fd();
    let mut command = Command::new(runtime_binary());
    command
        .arg("__workload-gate")
        .arg(fd.to_string())
        .env_clear()
        .current_dir("/")
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: only descriptor and signal syscalls run after fork. Never make the parent's
    // descriptor inheritable; clear CLOEXEC only in this child's private table.
    unsafe {
        command.pre_exec(move || {
            if libc::close(writer) != 0 || libc::fcntl(fd, libc::F_SETFD, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Simulate inherited shell dispositions/masks. The gate must be
            // cancelable before permission, and the target must not inherit them.
            let mut mask = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, libc::SIGTERM);
            if libc::sigprocmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().unwrap();
    drop(child_socket);
    (child, owner, fd)
}

fn request(root: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "executable": std::env::var("NIXFIED_TEST_SHELL").unwrap().as_bytes(),
        "args": [b"-c".as_slice(), b"printf ran > marker".as_slice()],
        "env": [],
        "cwd": root.as_os_str().as_bytes(),
    })
}
fn frame(value: &serde_json::Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(value).unwrap();
    let mut frame = b"NXG1".to_vec();
    frame.extend((bytes.len() as u32).to_be_bytes());
    frame.extend(bytes);
    frame
}
fn failure(child: Child, mut channel: UnixStream, expected: u8) {
    channel.shutdown(Shutdown::Write).unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(15));
    assert_eq!(output.status.code(), Some(125));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let mut diagnostic = Vec::new();
    if let Err(error) = channel.read_to_end(&mut diagnostic) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    assert_eq!(diagnostic, [expected]);
}

#[test]
fn incomplete_malformed_and_extra_requests_never_execute() {
    let root = TempDir::new();
    let valid = frame(&request(&root.path));
    let mut extra = valid.clone();
    extra.push(0);
    let mut oversized = b"NXG1".to_vec();
    oversized.extend((1024_u32 * 1024 + 1).to_be_bytes());
    for bytes in [
        Vec::new(),
        valid[..4].to_vec(),
        valid[..valid.len() - 1].to_vec(),
        b"BAD!\0\0\0\x01x".to_vec(),
        oversized,
        extra,
    ] {
        let (child, mut channel) = gate();
        channel.write_all(&bytes).unwrap();
        failure(child, channel, 1);
        assert!(!root.path.join("marker").exists());
    }
}

#[test]
fn invalid_request_values_reject_without_exposing_them() {
    let root = TempDir::new();
    for alteration in 0..7 {
        let mut value = request(&root.path);
        match alteration {
            0 => value["executable"] = serde_json::json!(b"relative-secret"),
            1 => value["cwd"] = serde_json::json!(b"relative-secret"),
            2 => value["args"] = serde_json::json!([b"nul\0secret"]),
            3 => {
                value["env"] = serde_json::json!([
                    [b"A".as_slice(), b"secret".as_slice()],
                    [b"A".as_slice(), b"duplicate".as_slice()]
                ])
            }
            4 => value["env"] = serde_json::json!([[b"BAD=NAME".as_slice(), b"secret".as_slice()]]),
            5 => value["env"] = serde_json::json!([[b"".as_slice(), b"secret".as_slice()]]),
            6 => value["unexpected"] = serde_json::json!("secret"),
            _ => unreachable!(),
        }
        let (child, mut channel) = gate();
        channel.write_all(&frame(&value)).unwrap();
        failure(child, channel, 2);
        assert!(!root.path.join("marker").exists());
    }
}

#[test]
fn complete_request_is_required_and_workload_keeps_stdin_environment_and_pid() {
    let root = TempDir::new();
    let mut value = request(&root.path);
    value["args"] = serde_json::json!([
        b"-c".as_slice(),
        b"read value; printf '%s:%s:%s' \"$$\" \"$ONLY\" \"$value\"; printf ran > marker"
            .as_slice()
    ]);
    value["env"] = serde_json::json!([[b"ONLY".as_slice(), b"private-value".as_slice()]]);
    let bytes = frame(&value);
    let (mut child, mut channel) = gate();
    let pid = child.id();
    channel.write_all(&bytes[..bytes.len() - 1]).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert!(!root.path.join("marker").exists());
    assert!(child.try_wait().unwrap().is_none());
    channel.write_all(&bytes[bytes.len() - 1..]).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert!(!root.path.join("marker").exists());
    channel.shutdown(Shutdown::Write).unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"workload-input\n")
        .unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(5));
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        format!("{pid}:private-value:workload-input").as_bytes()
    );
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read(root.path.join("marker")).unwrap(), b"ran");
    let mut diagnostic = Vec::new();
    channel.read_to_end(&mut diagnostic).unwrap();
    assert!(diagnostic.is_empty());
}

#[test]
fn setup_and_exec_failures_are_fixed_distinct_codes() {
    let root = TempDir::new();
    for (field, code) in [("cwd", 3), ("executable", 4)] {
        let mut value = request(&root.path);
        value[field] =
            serde_json::json!(root.path.join("missing-secret-path").as_os_str().as_bytes());
        let (child, mut channel) = gate();
        channel.write_all(&frame(&value)).unwrap();
        failure(child, channel, code);
        assert!(!root.path.join("marker").exists());
    }
}

#[test]
fn startup_wait_is_bounded_without_owner_closure() {
    let (child, channel) = gate();
    let start = std::time::Instant::now();
    // Keep the writer open: no EOF is available to rescue an unbounded reader.
    let output = wait_for_child_output(child, Duration::from_secs(13));
    assert_eq!(output.status.code(), Some(125));
    assert!(start.elapsed() < Duration::from_secs(13));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    drop(channel);
}

#[test]
fn workload_keeps_unix_cwd_and_argument_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let root = TempDir::new();
    let raw_dir = root
        .path
        .join(std::ffi::OsString::from_vec(b"cwd-\xff".to_vec()));
    std::fs::create_dir(&raw_dir).unwrap();
    let (child, mut channel) = gate();
    let mut value = request(&raw_dir);
    value["args"] = serde_json::json!([
        b"-c".as_slice(),
        b"printf '%s' \"$1\"; printf ran > marker".as_slice(),
        b"test".as_slice(),
        b"arg-\xff".as_slice()
    ]);
    channel.write_all(&frame(&value)).unwrap();
    channel.shutdown(Shutdown::Write).unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(5));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"arg-\xff");
    assert!(raw_dir.join("marker").exists());
    let mut byte = [0];
    assert_eq!(channel.read(&mut byte).unwrap(), 0);
}

#[test]
fn workload_signal_mask_is_reset_before_exec() {
    use std::os::unix::process::ExitStatusExt;
    let root = TempDir::new();
    let mut value = request(&root.path);
    value["args"] = serde_json::json!([
        b"-c".as_slice(),
        b"kill -TERM $$; printf unexpected > marker".as_slice()
    ]);
    let (child, mut channel) = gate();
    channel.write_all(&frame(&value)).unwrap();
    channel.shutdown(Shutdown::Write).unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(5));
    assert_eq!(output.status.signal(), Some(libc::SIGTERM));
    assert!(!root.path.join("marker").exists());
}

#[test]
fn startup_descriptor_is_closed_in_the_executed_workload() {
    let root = TempDir::new();
    let (child, mut channel, fd) = gate_with_descriptor();
    let mut value = request(&root.path);
    value["executable"] =
        serde_json::json!(std::env::var("NIXFIED_TEST_CHILD").unwrap().as_bytes());
    value["args"] = serde_json::json!([b"assert-fd-closed".to_vec(), fd.to_string().into_bytes()]);
    channel.write_all(&frame(&value)).unwrap();
    channel.shutdown(Shutdown::Write).unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(5));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

fn owner_registry(root: &std::path::Path) -> nixfied_runtime::registry::Registry {
    let identity =
        nixfied_runtime::registry::RegistryIdentity::default_slot("launch", "abi", "toolchain");
    let placement =
        nixfied_runtime::state::placement::derive_slot_placement("launch", "dev", 0, "run", root)
            .unwrap();
    nixfied_runtime::registry::Registry::open_or_create(registry_guard(&placement), &identity)
        .unwrap()
}
fn prepared(root: &std::path::Path) -> nixfied_runtime::launch::PreparedLaunch {
    nixfied_runtime::launch::PreparedLaunch::new(
        &std::env::var("NIXFIED_TEST_SHELL").unwrap(),
        &["-c".into(), "printf ran > marker".into()],
        &std::collections::BTreeMap::from([("PAYLOAD".into(), "x".repeat(60_000))]),
        root,
    )
    .unwrap()
}

#[test]
fn owner_registration_precedes_permission_and_failed_commit_keeps_child_owned() {
    use nixfied_runtime::registry::EventInsert;
    use nixfied_runtime::{ErrorCode, RuntimeError};
    for reject in [false, true] {
        let root = TempDir::new();
        let mut registry = owner_registry(&root.path);
        if reject {
            registry.connection().execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT, 'injected'); END").unwrap();
        }
        let pending = prepared(&root.path)
            .spawn(
                &runtime_binary(),
                registry.authority(),
                Stdio::null(),
                Stdio::piped(),
                Stdio::piped(),
            )
            .unwrap();
        let result = pending.register_and_release(
            |child| {
                assert!(!root.path.join("marker").exists());
                assert_eq!(
                    unsafe { libc::getpgid(child.id() as i32) },
                    child.id() as i32
                );
                #[cfg(target_os = "linux")]
                {
                    assert!(
                        std::fs::read(format!("/proc/{}/environ", child.id()))
                            .unwrap()
                            .is_empty()
                    );
                    assert_eq!(
                        std::fs::read_link(format!("/proc/{}/cwd", child.id())).unwrap(),
                        std::path::Path::new("/")
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
                assert!(!root.path.join("marker").exists());
                let payload = format!("{{\"pid\":{}}}", child.id());
                registry.append_event(EventInsert::new("test.process-registered", &payload))?;
                // Independent connection observes the committed callback record
                // before permission delivery can start. This is a primitive
                // callback proof, not a claim of workload-path integration.
                let observed = rusqlite::Connection::open(registry.path()).unwrap();
                let count: i64 = observed
                    .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
                    .unwrap();
                if count != 1 {
                    return Err(RuntimeError::new(
                        ErrorCode::RegistryCorrupt,
                        "registration not visible",
                    ));
                }
                Ok(())
            },
            || Ok(()),
        );
        if reject {
            let failure = result.expect_err("failed registration must retain the child");
            assert_eq!(failure.error.code, ErrorCode::RegistryCorrupt);
            let output = wait_for_child_output(failure.child, Duration::from_secs(3));
            assert_eq!(output.status.code(), Some(125));
            assert!(!root.path.join("marker").exists());
            assert_eq!(
                registry
                    .connection()
                    .query_row("SELECT count(*) FROM events", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        } else {
            let child = result.ok().expect("registered request should execute");
            let output = wait_for_child_output(child, Duration::from_secs(3));
            assert!(output.status.success());
            assert_eq!(std::fs::read(root.path.join("marker")).unwrap(), b"ran");
        }
        registry.close().unwrap();
    }
}

#[test]
fn cancellation_after_registration_sends_no_permission_and_returns_child() {
    let root = TempDir::new();
    let registry = owner_registry(&root.path);
    let pending = prepared(&root.path)
        .spawn(
            &runtime_binary(),
            registry.authority(),
            Stdio::null(),
            Stdio::piped(),
            Stdio::piped(),
        )
        .unwrap();
    let registered = std::cell::Cell::new(false);
    let result = pending.register_and_release(
        |_| {
            registered.set(true);
            Ok(())
        },
        || {
            if registered.get() {
                Err(nixfied_runtime::cancellation::canceled_error())
            } else {
                Ok(())
            }
        },
    );
    let failure = result.expect_err("cancellation must stop delivery");
    assert_eq!(failure.error.code, nixfied_runtime::ErrorCode::Canceled);
    let output = wait_for_child_output(failure.child, Duration::from_secs(3));
    assert_eq!(output.status.code(), Some(125));
    assert!(!root.path.join("marker").exists());
    registry.close().unwrap();
}

#[test]
fn owner_reports_exec_failure_without_losing_the_child_or_request_values() {
    let root = TempDir::new();
    let registry = owner_registry(&root.path);
    let launch = nixfied_runtime::launch::PreparedLaunch::new(
        root.path.join("missing-secret-program").to_str().unwrap(),
        &[],
        &std::collections::BTreeMap::new(),
        &root.path,
    )
    .unwrap();
    let pending = launch
        .spawn(
            &runtime_binary(),
            registry.authority(),
            Stdio::null(),
            Stdio::piped(),
            Stdio::piped(),
        )
        .unwrap();
    let failure = pending
        .register_and_release(|_| Ok(()), || Ok(()))
        .expect_err("exec must fail");
    assert_eq!(failure.error.message, "workload exec failed");
    let output = wait_for_child_output(failure.child, Duration::from_secs(3));
    assert_eq!(output.status.code(), Some(125));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    registry.close().unwrap();
}

#[test]
fn abandoned_pending_launch_returns_an_inert_child_for_reaping() {
    let root = TempDir::new();
    let registry = owner_registry(&root.path);
    let pending = prepared(&root.path)
        .spawn(
            &runtime_binary(),
            registry.authority(),
            Stdio::null(),
            Stdio::piped(),
            Stdio::piped(),
        )
        .unwrap();
    let output = wait_for_child_output(pending.abort(), Duration::from_secs(3));
    assert_eq!(output.status.code(), Some(125));
    assert!(!root.path.join("marker").exists());
    registry.close().unwrap();
}

#[test]
fn owner_rejects_invalid_or_oversized_requests_before_constructing_a_launch() {
    use nixfied_runtime::launch::PreparedLaunch;
    let root = TempDir::new();
    let executable = std::env::var("NIXFIED_TEST_SHELL").unwrap();
    for (args, env) in [
        (
            vec!["secret\0value".into()],
            std::collections::BTreeMap::new(),
        ),
        (
            vec!["x".repeat(1024 * 1024)],
            std::collections::BTreeMap::new(),
        ),
        (
            vec![],
            std::collections::BTreeMap::from([("BAD=KEY".into(), "secret".into())]),
        ),
    ] {
        let error = PreparedLaunch::new(&executable, &args, &env, &root.path)
            .err()
            .expect("request must reject");
        assert_eq!(error.code, nixfied_runtime::ErrorCode::ProcEscape);
        assert!(!error.message.contains("secret"));
    }
    assert!(!root.path.join("marker").exists());
}

#[test]
fn native_task_registration_failure_cannot_execute_the_workload() {
    use nixfied_runtime::registry::{Registry, RegistryIdentity};
    use nixfied_runtime::service::{
        RunContext, record_run_created, run_dependent_task_cancellable,
    };
    use nixfied_runtime::state::{derive_host_placement, materialize_run_roots};
    let root = TempDir::new();
    let counter = root.path.join("must-not-execute");
    let mut value = test_child_manifest(23180, 23180);
    value["tasks"]["smoke"]["requires"] = serde_json::json!([]);
    let program = value["tasks"]["smoke"]["invocation"]["run"][0].clone();
    value["tasks"]["smoke"]["invocation"]["run"] =
        serde_json::json!([program, "output", "occurrence", counter, "0"]);
    let manifest: nixfied_manifest::Manifest = serde_json::from_value(value).unwrap();
    let admission = fixture_admission(&manifest, &root.path);
    let placement = derive_host_placement(&manifest, "gated-task", &root.path).unwrap();
    materialize_run_roots(&placement).unwrap();
    let mut registry = Registry::open_or_create(
        registry_guard(&placement),
        &RegistryIdentity::default_slot(
            &manifest.project.project_id,
            &manifest.runtime_abi,
            &manifest.toolchain_id,
        ),
    )
    .unwrap();
    record_run_created(&mut registry, "gated-task", &admission, &placement).unwrap();
    registry.connection().execute_batch("CREATE TRIGGER deny_task_registration BEFORE INSERT ON events WHEN NEW.event_type = 'task.running' BEGIN SELECT RAISE(ABORT, 'injected'); END").unwrap();
    let error = run_dependent_task_cancellable(
        &placement,
        &mut registry,
        RunContext::new(
            &runtime_binary(),
            &admission,
            "gated-task",
            &placement.state_root,
            &nixfied_runtime::redaction::Redactor::empty(),
        ),
        &[],
        "smoke",
        0,
        admission
            .common()
            .execution_manifest()
            .leaf("smoke")
            .unwrap(),
        &nixfied_runtime::cancellation::CancellationToken::new(),
        nixfied_runtime::output::SourcePresentation::Shown,
    )
    .expect_err("registration must fail");
    assert_eq!(
        error.error().code,
        nixfied_runtime::ErrorCode::RegistryCorrupt
    );
    assert!(!counter.exists());
    assert_eq!(
        registry
            .connection()
            .query_row("SELECT count(*) FROM processes", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        registry
            .connection()
            .query_row(
                "SELECT count(*) FROM events WHERE event_type='task.running'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    registry.close().unwrap();
}
