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
    for accepted in [false, true] {
        for composite in [false, true] {
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
                    if accepted {
                        manifest["tasks"]["smoke"]["exitPolicy"]["successCodes"] = json!([7]);
                    }
                    if secret {
                        manifest["secrets"]["token"] = json!({"secretId":"token","source":{
                            "kind":"env-var","envVar":"NIXFIED_CAPTURE_SECRET"
                        }});
                        manifest["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] =
                            json!("${secret:token}");
                    }
                    let later = markers.path.join("later-task");
                    if composite {
                        let mut after = manifest["tasks"]["smoke"].clone();
                        let program = after["invocation"]["run"][0].clone();
                        after["invocation"]["run"] = json!([program, "prepare", later]);
                        after["operationId"] = json!("task.after.run");
                        manifest["tasks"]["after"] = after;
                        manifest["tasks"]["pipeline"] = json!({
                            "kind": "composite", "defaultOutput": "summary", "steps": {
                                "first": {"task":"smoke"}, "after": {"task":"after", "dependsOn":["first"]}
                            }
                        });
                    }
                    let fixture = RuntimeFixture::new(manifest);
                    let child = fixture
                        .command(
                            "run",
                            &[
                                "--task",
                                if composite { "pipeline" } else { "smoke" },
                                // Capture settlement is the subject; presentation
                                // of the retained safe prefix has its own proofs.
                                "--output",
                                "json",
                            ],
                        )
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
                    // Live presentation may show safe redacted bytes before
                    // capture completes; it never shows a possible secret.
                    assert!(
                        !output.stdout.windows(6).any(|window| window == b"abcdef"),
                        "presentation never shows an undecided secret"
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
                    let runs = fixture.state_base.join("registry/runtime-test/dev/0/runs");
                    let run = fs::read_dir(runs).unwrap().next().unwrap().unwrap().path();
                    let summary: Value = serde_json::from_slice(
                        &fs::read(run.join("artifacts/run-summary.json")).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(summary["nodes"], json!([]));
                    assert!(!run.join("summary.0.json").exists());
                    for stream in ["stdout", "stderr"] {
                        let path = run.join(format!("logs/task.0.{stream}.log"));
                        let before = fs::read(&path).unwrap();
                        assert!(!before.is_empty());
                        if secret && activity == "idle" {
                            // Only the suffix that may still become the secret is discarded.
                            assert_eq!(before, b"safe-data-");
                        }

                        // The survivor remains alive with writers. Returned evidence is closed.
                        std::thread::sleep(Duration::from_millis(30));
                        assert_eq!(fs::read(path).unwrap(), before);
                    }
                    let connection = rusqlite::Connection::open_with_flags(
                        fixture
                            .state_base
                            .join("registry/runtime-test/dev/0/registry.sqlite3"),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .unwrap();
                    let observed: (String, i32) = connection.query_row(
                "SELECT execution_outcome, exit_code FROM processes WHERE service_instance_id IS NULL",
                [], |row| Ok((row.get(0)?, row.get(1)?)),
            ).unwrap();
                    let (capture, sealed): (String, String) = connection
                        .query_row(
                            "SELECT p.capture, r.output FROM processes p JOIN runs r USING (run_id)
                             WHERE p.service_instance_id IS NULL",
                            [],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .unwrap();
                    assert_eq!(
                        capture, "incomplete",
                        "delivery never upgrades incomplete capture"
                    );
                    assert_eq!(sealed, "sealed");
                    assert_eq!(
                        observed,
                        (if accepted { "succeeded" } else { "failed" }.into(), 7)
                    );
                    assert_eq!(
                        stored_execution_outcome(&fixture),
                        if accepted && !composite {
                            "succeeded"
                        } else {
                            "failed"
                        }
                    );
                    assert!(
                        !later.exists(),
                        "capture failure must stop further graph execution"
                    );
                    drop(survivor);
                }
            }
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
            fixture.state_base.join("data/runtime-test/dev/0").display()
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
    assert_eq!(stored_execution_outcome(&fixture), "succeeded");
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
    assert_eq!(stored_execution_outcome(&fixture), "failed");
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
    assert_eq!(stored_execution_outcome(&fixture), "failed");
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
    assert_eq!(stored_execution_outcome(&fixture), "canceled");
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
                "kind": "composite", "defaultOutput": "summary", "steps": {"first": {"task": "first", "dependsOn": []}, "second": {"task": "smoke", "dependsOn": ["first"]}}
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
        assert_eq!(stored_execution_outcome(&fixture), "failed");
    }
}

#[test]
fn services_with_exec_probes_keep_distinct_capture_files() {
    let port = available_port_window(2);
    let mut manifest = task_manifest_at(&["exit".into(), "0".into()], port);
    manifest["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
    let invocation = manifest["tasks"]["smoke"]["invocation"].clone();
    manifest["services"]["synthetic"]["lifecycle"]["ready"]["probe"] = json!({
        "kind": "exec", "invocation": invocation,
        "timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 1
    });
    let mut later = manifest["services"]["synthetic"].clone();
    later["connectsTo"] = json!(["synthetic"]);
    for operation in ["start", "ready", "health", "stop", "clean"] {
        later["lifecycle"][operation]["operationId"] = json!(format!("later.{operation}"));
    }
    manifest["services"]["later"] = later;
    manifest["tasks"]["smoke"]["requires"] = json!(["later"]);
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for service in ["synthetic", "later"] {
        assert!(
            common::find_named(
                &fixture.state_base,
                &format!("lifecycle.{service}.ready.probe.0.stdout.log")
            )
            .is_some()
        );
    }
    let registry = rusqlite::Connection::open(
        common::find_named(&fixture.state_base, "registry.sqlite3").unwrap(),
    )
    .unwrap();
    let probes: i64 = registry.query_row("SELECT count(*) FROM processes WHERE role='probe' AND status='succeeded' AND execution_outcome='succeeded'", [], |row| row.get(0)).unwrap();
    assert_eq!(probes, 2);
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
            let pid: i32 = registry
                .query_row(
                    "SELECT p.pid FROM processes p WHERE p.service_name = ?1 AND p.role = 'service'",
                    [victim],
                    |row| row.get(0),
                )
                .unwrap();
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
fn stalled_stdout_reader_blocks_neither_settlement_nor_slot_release() {
    let count = 4 * 1024 * 1024;
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
    // Hold the caller's stdout open without ever reading it.
    let stalled = child.stdout.take().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let registry = fixture
        .state_base
        .join("registry/runtime-test/dev/0/registry.sqlite3");
    loop {
        let settled = rusqlite::Connection::open_with_flags(
            &registry,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT finalization = 'complete' AND output = 'sealed' FROM runs",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .ok()
        })
        .unwrap_or(false);
        if settled {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a stalled reader must not delay session settlement"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        child.try_wait().unwrap().is_none(),
        "the command keeps presenting after settlement"
    );
    // Slot release follows the seal without waiting for the reader: another
    // owner acquires it while this command is still presenting.
    let clean = loop {
        let clean = run_control(&fixture, "clean");
        if clean.status.success() || std::time::Instant::now() >= deadline {
            break clean;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let output = wait_for_child_output(child, Duration::from_secs(10));
    drop(stalled);
    assert_eq!(output.status.code(), Some(38));
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("OUTPUT_PROJECTION_FAILED"),
        "{diagnostic}"
    );
    assert_eq!(
        stored_execution_outcome(&fixture),
        "succeeded",
        "interrupted delivery never rewrites the settled session"
    );
}

#[test]
fn live_task_output_arrives_before_the_task_finishes_and_down_ends_it() {
    use std::io::Read;
    let marker = tempfile_marker("live-output");
    let mut manifest = leaf_task_manifest(&[
        "output".into(),
        "hex-block".into(),
        hex(b"live stdout\n"),
        hex(b"live stderr\n"),
        marker.to_string_lossy().into_owned(),
    ]);
    manifest["tasks"]["smoke"]["invocation"]
        .as_object_mut()
        .unwrap()
        .remove("timeoutMs");
    let fixture = RuntimeFixture::new(manifest);
    let mut child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut live = vec![0_u8; b"live stdout\n".len()];
    stdout.read_exact(&mut live).unwrap();
    assert_eq!(
        live, b"live stdout\n",
        "output is presented while the task runs"
    );
    assert!(child.try_wait().unwrap().is_none());
    let down = run_control(&fixture, "down");
    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    let output = wait_for_child_output(child, Duration::from_secs(10));
    let mut rest = Vec::new();
    stdout.read_to_end(&mut rest).unwrap();
    assert!(
        rest.is_empty(),
        "the exact task bytes are shown once: {rest:?}"
    );
    assert_eq!(output.status.code(), Some(27));
    assert_stream_contains(&output.stderr, b"live stderr\n", "stderr");
    assert_eq!(stored_execution_outcome(&fixture), "canceled");
}

#[test]
fn summary_mode_labels_live_sources_on_stderr_and_keeps_stdout_empty() {
    let manifest = leaf_task_manifest(&[
        "output".into(),
        "hex".into(),
        hex(b"first\nsecond"),
        hex(b"problem\n"),
    ]);
    let fixture = RuntimeFixture::new(manifest);
    let output = run(&fixture, &["--task", "smoke", "--output", "summary"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    for line in [
        &b"[smoke] first\n"[..],
        b"[smoke] second\n",
        b"[smoke:err] problem\n",
        b"  ok smoke (smoke)",
        b"  result: ok 1 passed",
    ] {
        assert_stream_contains(&output.stderr, line, "stderr");
    }
}

fn run_control(fixture: &RuntimeFixture, command: &str) -> Output {
    fixture
        .command(command, &[])
        .stdout(std::process::Stdio::piped())
        .output()
        .unwrap()
}

#[test]
fn invalid_selection_is_rejected_before_state_or_child_side_effects() {
    let mut manifest = task_manifest(&["exit".to_string(), "0".to_string()]);
    manifest["tasks"]["pipeline"] = json!({
        "kind": "composite",
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

#[test]
fn slot_ownership_ps_absence_has_no_filesystem_effects() {
    let fixture = RuntimeFixture::new(leaf_task_manifest(&["exit".into(), "0".into()]));
    let output = fixture.command("ps", &[]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"processes": []})
    );
    assert!(!fixture.state_base.exists());
}

#[test]
fn slot_ownership_rejects_competing_mutations_but_allows_read_only_ps() {
    let tmp = TempDir::new();
    let started = tmp.path.join("started");
    let release = tmp.path.join("release");
    let fixture = RuntimeFixture::new(leaf_task_manifest(&[
        "prepare".into(),
        started.to_str().unwrap().into(),
        release.to_str().unwrap().into(),
    ]));
    let owner = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&started, Duration::from_secs(5)));
    let registry = fixture
        .state_base
        .join("registry/runtime-test/dev/0/registry.sqlite3");
    let counts = || {
        let conn = rusqlite::Connection::open_with_flags(
            &registry,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        conn.query_row(
            "SELECT (SELECT count(*) FROM runs), (SELECT count(*) FROM events), (SELECT count(*) FROM processes)",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        )
        .unwrap()
    };
    // The child marker may precede durable process registration. Establish a
    // committed owner baseline before attributing later writes to contenders.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let before = loop {
        let snapshot = counts();
        if snapshot.2 == 1 {
            break snapshot;
        }
        if std::time::Instant::now() >= deadline {
            fs::write(&release, b"").unwrap();
            let _ = wait_for_child_output(owner, Duration::from_secs(5));
            panic!("owner did not commit its process evidence");
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let ps = fixture.command("ps", &[]).output().unwrap();
    let competitor = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .output()
        .unwrap();
    let clean = fixture.command("clean", &[]).output().unwrap();
    let after = counts();
    // Release before assertions so failure cannot leave the admitted child waiting.
    fs::write(&release, b"").unwrap();
    let owner = wait_for_child_output(owner, Duration::from_secs(5));
    assert!(
        owner.status.success(),
        "{}",
        String::from_utf8_lossy(&owner.stderr)
    );
    assert!(
        ps.status.success(),
        "{}",
        String::from_utf8_lossy(&ps.stderr)
    );
    let report: Value = serde_json::from_slice(&ps.stdout).unwrap();
    let processes = report["processes"].as_array().unwrap();
    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0]["live"], true);
    assert!(processes[0].get("borrowerCount").is_none());
    assert!(processes[0].get("serviceLifetime").is_none());
    assert!(!competitor.status.success());
    assert!(!clean.status.success());
    assert_eq!(
        before, after,
        "read-only ps and losing commands cannot mutate registry evidence"
    );
    assert_eq!(before.0, 1);
}

#[test]
fn removed_state_epoch_rejects_before_slot_or_child_effects() {
    for epoch in [json!("1"), Value::Null] {
        let mut manifest = leaf_task_manifest(&["exit".into(), "0".into()]);
        manifest["state"]["stateEpoch"] = epoch;
        let fixture = RuntimeFixture::new(manifest);
        let result = run(&fixture, &["--task", "smoke", "--output", "json"]);
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("MANIFEST_INVALID"), "{error}");
        assert!(error.contains("stateEpoch"), "{error}");
        assert!(!fixture.state_base.exists());
    }
}

#[test]
fn changed_service_startup_failure_preserves_persistent_data() {
    let mut manifest = task_manifest(&["exit".into(), "0".into()]);
    manifest["state"]["persistence"] = json!("persistent");
    let fixture = RuntimeFixture::new(&manifest);
    let first = run(&fixture, &["--task", "smoke", "--output", "json"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let data = fixture.state_base.join("data/runtime-test/dev/0");
    let sentinel = data.join("application-format");
    fs::write(&sentinel, b"application-owned format").unwrap();
    let marker: Value =
        serde_json::from_slice(&fs::read(data.join(".nixfied-state.json")).unwrap()).unwrap();
    assert!(marker.get("stateEpoch").is_none());
    let program =
        manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"][0].clone();
    manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"] =
        json!([program, "exit", "7"]);
    fs::write(
        &fixture.manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let failed = run(&fixture, &["--task", "smoke", "--output", "json"]);
    assert!(!failed.status.success());
    assert_eq!(fs::read(&sentinel).unwrap(), b"application-owned format");
    let connection = rusqlite::Connection::open_with_flags(
        fixture
            .state_base
            .join("registry/runtime-test/dev/0/registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (starts, cleanups): (i64, i64) = connection
        .query_row(
            "SELECT (SELECT count(*) FROM events WHERE event_type = 'service.starting'),
                (SELECT count(*) FROM cleanups)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        starts, 2,
        "changed configuration must reach application startup"
    );
    assert_eq!(cleanups, 0, "startup failure grants no deletion authority");
}

#[test]
fn removed_service_lifetime_rejects_before_slot_or_child_effects() {
    for obsolete in ["run-scoped", "until-idle", "persistent-until-down"] {
        let mut value = leaf_task_manifest(&["exit".into(), "0".into()]);
        value["tasks"]["smoke"]["serviceLifetime"] = json!(obsolete);
        let fixture = RuntimeFixture::new(value);
        let output = fixture
            .command("run", &["--task", "smoke", "--output", "json"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("MANIFEST_INVALID"), "{error}");
        assert!(error.contains("serviceLifetime"), "{error}");
        assert!(!fixture.state_base.exists());
    }
}

#[test]
fn session_owns_and_stops_services_before_the_next_run() {
    let fixture = RuntimeFixture::new(task_manifest(&["exit".into(), "0".into()]));
    let mut keys = Vec::new();
    for _ in 0..2 {
        let output = fixture
            .command("run", &["--task", "smoke", "--output", "json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let services = report["services"].as_array().unwrap();
        assert_eq!(services.len(), 1);
        keys.push(services[0]["processKey"].as_str().unwrap().to_string());
        let port = services[0]["selectedEndpoint"]["port"].as_u64().unwrap() as u16;
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "completed session retains a listener"
        );
        let ps = fixture.command("ps", &[]).output().unwrap();
        assert!(ps.status.success());
        let rows: Value = serde_json::from_slice(&ps.stdout).unwrap();
        assert!(
            rows["processes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["live"] == false)
        );
    }
    assert_ne!(
        keys[0], keys[1],
        "separate sessions must own separate service processes"
    );
    let connection = rusqlite::Connection::open_with_flags(
        fixture
            .state_base
            .join("registry/runtime-test/dev/0/registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let stopped: i64 = connection.query_row("SELECT count(*) FROM processes WHERE service_instance_id IS NOT NULL AND status = 'stopped'", [], |row| row.get(0)).unwrap();
    assert_eq!(stopped, 2);
    let successful_sessions: i64 = connection
        .query_row(
            "SELECT count(*) FROM runs WHERE execution_outcome = 'succeeded'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        successful_sessions, 2,
        "successor recovery must preserve the first outcome"
    );
    let outcomes_before_teardown: i64 = connection.query_row(
        "SELECT count(*) FROM events outcome JOIN events stopped ON stopped.run_id = outcome.run_id
         WHERE outcome.event_type = 'run.execution-settled' AND stopped.event_type = 'service.stopped'
           AND outcome.seq < stopped.seq", [], |row| row.get(0),
    ).unwrap();
    assert_eq!(outcomes_before_teardown, 2);
}

fn stored_execution_outcome(fixture: &RuntimeFixture) -> String {
    let connection = rusqlite::Connection::open_with_flags(
        fixture
            .state_base
            .join("registry/runtime-test/dev/0/registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    connection
        .query_row("SELECT execution_outcome FROM runs", [], |row| row.get(0))
        .unwrap()
}

fn registry_connection(fixture: &RuntimeFixture) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        fixture
            .state_base
            .join("registry/runtime-test/dev/0/registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

#[test]
fn session_settlement_applies_the_trees_own_retention_after_every_outcome() {
    for persistent in [false, true] {
        for (scenario, expected_exit, expected_outcome) in [
            ("success", 0, "succeeded"),
            ("failure", 30, "failed"),
            ("cancel", 27, "canceled"),
        ] {
            let marker = tempfile_marker("retention");
            let args: Vec<String> = match scenario {
                "success" => vec!["exit".into(), "0".into()],
                "failure" => vec!["exit".into(), "3".into()],
                _ => vec![
                    "output".into(),
                    "hex-block".into(),
                    "".into(),
                    "".into(),
                    marker.to_string_lossy().into_owned(),
                ],
            };
            let mut manifest = leaf_task_manifest(&args);
            if persistent {
                manifest["state"]["persistence"] = json!("persistent");
            }
            let fixture = RuntimeFixture::new(manifest);
            let child = fixture
                .command("run", &["--task", "smoke", "--output", "json"])
                .spawn()
                .unwrap();
            if scenario == "cancel" {
                assert!(wait_for_path(&marker, Duration::from_secs(5)));
                assert_eq!(
                    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
                    0
                );
            }
            let output = wait_for_child_output(child, Duration::from_secs(10));
            assert_eq!(
                output.status.code(),
                Some(expected_exit),
                "{scenario}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let data = fixture.state_base.join("data/runtime-test/dev/0");
            assert_eq!(
                data.join(".nixfied-state.json").is_file(),
                persistent,
                "{scenario}: persistence alone decides whether data survives"
            );
            let connection = registry_connection(&fixture);
            let (outcome, finalization, cleanups, logs): (String, String, i64, i64) = connection
                .query_row(
                    "SELECT execution_outcome, finalization,
                       (SELECT count(*) FROM cleanups WHERE status = 'completed'),
                       (SELECT count(*) FROM events WHERE event_type = 'run.finalized')
                     FROM runs",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            assert_eq!(outcome, expected_outcome, "{scenario}");
            assert_eq!(finalization, "complete", "{scenario}");
            assert_eq!(cleanups, i64::from(!persistent), "{scenario}");
            assert_eq!(logs, 1, "{scenario}");
            let runs = fixture.state_base.join("registry/runtime-test/dev/0/runs");
            let run = fs::read_dir(runs).unwrap().next().unwrap().unwrap().path();
            assert!(
                run.join("logs/task.0.stdout.log").is_file(),
                "{scenario}: retained evidence survives data deletion"
            );
        }
    }
}

#[test]
fn successor_recovery_applies_predecessor_retention_before_a_fresh_session() {
    for persistent in [false, true] {
        let marker = tempfile_marker("killed-owner");
        let block = vec![
            "output".into(),
            "hex-block".into(),
            "".into(),
            "".into(),
            marker.to_string_lossy().into_owned(),
        ];
        let mut manifest = leaf_task_manifest(&block);
        if persistent {
            manifest["state"]["persistence"] = json!("persistent");
        }
        let fixture = RuntimeFixture::new(&manifest);
        let owner = fixture
            .command("run", &["--task", "smoke", "--output", "json"])
            .spawn()
            .unwrap();
        assert!(wait_for_path(&marker, Duration::from_secs(5)));
        let data = fixture.state_base.join("data/runtime-test/dev/0");
        let sentinel = data.join("application-data");
        fs::write(&sentinel, b"written by the killed session").unwrap();
        let generation = |path: &PathBuf| -> String {
            let marker: Value =
                serde_json::from_slice(&fs::read(path.join(".nixfied-state.json")).unwrap())
                    .unwrap();
            marker["dataGeneration"].as_str().unwrap().to_owned()
        };
        let before = generation(&data);
        assert_eq!(
            unsafe { libc::kill(owner.id() as libc::pid_t, libc::SIGKILL) },
            0
        );
        let killed = wait_for_child_output(owner, Duration::from_secs(5));
        assert!(!killed.status.success());

        let program = manifest["tasks"]["smoke"]["invocation"]["run"][0].clone();
        manifest["tasks"]["smoke"]["invocation"]["run"] = json!([program, "exit", "0"]);
        fs::write(
            &fixture.manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let successor = run(&fixture, &["--task", "smoke", "--output", "json"]);
        assert!(
            successor.status.success(),
            "{}",
            String::from_utf8_lossy(&successor.stderr)
        );
        assert_eq!(
            sentinel.exists(),
            persistent,
            "the predecessor's own retention decides its data"
        );
        if persistent {
            assert_eq!(generation(&data), before);
        } else {
            assert!(!data.exists(), "the successor's run-scoped data also ends");
        }
        let connection = registry_connection(&fixture);
        let sessions: Vec<(String, String)> = connection
            .prepare("SELECT execution_outcome, finalization FROM runs ORDER BY rowid")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            sessions,
            [
                ("interrupted".to_string(), "complete".to_string()),
                ("succeeded".to_string(), "complete".to_string()),
            ]
        );
        let recovered: i64 = connection
            .query_row(
                "SELECT count(*) FROM events WHERE event_type = 'run.recovered'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(recovered, 1);
    }
}

fn blocking_session(marker: &std::path::Path) -> RuntimeFixture {
    let mut manifest = leaf_task_manifest(&[
        "output".into(),
        "hex-block".into(),
        "".into(),
        "".into(),
        marker.to_string_lossy().into_owned(),
    ]);
    manifest["tasks"]["smoke"]["invocation"]
        .as_object_mut()
        .unwrap()
        .remove("timeoutMs");
    RuntimeFixture::new(manifest)
}

fn published_session(fixture: &RuntimeFixture) -> (String, i32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let row = rusqlite::Connection::open_with_flags(
            fixture
                .state_base
                .join("registry/runtime-test/dev/0/registry.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT r.run_id, p.pid FROM runs r JOIN processes p ON p.run_id = r.run_id
                     WHERE r.finalization = 'unfinished' AND p.status = 'running'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok()
        });
        if let Some(row) = row {
            return row;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "session never published"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn session_row(fixture: &RuntimeFixture, run_id: &str) -> (Option<String>, String) {
    registry_connection(fixture)
        .query_row(
            "SELECT execution_outcome, finalization FROM runs WHERE run_id = ?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn down_cancels_the_live_session_through_its_own_endpoint_and_observes_settlement() {
    let marker = tempfile_marker("down-live");
    let fixture = blocking_session(&marker);
    let owner = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    let (run_id, _) = published_session(&fixture);
    let down = fixture
        .command("down", &["--timeout-ms", "10000"])
        .output()
        .unwrap();
    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    let report: Value = serde_json::from_slice(&down.stdout).unwrap();
    assert_eq!(
        report,
        json!({"canceledRunId": run_id, "stopped": [], "stale": []}),
        "the owner, not down, performs teardown"
    );
    let owner = wait_for_child_output(owner, Duration::from_secs(5));
    assert_eq!(owner.status.code(), Some(27));
    assert!(String::from_utf8_lossy(&owner.stderr).contains("CANCELED"));
    assert_eq!(
        session_row(&fixture, &run_id),
        (Some("canceled".into()), "complete".into())
    );
    let endpoint = fixture
        .state_base
        .join("registry/runtime-test/dev/0/runs")
        .join(&run_id)
        .join(nixfied_runtime::session_control::CONTROL_FIFO_NAME);
    assert!(
        !endpoint.exists(),
        "the owner removes its endpoint before slot release"
    );
}

#[test]
fn down_recovers_a_dead_owner_under_slot_authority() {
    let marker = tempfile_marker("down-dead");
    let fixture = blocking_session(&marker);
    let owner = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    let (run_id, task_pid) = published_session(&fixture);
    assert_eq!(
        unsafe { libc::kill(owner.id() as libc::pid_t, libc::SIGKILL) },
        0
    );
    let _ = wait_for_child_output(owner, Duration::from_secs(5));
    let down = fixture
        .command("down", &["--timeout-ms", "10000"])
        .output()
        .unwrap();
    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    let report: Value = serde_json::from_slice(&down.stdout).unwrap();
    assert!(report.get("canceledRunId").is_none());
    assert_eq!(report["stopped"].as_array().unwrap().len(), 1);
    assert_ne!(
        unsafe { libc::kill(task_pid, 0) },
        0,
        "recovery settled the orphaned task"
    );
    assert_eq!(
        session_row(&fixture, &run_id),
        (Some("interrupted".into()), "complete".into())
    );
}

#[test]
fn down_times_out_on_a_stopped_owner_without_signaling_it() {
    let marker = tempfile_marker("down-stopped");
    let fixture = blocking_session(&marker);
    let owner = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    let (run_id, task_pid) = published_session(&fixture);
    let owner_pid = owner.id() as libc::pid_t;
    assert_eq!(unsafe { libc::kill(owner_pid, libc::SIGSTOP) }, 0);
    let down = fixture
        .command("down", &["--timeout-ms", "300"])
        .output()
        .unwrap();
    let alive = unsafe { libc::kill(task_pid, 0) } == 0;
    assert_eq!(unsafe { libc::kill(owner_pid, libc::SIGCONT) }, 0);
    assert_eq!(down.status.code(), Some(31));
    let error = String::from_utf8_lossy(&down.stderr);
    assert!(error.contains("LIFECYCLE_FAILED"), "{error}");
    assert!(alive, "a timeout never tears down the owner's children");
    // The buffered request reaches the resumed owner; it finalizes itself.
    let owner = wait_for_child_output(owner, Duration::from_secs(5));
    assert_eq!(owner.status.code(), Some(27));
    assert_eq!(
        session_row(&fixture, &run_id),
        (Some("canceled".into()), "complete".into())
    );
}

#[test]
fn an_old_session_request_never_reaches_its_successor() {
    let marker = tempfile_marker("down-successor");
    let fixture = blocking_session(&marker);
    let first = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    let (first_run, _) = published_session(&fixture);
    let old_endpoint = fixture
        .state_base
        .join("registry/runtime-test/dev/0/runs")
        .join(&first_run);
    assert_eq!(
        nixfied_runtime::session_control::request_cancellation(&old_endpoint).unwrap(),
        nixfied_runtime::session_control::CancellationDelivery::Requested
    );
    let _ = wait_for_child_output(first, Duration::from_secs(5));
    fs::remove_file(&marker).unwrap();
    let successor = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    let (second_run, _) = published_session(&fixture);
    assert_ne!(first_run, second_run);
    // A requester that paused across the old owner's exit reaches no reader.
    assert_eq!(
        nixfied_runtime::session_control::request_cancellation(&old_endpoint).unwrap(),
        nixfied_runtime::session_control::CancellationDelivery::Unavailable
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        session_row(&fixture, &second_run).1,
        "unfinished",
        "the successor keeps running"
    );
    let down = fixture.command("down", &[]).output().unwrap();
    assert!(down.status.success());
    let successor = wait_for_child_output(successor, Duration::from_secs(5));
    assert_eq!(successor.status.code(), Some(27));
}

#[test]
fn unexpected_service_exit_zero_fails_the_session_and_still_settles() {
    let task_marker = tempfile_marker("exit-zero-task");
    let service_marker = tempfile_marker("exit-zero-service");
    let mut manifest = task_manifest(&[
        "output".into(),
        "hex-block".into(),
        "".into(),
        "".into(),
        task_marker.to_string_lossy().into_owned(),
    ]);
    manifest["tasks"]["smoke"]["invocation"]
        .as_object_mut()
        .unwrap()
        .remove("timeoutMs");
    let program =
        manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"][0].clone();
    manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"] = json!([
        program,
        "listen",
        "127.0.0.1",
        "${port}",
        "exit-zero-on-marker",
        service_marker.to_string_lossy()
    ]);
    let fixture = RuntimeFixture::new(manifest);
    let child = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&task_marker, Duration::from_secs(5)));
    fs::write(&service_marker, b"exit").unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(6));
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("DEPENDENCY_UNAVAILABLE"), "{error}");
    let connection = registry_connection(&fixture);
    let (outcome, finalization, service, ownership): (String, String, String, i64) = connection
        .query_row(
            "SELECT r.execution_outcome, r.finalization,
               (SELECT status FROM processes WHERE role = 'service'),
               (SELECT count(*) FROM processes WHERE ownership = 'unresolved')
             FROM runs r",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        outcome, "failed",
        "exit zero is not a successful task result"
    );
    assert_eq!(service, "failed");
    assert_eq!(ownership, 0);
    assert_eq!(finalization, "complete");
    assert!(
        !fixture.state_base.join("data/runtime-test/dev/0").exists(),
        "a settled failure still applies run-scoped retention"
    );
}

#[test]
fn service_failure_during_another_services_preparation_releases_no_further_workload() {
    let port = available_port_window(2);
    let exit_marker = tempfile_marker("prepare-dependency-exit");
    let release = tempfile_marker("prepare-release");
    let executable = test_child();
    let executable = executable.to_str().unwrap();
    let mut manifest = synthetic_manifest(
        executable,
        &[
            "listen",
            "127.0.0.1",
            "${port}",
            "exit-zero-on-marker",
            exit_marker.to_str().unwrap(),
        ],
        port,
        port + 1,
    );
    let program = manifest["tasks"]["smoke"]["invocation"]["run"][0].clone();
    let mut second = manifest["services"]["synthetic"].clone();
    for class in ["start", "ready", "health", "stop", "clean"] {
        second["lifecycle"][class]["operationId"] = json!(format!("service.second.{class}"));
    }
    second["lifecycle"]["start"]["invocation"]["run"] =
        json!([program.clone(), "listen", "127.0.0.1", "${port}", "hold"]);
    second["lifecycle"]["prepare"] = json!({"task": "prep"});
    second["endpoints"] = json!({"second-tcp": {"endpointId": "second-tcp", "host": "127.0.0.1"}});
    second["primaryEndpoint"] = json!("second-tcp");
    second["logRefs"] = json!(["service.second"]);
    manifest["services"]["second"] = second;
    let mut prep = manifest["tasks"]["smoke"].clone();
    prep["operationId"] = json!("task.prep.run");
    prep["logRefs"] = json!(["task.prep"]);
    // The prepare-only dependency fails while preparation waits without a deadline.
    prep["invocation"]["run"] = json!([program.clone(), "prepare", exit_marker, release]);
    prep["invocation"]
        .as_object_mut()
        .unwrap()
        .remove("timeoutMs");
    manifest["tasks"]["prep"] = prep;
    manifest["tasks"]["smoke"]["requires"] = json!(["second"]);
    manifest["tasks"]["smoke"]["invocation"]["run"] = json!([program, "exit", "0"]);
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("DEPENDENCY_UNAVAILABLE"), "{error}");
    let connection = registry_connection(&fixture);
    let (outcome, finalization, second_started, unresolved): (String, String, i64, i64) =
        connection
            .query_row(
                "SELECT r.execution_outcome, r.finalization,
                   (SELECT count(*) FROM processes WHERE service_name = 'second' AND role = 'service'),
                   (SELECT count(*) FROM processes WHERE ownership = 'unresolved')
                 FROM runs r",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(outcome, "failed");
    assert_eq!(
        second_started, 0,
        "no workload is released after an owned service failed"
    );
    assert_eq!(unresolved, 0, "the interrupted prepare task settled");
    assert_eq!(finalization, "complete");
    assert!(!release.exists());
}
