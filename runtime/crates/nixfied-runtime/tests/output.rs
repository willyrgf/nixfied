//! End-to-end coverage for the direct-leaf task-output projection.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use common::{
    TempDir, available_port_window, runtime_binary, synthetic_manifest, test_child,
    wait_for_child_output, wait_for_path,
};
use nixfied_runtime::redaction::REDACTION_TOKEN;
use serde_json::{Value, json};

struct RuntimeFixture {
    _tmp: TempDir,
    manifest_path: PathBuf,
    state_base: PathBuf,
}

fn fixture(manifest: Value) -> RuntimeFixture {
    let tmp = TempDir::new();
    let manifest_path = tmp.path.join("manifest.json");
    let state_base = tmp.path.join("state");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("fixture manifest should serialize"),
    )
    .expect("fixture manifest should be written");
    RuntimeFixture {
        _tmp: tmp,
        manifest_path,
        state_base,
    }
}

fn task_manifest(args: &[String]) -> Value {
    let executable = test_child();
    let executable = executable
        .to_str()
        .expect("test child path should be UTF-8")
        .to_string();
    let port = available_port_window(1);
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

fn command(fixture: &RuntimeFixture, extra: &[&str]) -> Command {
    let mut command = Command::new(runtime_binary());
    command
        .arg("run")
        .arg("--allow-non-store-manifest")
        .arg("--manifest")
        .arg(&fixture.manifest_path)
        .arg("--state-base")
        .arg(&fixture.state_base)
        .args(extra)
        .current_dir(&fixture._tmp.path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(fixture: &RuntimeFixture, extra: &[&str]) -> Output {
    command(fixture, extra)
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

fn no_service(manifest: &mut Value) {
    manifest["tasks"]["smoke"]["requires"] = json!([]);
    manifest["tasks"]["smoke"]["servicesRequired"] = json!([]);
}

#[test]
fn run_and_aggregate_views_cross_the_native_redaction_and_formatting_boundary() {
    let mut manifest = task_manifest(&["exit".to_string(), "0".to_string()]);
    no_service(&mut manifest);
    manifest["secrets"]["token"] = json!({"secretId":"token","source":{
        "kind":"env-var","envVar":"NIXFIED_OUTPUT_VIEW_SECRET"
    }});
    let fixture = fixture(manifest);
    let output = command(&fixture, &["--task", "smoke", "--output", "json"])
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
    let fixture = fixture(task_manifest(&args));
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
    let mut manifest = task_manifest(&[
        "output".to_string(),
        "hex".to_string(),
        hex(stdout),
        hex(stderr),
    ]);
    no_service(&mut manifest);
    set_task_default_output(&mut manifest, "task-output");
    let fixture = fixture(manifest);
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
    let mut manifest = task_manifest(&["exit".to_string(), "0".to_string()]);
    no_service(&mut manifest);
    set_task_default_output(&mut manifest, "task-output");
    let fixture = fixture(manifest);
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
        "servicesRequired": ["synthetic"],
        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });
    let fixture = fixture(manifest);
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
    let fixture = fixture(manifest);
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
    let mut manifest = task_manifest(&args);
    no_service(&mut manifest);
    let fixture = fixture(manifest);
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
    let mut manifest = task_manifest(&args);
    no_service(&mut manifest);
    let fixture = fixture(manifest);
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
    let mut manifest = task_manifest(&args);
    no_service(&mut manifest);
    let fixture = fixture(manifest);
    let mut child = command(&fixture, &["--task", "smoke", "--output", "task-output"])
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
    let fixture = fixture(task_manifest(&args));
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
        task_manifest(&["output".to_string(), "env".to_string(), "TOKEN".to_string()]);
    no_service(&mut manifest);
    manifest["secrets"]["api-token"] = json!({
        "secretId": "api-token",
        "source": {
            "kind": "env-var",
            "envVar": "NIXFIED_TEST_TASK_SECRET"
        }
    });
    manifest["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:api-token}");
    let fixture = fixture(manifest);
    let output = command(&fixture, &["--task", "smoke", "--output", "task-output"])
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
    let mut manifest = task_manifest(&args);
    no_service(&mut manifest);
    manifest["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(150);
    let fixture = fixture(manifest);
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
fn cancellation_replays_output_before_reporting_canceled() {
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
    let mut manifest = task_manifest(&args);
    no_service(&mut manifest);
    manifest["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(30_000);
    let fixture = fixture(manifest);
    let child = command(&fixture, &["--task", "smoke", "--output", "task-output"])
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
}

#[test]
fn invalid_selection_is_rejected_before_state_or_child_side_effects() {
    let mut manifest = task_manifest(&["exit".to_string(), "0".to_string()]);
    manifest["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "serviceLifetime": "run-scoped",
        "servicesRequired": ["synthetic"],
        "steps": { "only": { "task": "smoke" } }
    });
    let fixture = fixture(manifest);
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

    let missing = fixture(manifest.clone());
    let output = run(&missing, &["--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(output.stdout.is_empty());
    assert!(!missing.state_base.exists());

    let unknown = fixture(manifest.clone());
    let output = run(&unknown, &["--task", "missing", "--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(!unknown.state_base.exists());

    let repeated = fixture(manifest.clone());
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
        let alias = fixture(manifest.clone());
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
        let fixture = fixture(task_manifest(&["exit".to_string(), "0".to_string()]));
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
    let fixture = fixture(manifest);
    let mut bytes = b"synthetic-prefix-".to_vec();
    bytes.push(0xff);
    bytes.extend_from_slice(b"-synthetic-suffix");
    for mode in ["summary", "json", "both", "task-output"] {
        let output = command(&fixture, &["--task", "smoke", "--output", mode])
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
    let fixture = fixture(manifest);
    let output = Command::new(runtime_binary())
        .args(["check", "--allow-non-store-manifest", "--manifest"])
        .arg(&fixture.manifest_path)
        .current_dir(&fixture._tmp.path)
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
