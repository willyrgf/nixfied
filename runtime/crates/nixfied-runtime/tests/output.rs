//! End-to-end coverage for the direct-leaf task-output projection.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

use common::{
    RuntimeFixture, TempDir, available_port_window, runtime_binary, synthetic_manifest, test_child,
    wait_for_child_output, wait_for_path,
};
use nixfied_runtime::redaction::REDACTION_TOKEN;
use serde_json::{Value, json};

fn task_manifest(args: &[String]) -> Value {
    task_manifest_at(args, available_port_window(1))
}

fn leaf_task_manifest(args: &[String]) -> Value {
    // Unused service metadata needs no host port observation.
    let mut manifest = task_manifest_at(args, 23180);
    manifest["tasks"]["smoke"]["requires"] = json!([]);

    manifest
}

fn task_manifest_at(args: &[String], port: u16) -> Value {
    let executable = test_child();
    let executable = executable
        .to_str()
        .expect("test child path should be UTF-8")
        .to_string();
    let mut manifest = synthetic_manifest(
        &executable,
        &["listen", "127.0.0.1", "${port}", "hold"],
        port,
        port,
    );
    set_task_run_args(&mut manifest, args);
    manifest
}

fn set_task_run_args(manifest: &mut Value, args: &[String]) {
    let program = manifest["tasks"]["smoke"]["invocation"]["run"]
        .as_array()
        .expect("fixture task invocation should be an array")
        .first()
        .cloned()
        .expect("fixture task invocation should have a program");
    let mut run = vec![program];
    run.extend(args.iter().cloned().map(Value::String));
    manifest["tasks"]["smoke"]["invocation"]["run"] = Value::Array(run);
}

fn set_task_default_output(manifest: &mut Value, output: &str) {
    manifest["tasks"]["smoke"]["defaultOutput"] = json!(output);
}

fn run(fixture: &RuntimeFixture, extra: &[&str]) -> Output {
    fixture
        .command("run", extra)
        .output()
        .expect("runtime command should execute")
}

fn assert_stream_contains(haystack: &[u8], needle: &[u8], stream: &str) {
    assert!(
        haystack
            .windows(needle.len())
            .any(|window| window == needle),
        "{stream} did not contain expected bytes\nexpected: {needle:?}\nactual: {haystack:?}"
    );
}

#[test]
fn escaped_idle_and_continuous_writers_cannot_hold_capture_or_publish_replay() {
    struct Survivor(libc::pid_t);
    impl Drop for Survivor {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
        }
    }
    for activity in ["idle", "continuous"] {
        for secret in [false, true] {
            let markers = TempDir::new();
            let pid_path = markers.path.join("pid");
            let acknowledgement = markers.path.join("ack");
            let prefix = if secret {
                b"safe-data-abc".as_slice()
            } else {
                b"safe-data".as_slice()
            };
            let args = vec![
                "output".into(),
                "escaped-writer".into(),
                activity.into(),
                pid_path.to_string_lossy().into_owned(),
                acknowledgement.to_string_lossy().into_owned(),
                hex(prefix),
                hex(prefix),
            ];
            let mut manifest = leaf_task_manifest(&args);
            if secret {
                manifest["secrets"]["token"] = json!({"secretId":"token","source":{
                    "kind":"env-var","envVar":"NIXFIED_CAPTURE_SECRET"
                }});
                manifest["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:token}");
            }
            let fixture = RuntimeFixture::new(manifest);
            let child = fixture
                .command("run", &["--task", "smoke", "--output", "task-output"])
                .env("NIXFIED_CAPTURE_SECRET", "abcdef")
                .spawn()
                .unwrap();
            assert!(wait_for_path(&pid_path, Duration::from_secs(5)));
            let survivor = Survivor(
                fs::read_to_string(&pid_path)
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap(),
            );
            assert_eq!(
                unsafe { libc::getpgid(survivor.0) },
                survivor.0,
                "fixture must have escaped into its own group"
            );
            let started = std::time::Instant::now();
            fs::write(acknowledgement, b"release parent").unwrap();
            let output = wait_for_child_output(child, Duration::from_secs(8));
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "capture must not wait for escaped writers"
            );
            assert!(!output.status.success());
            assert!(
                output.stdout.is_empty(),
                "incomplete capture must not replay safe prefix files"
            );
            assert_eq!(
                unsafe { libc::kill(survivor.0, 0) },
                0,
                "capture timeout is not proof of process death"
            );
            let text = String::from_utf8_lossy(&output.stderr);
            assert!(text.contains("SECRET_LEAK_BLOCKED"), "{text}");
            // Task-output emits human diagnostics. Inspect the durable aggregate summary
            // and files directly: no completed node or task summary may be published.
            let runs = fixture.state_base.join("runtime-test/dev/0/runs");
            let run = fs::read_dir(runs).unwrap().next().unwrap().unwrap().path();
            let summary: Value =
                serde_json::from_slice(&fs::read(run.join("artifacts/run-summary.json")).unwrap())
                    .unwrap();
            assert_eq!(summary["nodes"], json!([]));
            assert!(!run.join("summary.0.json").exists());
            for stream in ["stdout", "stderr"] {
                let path = run.join(format!("logs/task.0.{stream}.log"));
                let before = fs::read(&path).unwrap();
                assert!(!before.is_empty());
                if secret && activity == "idle" {
                    assert_eq!(before, b"safe-dat");
                }
                // The survivor remains alive with writers. Returned evidence is closed.
                std::thread::sleep(Duration::from_millis(30));
                assert_eq!(fs::read(path).unwrap(), before);
            }
            drop(survivor);
        }
    }
}

#[test]
fn unused_graph_and_template_faults_reject_before_state_or_child_effects() {
    for fault in ["cycle", "template", "nested-secret"] {
        let mut manifest = leaf_task_manifest(&["prepare".into(), "child-started".into()]);
        match fault {
            "cycle" => manifest["services"]["synthetic"]["connectsTo"] = json!(["synthetic"]),
            "template" => {
                manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["env"]["BAD"] =
                    json!("${port:${HOME}}")
            }
            "nested-secret" => {
                manifest["tasks"]["smoke"]["invocation"]["run"] = json!([
                    test_child().file_name().unwrap().to_str().unwrap(),
                    "prepare",
                    "${HOME:-${secret:undeclared}}"
                ])
            }
            _ => unreachable!(),
        }
        let fixture = RuntimeFixture::new(manifest);
        let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);
        assert!(!output.status.success(), "{fault}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("MANIFEST_ADMISSION"),
            "{fault}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!fixture.state_base.exists(), "{fault} materialized state");
        assert!(
            !fixture.tmp.path.join("child-started").exists(),
            "{fault} started a child"
        );
    }
}

#[test]
fn child_receives_inserted_state_path_without_recursive_substitution() {
    let mut manifest = leaf_task_manifest(&["output".into(), "env".into(), "VALUE".into()]);
    manifest["tasks"]["smoke"]["invocation"]["env"]["VALUE"] = json!("${HOME:-${stateDir}}");
    let mut fixture = RuntimeFixture::new(manifest);
    fixture.state_base = fixture.tmp.path.join("state-${port:unresolved}");
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "${{HOME:-{}}}",
            fixture.state_base.join("runtime-test/dev/0").display()
        )
    );
}

#[test]
fn run_and_aggregate_views_cross_the_native_redaction_and_formatting_boundary() {
    let mut manifest = leaf_task_manifest(&["exit".to_string(), "0".to_string()]);
    manifest["secrets"]["token"] = json!({"secretId":"token","source":{
        "kind":"env-var","envVar":"NIXFIED_OUTPUT_VIEW_SECRET"
    }});
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .env("NIXFIED_OUTPUT_VIEW_SECRET", "smoke")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = String::from_utf8(output.stdout).unwrap();
    assert!(bytes.starts_with("{\n  \"computedManifestHash\": "));
    assert!(bytes.ends_with("\n}\n"));
    assert!(!bytes.contains("smoke"));
    let result: Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(result["task"]["taskId"], REDACTION_TOKEN);
    assert_eq!(result["task"]["exitCode"], 0);
    assert_eq!(result["task"]["success"], true);
    assert_eq!(result["nodes"][0]["nodeId"], REDACTION_TOKEN);
    assert_eq!(result["services"], json!([]));
    let summary = fs::read_to_string(result["runSummaryPath"].as_str().unwrap()).unwrap();
    assert!(summary.starts_with("{\n  \"durationMs\": "));
    assert!(summary.ends_with("\n}"));
    assert!(!summary.ends_with('\n'));
    assert!(!summary.contains("smoke"));
    let value: Value = serde_json::from_str(&summary).unwrap();
    assert_eq!(value["tasks"][0]["taskId"], REDACTION_TOKEN);
    assert_eq!(value["nodes"][0]["success"], true);
    assert_eq!(value["success"], true);
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "durationMs",
            "nodes",
            "runId",
            "services",
            "success",
            "tasks"
        ]
    );
}

#[test]
fn direct_leaf_replays_exact_binary_without_metadata() {
    let stdout = [0_u8, 1, 2, 0, 0xff, b'\n'];
    let stderr = b"stderr-without-final-newline\0";
    let args = vec![
        "output".to_string(),
        "hex".to_string(),
        hex(&stdout),
        hex(stderr),
    ];
    let fixture = RuntimeFixture::new(task_manifest(&args));
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);

    assert!(
        output.status.success(),
        "task-output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, stderr, "stderr");
    assert!(
        !output.stdout.windows(1).any(|window| window == b"{"),
        "task-output stdout must not contain runtime JSON"
    );
}

#[test]
fn leaf_default_replays_when_output_is_omitted() {
    let stdout = b"default stdout";
    let stderr = b"default stderr";
    let mut manifest = leaf_task_manifest(&[
        "output".to_string(),
        "hex".to_string(),
        hex(stdout),
        hex(stderr),
    ]);
    set_task_default_output(&mut manifest, "task-output");
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke"]);

    assert!(
        output.status.success(),
        "implicit task-output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, stderr, "stderr");
}

#[test]
fn explicit_output_overrides_leaf_default() {
    let mut manifest = leaf_task_manifest(&["exit".to_string(), "0".to_string()]);
    set_task_default_output(&mut manifest, "task-output");
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "summary"]);

    assert!(
        output.status.success(),
        "explicit summary failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "summary must not replay task stdout"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("result: ok"));
}

#[test]
fn composite_selection_uses_its_metadata_default_not_a_child_default() {
    let mut manifest = task_manifest(&["exit".to_string(), "0".to_string()]);
    set_task_default_output(&mut manifest, "task-output");
    manifest["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "defaultOutput": "summary",
        "serviceLifetime": "run-scoped",

        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "pipeline"]);

    assert!(
        output.status.success(),
        "composite run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "composites must not replay child output"
    );
}

#[test]
fn accepted_nonzero_code_replays_and_succeeds() {
    let stdout = b"accepted nonzero\0";
    let stderr = b"diagnostic";
    let args = vec![
        "output".to_string(),
        "hex-exit".to_string(),
        hex(stdout),
        hex(stderr),
        "7".to_string(),
    ];
    let mut manifest = task_manifest(&args);
    manifest["tasks"]["smoke"]["exitPolicy"]["successCodes"] = json!([0, 7]);
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);

    assert!(
        output.status.success(),
        "accepted nonzero failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, stderr, "stderr");
}

#[test]
fn empty_output_is_a_valid_zero_byte_replay() {
    let args = vec![
        "output".to_string(),
        "hex".to_string(),
        String::new(),
        String::new(),
    ];
    let manifest = leaf_task_manifest(&args);
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);

    assert!(
        output.status.success(),
        "empty task-output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn large_simultaneous_streams_replay_exactly() {
    let stdout = vec![0x61; 256 * 1024];
    let stderr = vec![0x7a; 192 * 1024];
    let args = vec![
        "output".to_string(),
        "repeat".to_string(),
        "61".to_string(),
        stdout.len().to_string(),
        "7a".to_string(),
        stderr.len().to_string(),
    ];
    let manifest = leaf_task_manifest(&args);
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);

    assert!(
        output.status.success(),
        "large task-output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, &stderr, "stderr");
}

#[test]
fn broken_stdout_pipe_is_typed_and_does_not_stop_stderr_replay() {
    let stdout = vec![b'x'; 128 * 1024];
    let stderr = vec![b'z'; 32 * 1024];
    let args = vec![
        "output".to_string(),
        "repeat".to_string(),
        "78".to_string(),
        stdout.len().to_string(),
        "7a".to_string(),
        stderr.len().to_string(),
    ];
    let manifest = leaf_task_manifest(&args);
    let fixture = RuntimeFixture::new(manifest);
    let mut child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .expect("runtime command should spawn");
    drop(child.stdout.take());
    let output = child
        .wait_with_output()
        .expect("runtime output should be collected");

    assert_eq!(output.status.code(), Some(38));
    assert!(output.stdout.is_empty());
    assert_stream_contains(&output.stderr, &stderr, "stderr");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("OUTPUT_PROJECTION_FAILED"));
    assert!(diagnostic.contains("broken-pipe"));
}

#[test]
fn task_failure_replays_captured_bytes_and_preserves_status() {
    let stdout = b"failed stdout";
    let stderr = b"failed stderr";
    let args = vec![
        "output".to_string(),
        "hex-exit".to_string(),
        hex(stdout),
        hex(stderr),
        "7".to_string(),
    ];
    let fixture = RuntimeFixture::new(task_manifest(&args));
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);

    assert_eq!(output.status.code(), Some(30));
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, stderr, "stderr");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error: TASK_FAILED:"),
        "task failure should remain the public status: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn redaction_happens_before_task_output_replay() {
    let mut manifest =
        leaf_task_manifest(&["output".to_string(), "env".to_string(), "TOKEN".to_string()]);
    manifest["secrets"]["api-token"] = json!({
        "secretId": "api-token",
        "source": {
            "kind": "env-var",
            "envVar": "NIXFIED_TEST_TASK_SECRET"
        }
    });
    manifest["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:api-token}");
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .env("NIXFIED_TEST_TASK_SECRET", "child-visible-secret")
        .output()
        .expect("runtime command should execute");

    assert!(
        output.status.success(),
        "redacted task-output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, REDACTION_TOKEN.as_bytes());
    let secret = b"child-visible-secret";
    assert!(
        !output
            .stdout
            .windows(secret.len())
            .any(|window| window == secret)
    );
    assert!(
        !output
            .stderr
            .windows(secret.len())
            .any(|window| window == secret)
    );
}

#[test]
fn timeout_replays_output_before_reporting_task_failure() {
    let marker = tempfile_marker("timeout");
    let stdout = b"timeout stdout";
    let stderr = b"timeout stderr";
    let args = vec![
        "output".to_string(),
        "hex-block".to_string(),
        hex(stdout),
        hex(stderr),
        marker.to_string_lossy().into_owned(),
    ];
    let mut manifest = leaf_task_manifest(&args);
    manifest["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(150);
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "task-output"]);

    assert_eq!(output.status.code(), Some(30));
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, stderr, "stderr");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("TASK_FAILED"),
        "timeout should remain a task failure"
    );
    assert!(wait_for_path(&marker, Duration::from_secs(1)));
}

#[test]
fn task_without_deadline_survives_former_default_and_remains_cancelable() {
    let marker = tempfile_marker("cancel");
    let stdout = b"cancel stdout";
    let stderr = b"cancel stderr";
    let args = vec![
        "output".to_string(),
        "hex-block".to_string(),
        hex(stdout),
        hex(stderr),
        marker.to_string_lossy().into_owned(),
    ];
    let mut manifest = leaf_task_manifest(&args);
    manifest["tasks"]["smoke"]["invocation"]
        .as_object_mut()
        .unwrap()
        .remove("timeoutMs");
    let fixture = RuntimeFixture::new(manifest);
    let mut child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .expect("runtime command should spawn");
    assert!(wait_for_path(&marker, Duration::from_secs(3)));
    std::thread::sleep(Duration::from_secs(31));
    assert!(
        child.try_wait().unwrap().is_none(),
        "absent deadline must not reconstruct the former 30s default"
    );
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let output = wait_for_child_output(child, Duration::from_secs(6));

    assert_eq!(output.status.code(), Some(27));
    assert_eq!(output.stdout, stdout);
    assert_stream_contains(&output.stderr, stderr, "stderr");
    assert!(String::from_utf8_lossy(&output.stderr).contains("CANCELED"));
}

#[test]
fn service_exit_interrupts_a_task_without_deadline() {
    for unrelated in [false, true] {
        let marker = tempfile_marker("dependency-exit");
        let args = vec![
            "output".into(),
            "hex-block".into(),
            "".into(),
            "".into(),
            marker.to_string_lossy().into_owned(),
        ];
        let mut manifest = task_manifest(&args);
        manifest["tasks"]["smoke"]["invocation"]
            .as_object_mut()
            .unwrap()
            .remove("timeoutMs");
        let selected = if unrelated {
            let mut first = manifest["tasks"]["smoke"].clone();
            first["operationId"] = json!("task.first.run");
            let program = first["invocation"]["run"][0].clone();
            first["invocation"]["run"] = json!([program, "exit", "0"]);
            manifest["tasks"]["first"] = first;
            manifest["tasks"]["smoke"]["requires"] = json!([]);
            manifest["tasks"]["pipeline"] = json!({
                "kind": "composite", "defaultOutput": "summary", "serviceLifetime": "run-scoped",
                "steps": {"first": {"task": "first", "dependsOn": []}, "second": {"task": "smoke", "dependsOn": ["first"]}}
            });
            "pipeline"
        } else {
            "smoke"
        };
        let fixture = RuntimeFixture::new(manifest);
        let child = fixture
            .command("run", &["--task", selected, "--output", "json"])
            .spawn()
            .unwrap();
        assert!(wait_for_path(&marker, Duration::from_secs(5)));
        let database = common::find_named(&fixture.state_base, "registry.sqlite3").unwrap();
        let registry = rusqlite::Connection::open(database).unwrap();
        let pid: i32 = registry
            .query_row(
                "SELECT pid FROM processes WHERE service_instance_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        let output = wait_for_child_output(child, Duration::from_secs(6));
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("DEPENDENCY_UNAVAILABLE"));
    }
}

#[test]
fn service_failure_interrupts_another_services_exec_probe() {
    for phase in ["ready", "health"] {
        for victim in ["synthetic", "later"] {
            let probe_marker = tempfile_marker("probe-observation");
            let task_marker = tempfile_marker("unreleased-task");
            let port = available_port_window(2);
            let mut manifest = task_manifest_at(
                &["prepare".into(), task_marker.to_string_lossy().into_owned()],
                port,
            );
            manifest["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
            let mut later = manifest["services"]["synthetic"].clone();
            later["connectsTo"] = json!(["synthetic"]);
            for operation in ["start", "ready", "health", "stop", "clean"] {
                later["lifecycle"][operation]["operationId"] = json!(format!("later.{operation}"));
            }
            let mut probe_invocation = manifest["tasks"]["smoke"]["invocation"].clone();
            let program = probe_invocation["run"][0].clone();
            probe_invocation["run"] = json!([
                program,
                "output",
                "hex-block",
                "",
                "",
                probe_marker.to_string_lossy()
            ]);
            later["lifecycle"][phase]["probe"] = json!({
                "kind": "exec", "invocation": probe_invocation,
                "timeoutMs": 30000, "retryIntervalMs": 100, "maxAttempts": 1
            });
            manifest["services"]["later"] = later;
            manifest["tasks"]["smoke"]["requires"] = json!(["later"]);
            let fixture = RuntimeFixture::new(manifest);
            let child = fixture
                .command("run", &["--task", "smoke", "--output", "json"])
                .spawn()
                .unwrap();
            assert!(
                wait_for_path(&probe_marker, Duration::from_secs(5)),
                "{phase} probe did not start"
            );
            let registry = rusqlite::Connection::open(
                common::find_named(&fixture.state_base, "registry.sqlite3").unwrap(),
            )
            .unwrap();
            let pid: i32 = registry.query_row(
                "SELECT p.pid FROM processes p JOIN services s ON s.service_instance_id = p.service_instance_id WHERE s.service_name = ?1",
                [victim], |row| row.get(0)
            ).unwrap();
            assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
            // A 30s probe cannot explain completion inside this bound. The marker
            // proves the failure occurred while its child was actually running.
            let output = wait_for_child_output(child, Duration::from_secs(6));
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains(if victim == "later" {
                    "PROC_ESCAPE"
                } else {
                    "DEPENDENCY_UNAVAILABLE"
                }),
                "{error}"
            );
            assert!(
                !task_marker.exists(),
                "no task may start after observed service failure"
            );
        }
    }
}

#[test]
fn cancellation_during_replay_is_recorded_once_and_finishes_cleanup() {
    use std::io::Read;
    let count = 1024 * 1024;
    let manifest = task_manifest(&[
        "output".into(),
        "repeat".into(),
        "78".into(),
        count.to_string(),
        "79".into(),
        "0".into(),
    ]);
    let fixture = RuntimeFixture::new(manifest);
    let mut child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .unwrap();
    // A byte on the runtime's stdout proves task capture finished and replay
    // started. Keep the large pipe blocked until the signal has been sent.
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0];
    stdout.read_exact(&mut first).unwrap();
    assert_eq!(first, [b'x']);
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let reader = std::thread::spawn(move || {
        let mut bytes = vec![first[0]];
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let output = wait_for_child_output(child, Duration::from_secs(10));
    assert_eq!(reader.join().unwrap(), vec![b'x'; count]);
    assert_eq!(output.status.code(), Some(27));
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert_eq!(diagnostic.matches("CANCELED").count(), 1, "{diagnostic}");
    let registry = rusqlite::Connection::open(
        fixture
            .state_base
            .join("registry/runtime-test/dev/0/registry.sqlite3"),
    )
    .unwrap();
    let unfinished: i64 = registry
        .query_row(
            "SELECT (SELECT count(*) FROM processes WHERE status IN ('starting','running','ready'))
              + (SELECT count(*) FROM run_leases WHERE status IN ('active','canceling'))
              + (SELECT count(*) FROM ports WHERE status IN ('reserved','active'))",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        unfinished, 0,
        "cancellation must continue service and lease cleanup"
    );
}

#[test]
fn invalid_selection_is_rejected_before_state_or_child_side_effects() {
    let mut manifest = task_manifest(&["exit".to_string(), "0".to_string()]);
    manifest["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "serviceLifetime": "run-scoped",

        "steps": { "only": { "task": "smoke" } }
    });
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "pipeline", "--output", "task-output"]);

    assert_eq!(output.status.code(), Some(37));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("TASK_SELECTION_INVALID"));
    assert!(
        !fixture.state_base.exists(),
        "selection refusal must not materialize state"
    );
}

#[test]
fn parser_refuses_missing_unknown_repeated_and_alias_selections() {
    let manifest = task_manifest(&["exit".to_string(), "0".to_string()]);

    let missing = RuntimeFixture::new(manifest.clone());
    let output = run(&missing, &["--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(output.stdout.is_empty());
    assert!(!missing.state_base.exists());

    let unknown = RuntimeFixture::new(manifest.clone());
    let output = run(&unknown, &["--task", "missing", "--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(!unknown.state_base.exists());

    let repeated = RuntimeFixture::new(manifest.clone());
    let output = run(
        &repeated,
        &[
            "--task",
            "smoke",
            "--task",
            "smoke",
            "--output",
            "task-output",
        ],
    );
    assert_eq!(output.status.code(), Some(37));
    assert!(!repeated.state_base.exists());

    for spelling in ["--task-output", "--json", "--both", "--summary", "--output"] {
        let alias = RuntimeFixture::new(manifest.clone());
        let args = if spelling == "--output" {
            vec!["--task", "smoke", "--output", "task_output"]
        } else {
            vec!["--task", "smoke", spelling]
        };
        let output = run(&alias, &args);
        assert_eq!(output.status.code(), Some(35), "spelling {spelling}");
        assert!(!alias.state_base.exists());
    }
}

#[test]
fn task_output_conflicts_regardless_of_flag_order_and_projects_errors() {
    for args in [
        ["--output", "task-output", "--output", "json"],
        ["--output", "json", "--output", "task-output"],
        ["--output", "summary", "--output", "both"],
    ] {
        let fixture = RuntimeFixture::new(task_manifest(&["exit".to_string(), "0".to_string()]));
        let output = run(&fixture, &args);
        assert_eq!(output.status.code(), Some(36), "args {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.state_base.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("OUTPUT_MODE_CONFLICT"));
    }
}

fn hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn tempfile_marker(label: &str) -> PathBuf {
    common::temp_marker(&format!("nixfied-output-{label}"))
}

#[test]
fn non_utf8_environment_secret_never_reaches_diagnostics() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let marker_dir = TempDir::new();
    let marker = marker_dir.path.join("child-started");
    let mut manifest =
        task_manifest(&["prepare".to_string(), marker.to_string_lossy().into_owned()]);
    manifest["secrets"]["token"] = json!({"secretId":"token","source":{
        "kind":"env-var","envVar":"NIXFIED_TEST_INVALID_SECRET"
    }});
    manifest["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:token}");
    let fixture = RuntimeFixture::new(manifest);
    let mut bytes = b"synthetic-prefix-".to_vec();
    bytes.push(0xff);
    bytes.extend_from_slice(b"-synthetic-suffix");
    for mode in ["summary", "json", "both", "task-output"] {
        let output = fixture
            .command("run", &["--task", "smoke", "--output", mode])
            .env(
                "NIXFIED_TEST_INVALID_SECRET",
                OsString::from_vec(bytes.clone()),
            )
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(33));
        assert!(output.stdout.is_empty());
        assert!(!marker.exists());
        assert!(!fixture.state_base.exists());
        for fragment in [
            b"synthetic-prefix".as_slice(),
            b"synthetic-suffix".as_slice(),
        ] {
            assert!(
                !output
                    .stderr
                    .windows(fragment.len())
                    .any(|window| window == fragment),
                "rejected secret material reached {mode} diagnostics"
            );
        }
        if matches!(mode, "json" | "both") {
            let error = common::stderr_json(&output.stderr);
            assert_eq!(error["code"], "SECRET_UNAVAILABLE");
            assert_eq!(error["exitClass"], "error");
        }
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert!(diagnostic.contains("SECRET_UNAVAILABLE"));
        assert!(diagnostic.contains("is unavailable: value is not valid UTF-8"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn optional_host_ephemeral_observation_warns_without_executing_children() {
    let Ok(raw) = fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range") else {
        return;
    };
    let bounds = raw
        .split_whitespace()
        .map(str::parse::<u16>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(bounds.len(), 2);
    let executable = test_child();
    let manifest = synthetic_manifest(
        executable.to_str().unwrap(),
        &["listen", "127.0.0.1", "${port}", "hold"],
        bounds[0],
        bounds[0],
    );
    let fixture = RuntimeFixture::new(manifest);
    let output = Command::new(runtime_binary())
        .args(["check", "--allow-non-store-manifest", "--manifest"])
        .arg(&fixture.manifest_path)
        .current_dir(&fixture.tmp.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
        "overlaps the host ephemeral port range {}-{}",
        bounds[0], bounds[1]
    )));
    assert!(!fixture.state_base.exists());
}
