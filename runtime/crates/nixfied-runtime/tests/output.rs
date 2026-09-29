//! End-to-end coverage for the direct-leaf task-output projection.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Output};
use std::time::Duration;

use common::*;
use nixfied_runtime::redaction::REDACTION_TOKEN;
use serde_json::{Value, json};

fn task_manifest(args: &[&str]) -> Value {
    task_manifest_at(args, available_port_window(1))
}

fn leaf_task_manifest(args: &[&str]) -> Value {
    // Unused service metadata needs no host port observation.
    let mut manifest = task_manifest_at(args, 23180);
    manifest["tasks"]["smoke"]["requires"] = json!([]);
    manifest
}

fn task_manifest_at(args: &[&str], port: u16) -> Value {
    let mut manifest = test_child_manifest(port, port);
    set_task_run_args(&mut manifest, args);
    manifest
}

/// A smoke task without a deadline that writes `stdout`/`stderr`, touches
/// `marker`, and blocks until canceled.
fn blocking_manifest(marker: &Path, service: bool, stdout: &[u8], stderr: &[u8]) -> Value {
    let args = [
        "output",
        "hex-block",
        &hex::encode(stdout),
        &hex::encode(stderr),
        marker.to_str().unwrap(),
    ];
    let mut manifest = if service {
        task_manifest(&args)
    } else {
        leaf_task_manifest(&args)
    };
    clear_task_deadline(&mut manifest, "smoke");
    manifest
}

fn blocking_session(marker: &Path) -> RuntimeFixture {
    RuntimeFixture::new(blocking_manifest(marker, false, b"", b""))
}

fn set_task_default_output(manifest: &mut Value, output: &str) {
    manifest["tasks"]["smoke"]["defaultOutput"] = json!(output);
}

fn stored_execution_outcome(fixture: &RuntimeFixture) -> String {
    fixture
        .registry()
        .query_row("SELECT execution_outcome FROM runs", [], |row| row.get(0))
        .unwrap()
}

/// A session's `(execution_outcome, finalization)` once its row is visible.
fn run_row(fixture: &RuntimeFixture, run_id: &str) -> Option<(Option<String>, String)> {
    try_registry_ro(&fixture.state_base)?
        .query_row(
            "SELECT execution_outcome, finalization FROM runs WHERE run_id = ?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
}

fn session_row(fixture: &RuntimeFixture, run_id: &str) -> (Option<String>, String) {
    run_row(fixture, run_id).expect("session row should exist")
}

/// The live session and its running task's pid.
fn published_session(fixture: &RuntimeFixture) -> (String, i32) {
    poll_until(Duration::from_secs(5), "the session to publish", || {
        try_registry_ro(&fixture.state_base)?
            .query_row(
                "SELECT r.run_id, p.pid FROM runs r JOIN processes p ON p.run_id = r.run_id
                 WHERE r.finalization = 'unfinished' AND p.status = 'running'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()
    })
}

/// Retry `operation` until it succeeds, returning the last attempt: slot
/// release is asynchronous to the stalled presentation.
fn output_eventually(fixture: &RuntimeFixture, operation: &str, extra: &[&str]) -> Output {
    let mut last = None;
    let succeeded = poll(Duration::from_secs(10), || {
        let output = fixture.output(operation, extra);
        if output.status.success() {
            return Some(output);
        }
        last = Some(output);
        None
    });
    succeeded.unwrap_or_else(|| last.expect("the operation ran at least once"))
}

/// Spawn the session and wait until its blocking task is live.
fn spawn_live_session(fixture: &RuntimeFixture, marker: &Path) -> (Child, String, i32) {
    let child = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(marker, Duration::from_secs(5)));
    let (run_id, pid) = published_session(fixture);
    (child, run_id, pid)
}

fn wait_for_settled(fixture: &RuntimeFixture, run_id: &str) -> Option<String> {
    poll_until(Duration::from_secs(10), "the session to settle", || {
        run_row(fixture, run_id)
            .filter(|(_, finalization)| finalization == "complete")
            .map(|(outcome, _)| outcome)
    })
}

/// Wait until the registry answers `query` with true; a stalled caller never
/// delays settlement or the output seal.
fn wait_for_registry(fixture: &RuntimeFixture, query: &str) {
    poll_until(Duration::from_secs(20), query, || {
        try_registry_ro(&fixture.state_base)?
            .query_row(query, [], |row| row.get::<_, bool>(0))
            .ok()?
            .then_some(())
    })
}

const SEALED_SETTLEMENT: &str = "SELECT finalization = 'complete' AND output = 'sealed' FROM runs";

fn assert_stream_contains(haystack: &[u8], needle: &[u8], stream: &str) {
    assert!(
        needle.is_empty()
            || haystack
                .windows(needle.len())
                .any(|window| window == needle),
        "{stream} did not contain expected bytes\nexpected: {needle:?}\nactual: {haystack:?}"
    );
}

#[test]
fn escaped_idle_and_continuous_writers_cannot_hold_capture_or_publish_evidence() {
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
                    let args = [
                        "output",
                        "escaped-writer",
                        activity,
                        pid_path.to_str().unwrap(),
                        acknowledgement.to_str().unwrap(),
                        &hex::encode(prefix),
                        &hex::encode(prefix),
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
                    let survivor = Survivor(wait_for_pid_file(&pid_path).try_into().unwrap());
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
                    let connection = fixture.registry();
                    let observed: (String, i32) = connection.query_row(
                "SELECT execution_outcome, exit_code FROM processes WHERE role = 'task'",
                [], |row| Ok((row.get(0)?, row.get(1)?)),
            ).unwrap();
                    let (capture, sealed): (String, String) = connection
                        .query_row(
                            "SELECT p.capture, r.output FROM processes p JOIN runs r USING (run_id)
                             WHERE p.role = 'task'",
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
        let mut manifest = leaf_task_manifest(&["prepare", "child-started"]);
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
        let output = fixture.output("run", &["--task", "smoke", "--output", "task-output"]);
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
    let mut manifest = leaf_task_manifest(&["output", "env", "VALUE"]);
    manifest["tasks"]["smoke"]["invocation"]["env"]["VALUE"] = json!("${HOME:-${stateDir}}");
    let mut fixture = RuntimeFixture::new(manifest);
    fixture.state_base = fixture.tmp.path.join("state-${port:unresolved}");
    let output = fixture.output("run", &["--task", "smoke", "--output", "task-output"]);
    assert_success(&output);
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
    let mut manifest = leaf_task_manifest(&["exit", "0"]);
    manifest["secrets"]["token"] = json!({"secretId":"token","source":{
        "kind":"env-var","envVar":"NIXFIED_OUTPUT_VIEW_SECRET"
    }});
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .env("NIXFIED_OUTPUT_VIEW_SECRET", "smoke")
        .output()
        .unwrap();
    assert_success(&output);
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
fn successful_presentation_follows_the_selected_or_default_output() {
    let binary = [0_u8, 1, 2, 0, 0xff, b'\n'];
    let binary_stderr = b"stderr-without-final-newline\0";
    let hex_task = |stdout: &[u8], stderr: &[u8]| {
        leaf_task_manifest(&["output", "hex", &hex::encode(stdout), &hex::encode(stderr)])
    };
    let direct = task_manifest(&[
        "output",
        "hex",
        &hex::encode(binary),
        &hex::encode(binary_stderr),
    ]);
    let mut omitted = hex_task(b"default stdout", b"default stderr");
    set_task_default_output(&mut omitted, "task-output");
    let mut accepted = task_manifest(&[
        "output",
        "hex-exit",
        &hex::encode(b"accepted nonzero\0"),
        &hex::encode(b"diagnostic"),
        "7",
    ]);
    accepted["tasks"]["smoke"]["exitPolicy"]["successCodes"] = json!([0, 7]);
    let mut overridden = leaf_task_manifest(&["exit", "0"]);
    set_task_default_output(&mut overridden, "task-output");
    let mut composite = task_manifest(&["exit", "0"]);
    set_task_default_output(&mut composite, "task-output");
    composite["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "defaultOutput": "summary",
        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });
    let task_output: &[&str] = &["--task", "smoke", "--output", "task-output"];
    for (case, manifest, args, stdout, stderr) in [
        (
            "exact binary without metadata",
            direct,
            task_output,
            &binary[..],
            &binary_stderr[..],
        ),
        (
            "omitted output uses the leaf default",
            omitted,
            &["--task", "smoke"][..],
            b"default stdout",
            b"default stderr",
        ),
        (
            "accepted nonzero code",
            accepted,
            task_output,
            b"accepted nonzero\0",
            b"diagnostic",
        ),
        (
            "empty output is a zero-byte presentation",
            hex_task(b"", b""),
            task_output,
            b"",
            b"",
        ),
        (
            "explicit summary overrides the leaf default",
            overridden,
            &["--task", "smoke", "--output", "summary"][..],
            b"",
            b"result: ok",
        ),
        (
            "a composite uses its own default, not a child's",
            composite,
            &["--task", "pipeline"][..],
            b"",
            b"",
        ),
    ] {
        let output = RuntimeFixture::new(manifest).output("run", args);
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, stdout, "{case}");
        assert_stream_contains(&output.stderr, stderr, case);
    }
}

#[test]
fn large_simultaneous_streams_present_exactly() {
    let stdout = vec![0x61; 256 * 1024];
    let stderr = vec![0x7a; 192 * 1024];
    let args = [
        "output",
        "repeat",
        "61",
        &stdout.len().to_string(),
        "7a",
        &stderr.len().to_string(),
    ];
    let manifest = leaf_task_manifest(&args);
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "smoke", "--output", "task-output"]);

    assert_success(&output);
    assert_eq!(output.stdout, stdout);
    // Live presentation merges runtime diagnostics into stderr wherever the
    // captured stderr had reached, possibly inside the task's bytes.
    let diagnostics =
        fs::read(find_named(&fixture.state_base, "diagnostics.log").unwrap()).unwrap();
    assert!(
        interleaves(&output.stderr, &diagnostics, &stderr),
        "stderr is not exactly the diagnostics interleaved with the task stderr"
    );
}

/// Whether `merged` is exactly `first` and `second` interleaved, each in order.
fn interleaves(merged: &[u8], first: &[u8], second: &[u8]) -> bool {
    if merged.len() != first.len() + second.len() {
        return false;
    }
    // Every count of `first` bytes that can explain the merged prefix.
    let mut taken = vec![0_usize];
    for (position, byte) in merged.iter().enumerate() {
        let mut next = Vec::new();
        for &from_first in &taken {
            if first.get(from_first) == Some(byte) {
                next.push(from_first + 1);
            }
            if second.get(position - from_first) == Some(byte) {
                next.push(from_first);
            }
        }
        next.sort_unstable();
        next.dedup();
        if next.is_empty() {
            return false;
        }
        taken = next;
    }
    true
}

#[test]
fn broken_stdout_pipe_is_typed_and_does_not_stop_stderr_delivery() {
    let stdout = vec![b'x'; 128 * 1024];
    let stderr = vec![b'z'; 32 * 1024];
    let args = [
        "output",
        "repeat",
        "78",
        &stdout.len().to_string(),
        "7a",
        &stderr.len().to_string(),
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
    // The command's own error document follows the presented streams.
    let diagnostics =
        fs::read(find_named(&fixture.state_base, "diagnostics.log").unwrap()).unwrap();
    let presented = output
        .stderr
        .get(..diagnostics.len() + stderr.len())
        .unwrap();
    assert!(
        interleaves(presented, &diagnostics, &stderr),
        "stderr is not exactly the diagnostics interleaved with the task stderr"
    );
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("OUTPUT_PROJECTION_FAILED"));
    assert!(diagnostic.contains("broken-pipe"));
    assert_eq!(stored_execution_outcome(&fixture), "succeeded");
}

#[test]
fn task_failure_presents_captured_bytes_and_preserves_status() {
    let stdout = b"failed stdout";
    let stderr = b"failed stderr";
    let args = [
        "output",
        "hex-exit",
        &hex::encode(stdout),
        &hex::encode(stderr),
        "7",
    ];
    let fixture = RuntimeFixture::new(task_manifest(&args));
    let output = fixture.output("run", &["--task", "smoke", "--output", "task-output"]);

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
fn redaction_happens_before_task_output_presentation() {
    let mut manifest = leaf_task_manifest(&["output", "env", "TOKEN"]);
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

    assert_success(&output);
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
fn timeout_presents_output_before_reporting_task_failure() {
    let marker = temp_marker("nixfied-output-timeout");
    let stdout = b"timeout stdout";
    let stderr = b"timeout stderr";
    let mut manifest = blocking_manifest(&marker, false, stdout, stderr);
    manifest["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(150);
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "smoke", "--output", "task-output"]);

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

/// A missing deadline lowers to none (see `execution::lower`); only
/// cancellation ends this task, after its output is presented.
#[test]
fn canceled_task_presents_output_before_reporting_cancellation() {
    let marker = temp_marker("nixfied-output-cancel");
    let stdout = b"cancel stdout";
    let stderr = b"cancel stderr";
    let fixture = RuntimeFixture::new(blocking_manifest(&marker, false, stdout, stderr));
    let child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .expect("runtime command should spawn");
    assert!(wait_for_path(&marker, Duration::from_secs(3)));
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

/// A service that is killed or exits zero while a task without a deadline
/// runs fails the session, which still settles with run-scoped retention.
#[test]
fn unexpected_service_exit_fails_the_session_and_still_settles() {
    for (exit_zero, unrelated) in [(false, false), (false, true), (true, false)] {
        let task_marker = temp_marker("nixfied-output-dependency-exit");
        let service_marker = temp_marker("nixfied-output-exit-zero-service");
        let mut manifest = blocking_manifest(&task_marker, true, b"", b"");
        if exit_zero {
            let program =
                manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"][0]
                    .clone();
            manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"] = json!([
                program,
                "listen",
                "127.0.0.1",
                "${port}",
                "exit-zero-on-marker",
                service_marker
            ]);
        }
        let selected = if unrelated {
            add_task_clone(&mut manifest, "first", &["synthetic"], &["exit", "0"]);
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
        assert!(wait_for_path(&task_marker, Duration::from_secs(5)));
        if exit_zero {
            fs::write(&service_marker, b"exit").unwrap();
        } else {
            let pid: i32 = fixture
                .registry()
                .query_row(
                    "SELECT pid FROM processes WHERE role = 'service'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        }
        let output = wait_for_child_output(child, Duration::from_secs(6));
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("DEPENDENCY_UNAVAILABLE"), "{error}");
        let (outcome, finalization, service, unresolved): (String, String, String, i64) = fixture
            .registry()
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
            "an unexpected exit is not a successful task result"
        );
        assert_eq!(service, "failed");
        assert_eq!(unresolved, 0);
        assert_eq!(finalization, "complete");
        assert!(
            !fixture.state_base.join("data/runtime-test/dev/0").exists(),
            "a settled failure still applies run-scoped retention"
        );
    }
}

#[test]
fn services_with_exec_probes_keep_distinct_capture_files() {
    let port = available_port_window(2);
    let mut manifest = task_manifest_at(&["exit", "0"], port);
    manifest["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
    let invocation = manifest["tasks"]["smoke"]["invocation"].clone();
    manifest["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["readyProbe"] = invocation;
    manifest["services"]["synthetic"]["lifecycle"]["ready"]["policy"] =
        json!({"timeoutMs": 1000, "retryIntervalMs": 100, "maxAttempts": 1});
    add_service_clone(&mut manifest, "later", LISTEN_HOLD, &["synthetic"]);
    manifest["tasks"]["smoke"]["requires"] = json!(["later"]);
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "smoke", "--output", "json"]);
    assert_success(&output);
    for service in ["synthetic", "later"] {
        assert!(
            find_named(
                &fixture.state_base,
                &format!("lifecycle.{service}.ready.probe.0.stdout.log")
            )
            .is_some()
        );
    }
    let probes: i64 = fixture.registry().query_row("SELECT count(*) FROM processes WHERE role='probe' AND status='succeeded' AND execution_outcome='succeeded'", [], |row| row.get(0)).unwrap();
    assert_eq!(probes, 4);
}

#[test]
fn service_failure_interrupts_another_services_exec_probe() {
    for phase in ["ready", "health"] {
        for victim in ["synthetic", "later"] {
            let probe_marker = temp_marker("nixfied-output-probe-observation");
            let task_marker = temp_marker("nixfied-output-unreleased-task");
            let port = available_port_window(2);
            let mut manifest = task_manifest_at(&["prepare", task_marker.to_str().unwrap()], port);
            manifest["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
            add_service_clone(&mut manifest, "later", LISTEN_HOLD, &["synthetic"]);
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
            manifest["services"]["later"]["endpoints"]["later-tcp"][if phase == "ready" {
                "readyProbe"
            } else {
                "healthProbe"
            }] = probe_invocation;
            manifest["services"]["later"]["lifecycle"][phase]["policy"] =
                json!({"timeoutMs": 30000, "retryIntervalMs": 100, "maxAttempts": 1});
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
            let pid: i32 = fixture
                .registry()
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

/// Settlement and slot release under stalled readers are proven below; an
/// interrupted delivery after settlement is an output failure alone. A signal
/// that lands after the session's last cancellation observation is never lost,
/// whether it arrives before or after slot release.
#[test]
fn interrupted_delivery_after_settlement_fails_output_not_the_session() {
    for before_release in [true, false] {
        let count = 4 * 1024 * 1024;
        let manifest = task_manifest(&["output", "repeat", "78", &count.to_string(), "79", "0"]);
        let fixture = RuntimeFixture::new(manifest);
        let mut child = fixture
            .command("run", &["--task", "smoke", "--output", "task-output"])
            .spawn()
            .unwrap();
        // Hold the caller's stdout open without ever reading it.
        let stalled = child.stdout.take().unwrap();
        wait_for_registry(&fixture, SEALED_SETTLEMENT);
        if !before_release {
            // The interrupt lands once the slot is released and only presentation remains.
            assert_success(&output_eventually(&fixture, "clean", &[]));
            assert!(
                child.try_wait().unwrap().is_none(),
                "the command keeps presenting after settlement"
            );
        }
        assert_eq!(
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        let output = wait_for_child_output(child, Duration::from_secs(10));
        drop(stalled);
        assert_eq!(
            output.status.code(),
            Some(38),
            "before release: {before_release}"
        );
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
}

#[test]
fn live_task_output_arrives_before_the_task_finishes_and_down_ends_it() {
    use std::io::Read;
    let marker = temp_marker("nixfied-output-live-output");
    let fixture = RuntimeFixture::new(blocking_manifest(
        &marker,
        false,
        b"live stdout\n",
        b"live stderr\n",
    ));
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
    let down = fixture.output("down", &[]);
    assert_success(&down);
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
        "output",
        "hex",
        &hex::encode(b"first\nsecond"),
        &hex::encode(b"problem\n"),
    ]);
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "smoke", "--output", "summary"]);
    assert_success(&output);
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

#[test]
fn invalid_selection_is_rejected_before_state_or_child_side_effects() {
    let mut manifest = task_manifest(&["exit", "0"]);
    manifest["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "steps": { "only": { "task": "smoke" } }
    });
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "pipeline", "--output", "task-output"]);

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
    let manifest = task_manifest(&["exit", "0"]);

    let missing = RuntimeFixture::new(manifest.clone());
    let output = missing.output("run", &["--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(output.stdout.is_empty());
    assert!(!missing.state_base.exists());

    let unknown = RuntimeFixture::new(manifest.clone());
    let output = unknown.output("run", &["--task", "missing", "--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(!unknown.state_base.exists());

    let repeated = RuntimeFixture::new(manifest.clone());
    let output = repeated.output(
        "run",
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
        let output = alias.output("run", &args);
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
        let fixture = RuntimeFixture::new(task_manifest(&["exit", "0"]));
        let output = fixture.output("run", &args);
        assert_eq!(output.status.code(), Some(36), "args {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.state_base.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("OUTPUT_MODE_CONFLICT"));
    }
}

#[test]
fn non_utf8_environment_secret_never_reaches_diagnostics() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let marker_dir = TempDir::new();
    let marker = marker_dir.path.join("child-started");
    let mut manifest = task_manifest(&["prepare", marker.to_str().unwrap()]);
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
            let error = stderr_json(&output.stderr);
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
    let fixture = RuntimeFixture::new(test_child_manifest(bounds[0], bounds[0]));
    let output = fixture.output("check", &[]);
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
        "overlaps the host ephemeral port range {}-{}",
        bounds[0], bounds[1]
    )));
    assert!(!fixture.state_base.exists());
}

#[test]
fn slot_ownership_ps_absence_has_no_filesystem_effects() {
    let fixture = RuntimeFixture::new(leaf_task_manifest(&["exit", "0"]));
    let output = fixture.output("ps", &[]);
    assert_success(&output);
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
        "prepare",
        started.to_str().unwrap(),
        release.to_str().unwrap(),
    ]));
    let owner = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .spawn()
        .unwrap();
    assert!(wait_for_path(&started, Duration::from_secs(5)));
    let counts = || {
        fixture.registry().query_row(
            "SELECT (SELECT count(*) FROM runs), (SELECT count(*) FROM events), (SELECT count(*) FROM processes)",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        )
        .unwrap()
    };
    // The child marker may precede durable process registration. Establish a
    // committed owner baseline before attributing later writes to contenders.
    let Some(before) = poll(Duration::from_secs(5), || {
        Some(counts()).filter(|snapshot| snapshot.2 == 1)
    }) else {
        fs::write(&release, b"").unwrap();
        let _ = wait_for_child_output(owner, Duration::from_secs(5));
        panic!("owner did not commit its process evidence");
    };
    let ps = fixture.output("ps", &[]);
    let competitor = fixture.output("run", &["--task", "smoke", "--output", "json"]);
    let clean = fixture.output("clean", &[]);
    let after = counts();
    // Release before assertions so failure cannot leave the admitted child waiting.
    fs::write(&release, b"").unwrap();
    let owner = wait_for_child_output(owner, Duration::from_secs(5));
    assert_success(&owner);
    assert_success(&ps);
    let report: Value = serde_json::from_slice(&ps.stdout).unwrap();
    let processes = report["processes"].as_array().unwrap();
    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0]["live"], true);
    assert!(!competitor.status.success());
    assert!(!clean.status.success());
    assert_eq!(
        before, after,
        "read-only ps and losing commands cannot mutate registry evidence"
    );
    assert_eq!(before.0, 1);
}

/// Wire proofs for every closed record live in `manifest_contract`; the CLI
/// refuses an unknown field before any slot, state, or child effect.
#[test]
fn unknown_manifest_field_rejects_before_slot_or_child_effects() {
    let mut manifest = leaf_task_manifest(&["prepare", "child-started"]);
    manifest["tasks"]["smoke"]["serviceLifetime"] = json!("run-scoped");
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "smoke", "--output", "json"]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("MANIFEST_INVALID"), "{error}");
    assert!(error.contains("serviceLifetime"), "{error}");
    assert!(!fixture.state_base.exists());
    assert!(!fixture.tmp.path.join("child-started").exists());
}

#[test]
fn changed_service_startup_failure_preserves_persistent_data() {
    let mut manifest = task_manifest(&["exit", "0"]);
    manifest["state"]["persistence"] = json!("persistent");
    let fixture = RuntimeFixture::new(&manifest);
    let first = fixture.output("run", &["--task", "smoke", "--output", "json"]);
    assert_success(&first);
    let data = fixture.state_base.join("data/runtime-test/dev/0");
    let sentinel = data.join("application-format");
    fs::write(&sentinel, b"application-owned format").unwrap();
    let program =
        manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"][0].clone();
    manifest["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"] =
        json!([program, "exit", "7"]);
    fs::write(
        &fixture.manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let failed = fixture.output("run", &["--task", "smoke", "--output", "json"]);
    assert!(!failed.status.success());
    assert_eq!(fs::read(&sentinel).unwrap(), b"application-owned format");
    let connection = fixture.registry();
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
fn session_owns_and_stops_services_before_the_next_run() {
    let fixture = RuntimeFixture::new(task_manifest(&["exit", "0"]));
    let mut keys = Vec::new();
    for _ in 0..2 {
        let output = fixture.output("run", &["--task", "smoke", "--output", "json"]);
        assert_success(&output);
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let services = report["services"].as_array().unwrap();
        assert_eq!(services.len(), 1);
        keys.push(services[0]["processKey"].as_str().unwrap().to_string());
        let port = services[0]["selectedEndpoint"]["port"].as_u64().unwrap() as u16;
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "completed session retains a listener"
        );
        let ps = fixture.output("ps", &[]);
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
    let connection = fixture.registry();
    let stopped: i64 = connection
        .query_row(
            "SELECT count(*) FROM processes WHERE role = 'service' AND status = 'stopped'",
            [],
            |row| row.get(0),
        )
        .unwrap();
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

#[test]
fn session_settlement_applies_the_trees_own_retention_after_every_outcome() {
    for persistent in [false, true] {
        for (scenario, expected_exit, expected_outcome) in [
            ("success", 0, "succeeded"),
            ("failure", 30, "failed"),
            ("cancel", 27, "canceled"),
        ] {
            let marker = temp_marker("nixfied-output-retention");
            let mut manifest = match scenario {
                "success" => leaf_task_manifest(&["exit", "0"]),
                "failure" => leaf_task_manifest(&["exit", "3"]),
                _ => blocking_manifest(&marker, false, b"", b""),
            };
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
            let connection = fixture.registry();
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
        let marker = temp_marker("nixfied-output-killed-owner");
        let mut manifest = blocking_manifest(&marker, false, b"", b"");
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

        set_task_run_args(&mut manifest, &["exit", "0"]);
        fs::write(
            &fixture.manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let successor = fixture.output("run", &["--task", "smoke", "--output", "json"]);
        assert_success(&successor);
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
        let connection = fixture.registry();
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

#[test]
fn down_cancels_the_live_session_through_its_own_endpoint_and_observes_settlement() {
    let marker = temp_marker("nixfied-output-down-live");
    let fixture = blocking_session(&marker);
    let (owner, run_id, _) = spawn_live_session(&fixture, &marker);
    let down = fixture.output("down", &["--timeout-ms", "10000"]);
    assert_success(&down);
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
    let marker = temp_marker("nixfied-output-down-dead");
    let fixture = blocking_session(&marker);
    let (owner, run_id, task_pid) = spawn_live_session(&fixture, &marker);
    assert_eq!(
        unsafe { libc::kill(owner.id() as libc::pid_t, libc::SIGKILL) },
        0
    );
    let _ = wait_for_child_output(owner, Duration::from_secs(5));
    let down = fixture.output("down", &["--timeout-ms", "10000"]);
    assert_success(&down);
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
    let marker = temp_marker("nixfied-output-down-stopped");
    let fixture = blocking_session(&marker);
    let (owner, run_id, task_pid) = spawn_live_session(&fixture, &marker);
    let owner_pid = owner.id() as libc::pid_t;
    assert_eq!(unsafe { libc::kill(owner_pid, libc::SIGSTOP) }, 0);
    let down = fixture.output("down", &["--timeout-ms", "300"]);
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
    let marker = temp_marker("nixfied-output-down-successor");
    let fixture = blocking_session(&marker);
    let (first, first_run, _) = spawn_live_session(&fixture, &marker);
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
    let (successor, second_run, _) = spawn_live_session(&fixture, &marker);
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
    let down = fixture.output("down", &[]);
    assert!(down.status.success());
    let successor = wait_for_child_output(successor, Duration::from_secs(5));
    assert_eq!(successor.status.code(), Some(27));
}

#[test]
fn service_failure_during_another_services_preparation_releases_no_further_workload() {
    let port = available_port_window(2);
    let exit_marker = temp_marker("nixfied-output-prepare-dependency-exit");
    let release = temp_marker("nixfied-output-prepare-release");
    let mut manifest = test_child_service(
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
    add_service_clone(&mut manifest, "second", LISTEN_HOLD, &[]);
    manifest["services"]["second"]["lifecycle"]["prepare"] = json!({"task": "prep"});
    // The prepare-only dependency fails while preparation waits without a deadline.
    add_task_clone(
        &mut manifest,
        "prep",
        &["synthetic"],
        &[
            "prepare",
            exit_marker.to_str().unwrap(),
            release.to_str().unwrap(),
        ],
    );
    clear_task_deadline(&mut manifest, "prep");
    manifest["tasks"]["smoke"]["requires"] = json!(["second"]);
    set_task_run_args(&mut manifest, &["exit", "0"]);
    let fixture = RuntimeFixture::new(manifest);
    let output = fixture.output("run", &["--task", "smoke", "--output", "json"]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("DEPENDENCY_UNAVAILABLE"), "{error}");
    let connection = fixture.registry();
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
    let rows: Vec<(String, String, String, String)> = connection
        .prepare("SELECT role, status, ownership, capture FROM processes")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        unresolved, 0,
        "the interrupted prepare task settled: {rows:?}"
    );
    assert_eq!(finalization, "complete");
    assert!(!release.exists());
}

#[test]
fn background_launch_acknowledges_establishment_not_task_success() {
    let marker = temp_marker("nixfied-output-daemon-live");
    let fixture = blocking_session(&marker);
    let started = std::time::Instant::now();
    let launch = fixture.output("run", &["--task", "smoke", "--daemon"]);
    assert_success(&launch);
    assert!(started.elapsed() < Duration::from_secs(10));
    let acknowledgement: Value = serde_json::from_slice(&launch.stdout).unwrap();
    let run_id = acknowledgement["runId"].as_str().unwrap().to_owned();
    let run_dir = PathBuf::from(acknowledgement["runDir"].as_str().unwrap());
    assert!(run_dir.join("diagnostics.log").is_file());
    assert_eq!(acknowledgement["logsDir"], json!(run_dir.join("logs")));
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    assert_eq!(
        run_row(&fixture, &run_id),
        Some((None, "unfinished".into())),
        "acknowledgement never claims task completion"
    );
    let owner: i32 = fixture
        .registry()
        .query_row(
            "SELECT json_extract(owner_identity, '$.pid') FROM runs WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(
        unsafe { libc::getsid(owner) },
        unsafe { libc::getsid(0) },
        "the owner leads its own OS session"
    );
    #[cfg(target_os = "linux")]
    for fd in 0..3 {
        assert_eq!(
            fs::read_link(format!("/proc/{owner}/fd/{fd}")).unwrap(),
            PathBuf::from("/dev/null"),
            "the owner holds no caller terminal or pipe"
        );
    }
    let down = fixture.output("down", &[]);
    assert_success(&down);
    let report: Value = serde_json::from_slice(&down.stdout).unwrap();
    assert_eq!(report["canceledRunId"], json!(run_id));
    assert_eq!(
        wait_for_settled(&fixture, &run_id).as_deref(),
        Some("canceled")
    );
}

#[test]
fn background_failure_after_acknowledgement_belongs_to_the_session() {
    let fixture = RuntimeFixture::new(leaf_task_manifest(&["exit", "3"]));
    let launch = fixture.output("run", &["--task", "smoke", "--daemon"]);
    assert_success(&launch);
    let acknowledgement: Value = serde_json::from_slice(&launch.stdout).unwrap();
    let run_id = acknowledgement["runId"].as_str().unwrap();
    assert_eq!(
        wait_for_settled(&fixture, run_id).as_deref(),
        Some("failed")
    );
    let runs = fixture.state_base.join("registry/runtime-test/dev/0/runs");
    assert!(
        runs.join(run_id).join("logs/task.0.stdout.log").is_file(),
        "background evidence is retained under the acknowledged identity"
    );
}

#[test]
fn background_launch_rejects_before_establishment_without_new_work() {
    // An occupied slot rejects without a second session record.
    let marker = temp_marker("nixfied-output-daemon-occupied");
    let fixture = blocking_session(&marker);
    let (owner, first, _) = spawn_live_session(&fixture, &marker);
    let launch = fixture.output("run", &["--task", "smoke", "--daemon"]);
    assert_eq!(
        launch.status.code(),
        Some(22),
        "{}",
        String::from_utf8_lossy(&launch.stderr)
    );
    assert!(String::from_utf8_lossy(&launch.stderr).contains("CLEANUP_REFUSED"));
    let runs: i64 = fixture
        .registry()
        .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(runs, 1);
    assert_success(&fixture.output("down", &[]));
    let _ = wait_for_child_output(owner, Duration::from_secs(10));
    assert_eq!(
        wait_for_settled(&fixture, &first).as_deref(),
        Some("canceled")
    );

    // Interactive stdin and an output projection reject before any state.
    let mut manifest = leaf_task_manifest(&["exit", "0"]);
    manifest["tasks"]["smoke"]["invocation"]["stdin"] = json!("inherit");
    let fixture = RuntimeFixture::new(manifest);
    let launch = fixture.output("run", &["--task", "smoke", "--daemon"]);
    assert_eq!(launch.status.code(), Some(37));
    assert!(!fixture.state_base.join("registry").exists());
    let fixture = RuntimeFixture::new(leaf_task_manifest(&["exit", "0"]));
    let launch = fixture.output("run", &["--task", "smoke", "--daemon", "--output", "json"]);
    assert_eq!(launch.status.code(), Some(36));
    assert!(!fixture.state_base.exists());
}

#[test]
fn sealed_backlog_larger_than_one_pass_is_drained_completely() {
    use std::io::Read;
    let count = 12 * 1024 * 1024;
    let manifest = task_manifest(&["output", "repeat", "78", &count.to_string(), "79", "0"]);
    let fixture = RuntimeFixture::new(manifest);
    let mut child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    // Read nothing until the output is sealed, so the whole backlog remains.
    wait_for_registry(&fixture, "SELECT output = 'sealed' FROM runs");
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(20));
    assert_success(&output);
    assert_eq!(bytes.len(), count, "a slow reader is never truncated");
    assert!(bytes.iter().all(|byte| *byte == b'x'));
}

#[test]
fn recovery_settles_a_dead_owners_service_without_inventing_capture() {
    let marker = temp_marker("nixfied-output-recovery-capture");
    let fixture = RuntimeFixture::new(blocking_manifest(&marker, true, b"", b""));
    let (owner, run_id, _) = spawn_live_session(&fixture, &marker);
    assert_eq!(
        unsafe { libc::kill(owner.id() as libc::pid_t, libc::SIGKILL) },
        0
    );
    let _ = wait_for_child_output(owner, Duration::from_secs(5));
    let down = fixture.output("down", &[]);
    assert_success(&down);
    let rows: Vec<(String, String, String)> = fixture
        .registry()
        .prepare("SELECT role, ownership, capture FROM processes WHERE run_id = ?1 ORDER BY role")
        .unwrap()
        .query_map([&run_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(rows.iter().any(|(role, ..)| role == "service"));
    for (role, ownership, capture) in rows {
        assert_eq!(ownership, "settled", "{role}");
        assert_eq!(
            capture,
            if role == "probe" {
                "complete"
            } else {
                "pending"
            },
            "recovery preserves previously completed probes and never invents capture ({role})"
        );
    }
}

#[test]
fn a_stalled_stream_does_not_hold_back_the_other() {
    use std::io::Read;
    let flood = 8 * 1024 * 1024;
    let small = 4096;
    for stall_stdout in [true, false] {
        let (stdout_count, stderr_count) = if stall_stdout {
            (flood, small)
        } else {
            (small, flood)
        };
        let fixture = RuntimeFixture::new(task_manifest(&[
            "output",
            "repeat",
            "78",
            &stdout_count.to_string(),
            "01",
            &stderr_count.to_string(),
        ]));
        let mut child = fixture
            .command("run", &["--task", "smoke", "--output", "task-output"])
            .spawn()
            .unwrap();
        let stdout: Box<dyn std::io::Read + Send> = Box::new(child.stdout.take().unwrap());
        let stderr: Box<dyn std::io::Read + Send> = Box::new(child.stderr.take().unwrap());
        let (stalled, mut live, live_byte) = if stall_stdout {
            (stdout, stderr, 0x01)
        } else {
            (stderr, stdout, b'x')
        };
        // Never read the stalled stream until the other has fully arrived.
        let mut seen = Vec::new();
        let mut chunk = [0_u8; 4096];
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while seen.iter().filter(|byte| **byte == live_byte).count() < small {
            assert!(
                std::time::Instant::now() < deadline,
                "a stream was held back by the stalled reader (stall stdout: {stall_stdout})"
            );
            let read = live.read(&mut chunk).unwrap();
            assert!(read > 0, "a stream closed before its bytes arrived");
            seen.extend_from_slice(&chunk[..read]);
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "the other stream is still stalled"
        );
        let reader = stdout_reader(stalled);
        live.read_to_end(&mut seen).unwrap();
        assert!(child.wait().unwrap().success());
        let resumed = reader.join().unwrap();
        let (stdout, stderr) = if stall_stdout {
            (resumed, seen)
        } else {
            (seen, resumed)
        };
        assert_eq!(
            stdout.len(),
            stdout_count,
            "a slow reader is never truncated"
        );
        assert!(stdout.iter().all(|byte| *byte == b'x'));
        assert_eq!(task_bytes(&stderr), stderr_count);
    }
}

#[test]
fn interrupted_launcher_leaves_no_live_background_session() {
    for delay in [0_u64, 5, 20, 80] {
        let marker = temp_marker("nixfied-output-daemon-interrupt");
        let fixture = blocking_session(&marker);
        let launcher = fixture
            .command("run", &["--task", "smoke", "--daemon"])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(delay));
        assert_eq!(
            unsafe { libc::kill(launcher.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        let output = wait_for_child_output(launcher, Duration::from_secs(20));
        if output.status.success() {
            // The signal arrived after the launcher returned its
            // acknowledgement: that session is established and independent.
            let acknowledgement: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(acknowledgement["runId"].is_string());
            assert_success(&fixture.output("down", &[]));
        }
        // Either the owner abandoned before its commit, or establishment won
        // and the launcher canceled exactly that session. A signal that lands
        // before the launcher installs its handlers ends it by default action.
        assert!(
            matches!(output.status.code(), Some(0 | 27) | None),
            "{:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        if find_named(&fixture.state_base, "registry.sqlite3").is_some() {
            // An interrupted launch leaves no live session.
            wait_for_registry(
                &fixture,
                "SELECT count(*) = 0 FROM runs WHERE finalization = 'unfinished'",
            );
        }
    }
}

#[test]
fn stalled_stdout_and_stderr_block_neither_settlement_nor_slot_release() {
    use std::io::Read;
    let count = 4 * 1024 * 1024;
    let manifest = task_manifest(&[
        "output",
        "repeat",
        "78",
        &count.to_string(),
        "01",
        &count.to_string(),
    ]);
    let fixture = RuntimeFixture::new(manifest);
    let mut child = fixture
        .command("run", &["--task", "smoke", "--output", "task-output"])
        .spawn()
        .unwrap();
    // Hold both caller streams open without reading either.
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    wait_for_registry(&fixture, SEALED_SETTLEMENT);
    assert!(
        child.try_wait().unwrap().is_none(),
        "the command keeps presenting after settlement"
    );
    // Another session acquires the released slot while both readers stall.
    assert_success(&output_eventually(
        &fixture,
        "run",
        &["--task", "smoke", "--output", "json"],
    ));
    assert!(child.try_wait().unwrap().is_none());
    // Delivery resumes from the sealed evidence without truncation.
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
    assert_eq!(bytes.len(), count);
    assert!(bytes.iter().all(|byte| *byte == b'x'));
    let stderr = reader.join().unwrap();
    assert_eq!(task_bytes(&stderr), count);
}

/// Count the task's 0x01 bytes on a stream that also carries live runtime
/// diagnostics, which may land between any two presented chunks.
fn task_bytes(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == 0x01).count()
}

/// A composite whose first step floods the caller's stderr in summary mode and
/// whose second step blocks with `marker`, so the reader is stalled while the
/// session is still live.
fn stalled_live_session(marker: &Path, service: bool) -> RuntimeFixture {
    let mut manifest = blocking_manifest(marker, service, b"", b"");
    let requires: &[&str] = if service { &["synthetic"] } else { &[] };
    let flood = (4 * 1024 * 1024).to_string();
    add_task_clone(
        &mut manifest,
        "flood",
        requires,
        &["output", "repeat", "78", "0", "01", &flood],
    );
    manifest["tasks"]["pipeline"] = json!({
        "kind": "composite", "defaultOutput": "summary",
        "steps": {"flood": {"task": "flood", "dependsOn": []}, "block": {"task": "smoke", "dependsOn": ["flood"]}}
    });
    RuntimeFixture::new(manifest)
}

#[test]
fn cancellation_and_service_failure_settle_while_the_caller_stalls() {
    for service in [false, true] {
        let marker = temp_marker("nixfied-output-stalled-live");
        let fixture = stalled_live_session(&marker, service);
        let mut child = fixture
            .command("run", &["--task", "pipeline", "--output", "summary"])
            .spawn()
            .unwrap();
        // Hold both caller streams without reading; the flood fills stderr.
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        assert!(wait_for_path(&marker, Duration::from_secs(20)));
        let (run_id, _) = published_session(&fixture);
        if service {
            let pid: i32 = fixture
                .registry()
                .query_row(
                    "SELECT pid FROM processes WHERE role = 'service' AND run_id = ?1",
                    [&run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        } else {
            let down = fixture.output("down", &["--timeout-ms", "10000"]);
            assert_success(&down);
            let report: Value = serde_json::from_slice(&down.stdout).unwrap();
            assert_eq!(report["canceledRunId"], json!(run_id));
        }
        let expected = if service { "failed" } else { "canceled" };
        assert_eq!(
            wait_for_settled(&fixture, &run_id).as_deref(),
            Some(expected),
            "supervision and teardown proceed while the caller stalls"
        );
        let rows: Vec<(String, String)> = fixture
            .registry()
            .prepare("SELECT role, ownership FROM processes WHERE run_id = ?1")
            .unwrap()
            .query_map([&run_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            rows.iter().all(|(_, ownership)| ownership == "settled"),
            "{rows:?}"
        );
        // The slot is released while the command still presents.
        assert_success(&output_eventually(&fixture, "clean", &[]));
        assert!(
            child.try_wait().unwrap().is_none(),
            "the caller's reader is still stalled"
        );
        // The resumed reader receives the whole flood and the command's verdict.
        let readers = [stdout_reader(stdout), stdout_reader(stderr)];
        let status = child.wait().unwrap();
        let [_, stderr] = readers.map(|reader| reader.join().unwrap());
        let verdict = String::from_utf8_lossy(&stderr[stderr.len().saturating_sub(4096)..]);
        assert!(!status.success());
        assert!(
            verdict.contains(if service {
                "DEPENDENCY_UNAVAILABLE"
            } else {
                "CANCELED"
            }),
            "{verdict}"
        );
        assert!(task_bytes(&stderr) == 4 * 1024 * 1024);
        assert_eq!(
            session_row(&fixture, &run_id).0.as_deref(),
            Some(expected),
            "interrupted delivery never rewrites the settled session"
        );
    }
}

#[test]
fn background_owner_death_after_acknowledgement_recovers_under_the_acknowledged_identity() {
    let marker = temp_marker("nixfied-output-daemon-owner-death");
    let fixture = blocking_session(&marker);
    let launch = fixture.output("run", &["--task", "smoke", "--daemon"]);
    assert_success(&launch);
    let acknowledgement: Value = serde_json::from_slice(&launch.stdout).unwrap();
    let run_id = acknowledgement["runId"].as_str().unwrap().to_owned();
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    let (published, task_pid) = published_session(&fixture);
    assert_eq!(published, run_id);
    let owner: i32 = fixture
        .registry()
        .query_row(
            "SELECT json_extract(owner_identity, '$.pid') FROM runs WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unsafe { libc::kill(owner, libc::SIGKILL) }, 0);
    poll_until(Duration::from_secs(5), "the owner to be reaped", || {
        (unsafe { libc::kill(owner, 0) } != 0).then_some(())
    });
    assert_eq!(
        run_row(&fixture, &run_id),
        Some((None, "unfinished".into())),
        "owner death alone never settles the session"
    );
    let down = fixture.output("down", &["--timeout-ms", "10000"]);
    assert_success(&down);
    let report: Value = serde_json::from_slice(&down.stdout).unwrap();
    assert!(report.get("canceledRunId").is_none(), "{report}");
    assert_eq!(report["stopped"].as_array().unwrap().len(), 1, "{report}");
    assert_ne!(unsafe { libc::kill(task_pid, 0) }, 0);
    assert_eq!(
        run_row(&fixture, &run_id),
        Some((Some("interrupted".into()), "complete".into())),
        "recovery settles the acknowledged session under its own identity"
    );
    let runs: i64 = fixture
        .registry()
        .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(runs, 1, "recovery invents no replacement session");
}

fn stdout_reader(
    mut stream: impl std::io::Read + Send + 'static,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        bytes
    })
}
