//! End-to-end coverage for the direct-leaf task-output projection.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use common::{
    TempDir, available_port_window, runtime_binary, synthetic_model, test_child,
    wait_for_child_output, wait_for_path,
};
use nixfied_runtime::redaction::REDACTION_TOKEN;
use serde_json::{Value, json};

struct RuntimeFixture {
    _tmp: TempDir,
    model_path: PathBuf,
    state_base: PathBuf,
}

fn fixture(model: Value) -> RuntimeFixture {
    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("fixture model should serialize"),
    )
    .expect("fixture model should be written");
    RuntimeFixture {
        _tmp: tmp,
        model_path,
        state_base,
    }
}

fn task_model(args: &[String]) -> Value {
    let executable = test_child();
    let executable = executable
        .to_str()
        .expect("test child path should be UTF-8")
        .to_string();
    let port = available_port_window(1);
    let mut model = synthetic_model(
        &executable,
        &["listen", "127.0.0.1", "${port}", "hold"],
        port,
        port,
    );
    set_task_run_args(&mut model, args);
    model
}

fn set_task_run_args(model: &mut Value, args: &[String]) {
    let program = model["tasks"]["smoke"]["invocation"]["run"]
        .as_array()
        .expect("fixture task invocation should be an array")
        .first()
        .cloned()
        .expect("fixture task invocation should have a program");
    let mut run = vec![program];
    run.extend(args.iter().cloned().map(Value::String));
    model["tasks"]["smoke"]["invocation"]["run"] = Value::Array(run);
}

fn set_task_default_output(model: &mut Value, output: &str) {
    model["tasks"]["smoke"]["defaultOutput"] = json!(output);
}

fn command(fixture: &RuntimeFixture, extra: &[&str]) -> Command {
    let mut command = Command::new(runtime_binary());
    command
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&fixture.model_path)
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

fn no_service(model: &mut Value) {
    model["tasks"]["smoke"]["requires"] = json!([]);
    model["tasks"]["smoke"]["servicesRequired"] = json!([]);
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
    let fixture = fixture(task_model(&args));
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
    let mut model = task_model(&[
        "output".to_string(),
        "hex".to_string(),
        hex(stdout),
        hex(stderr),
    ]);
    no_service(&mut model);
    set_task_default_output(&mut model, "task-output");
    let fixture = fixture(model);
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
    let mut model = task_model(&["exit".to_string(), "0".to_string()]);
    no_service(&mut model);
    set_task_default_output(&mut model, "task-output");
    let fixture = fixture(model);
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
    let mut model = task_model(&["exit".to_string(), "0".to_string()]);
    set_task_default_output(&mut model, "task-output");
    model["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "defaultOutput": "summary",
        "serviceLifetime": "run-scoped",
        "servicesRequired": ["synthetic"],
        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });
    let fixture = fixture(model);
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
    let mut model = task_model(&args);
    model["tasks"]["smoke"]["exitPolicy"]["successCodes"] = json!([0, 7]);
    let fixture = fixture(model);
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
    let mut model = task_model(&args);
    no_service(&mut model);
    let fixture = fixture(model);
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
    let mut model = task_model(&args);
    no_service(&mut model);
    let fixture = fixture(model);
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
    let mut model = task_model(&args);
    no_service(&mut model);
    let fixture = fixture(model);
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
    let fixture = fixture(task_model(&args));
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
    let mut model = task_model(&["output".to_string(), "env".to_string(), "TOKEN".to_string()]);
    no_service(&mut model);
    model["secrets"]["api-token"] = json!({
        "secretId": "api-token",
        "source": {
            "kind": "env-var",
            "envVar": "NIXFIED_TEST_TASK_SECRET"
        }
    });
    model["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:api-token}");
    let fixture = fixture(model);
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
    let mut model = task_model(&args);
    no_service(&mut model);
    model["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(150);
    let fixture = fixture(model);
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
    let mut model = task_model(&args);
    no_service(&mut model);
    model["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(30_000);
    let fixture = fixture(model);
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
    let mut model = task_model(&["exit".to_string(), "0".to_string()]);
    model["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "serviceLifetime": "run-scoped",
        "servicesRequired": ["synthetic"],
        "steps": { "only": { "task": "smoke" } }
    });
    let fixture = fixture(model);
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
    let model = task_model(&["exit".to_string(), "0".to_string()]);

    let missing = fixture(model.clone());
    let output = run(&missing, &["--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(output.stdout.is_empty());
    assert!(!missing.state_base.exists());

    let unknown = fixture(model.clone());
    let output = run(&unknown, &["--task", "missing", "--output", "task-output"]);
    assert_eq!(output.status.code(), Some(37));
    assert!(!unknown.state_base.exists());

    let repeated = fixture(model.clone());
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
        let alias = fixture(model.clone());
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
        let fixture = fixture(task_model(&["exit".to_string(), "0".to_string()]));
        let output = run(&fixture, &args);
        assert_eq!(output.status.code(), Some(36), "args {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.state_base.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("OUTPUT_MODE_CONFLICT"));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn tempfile_marker(label: &str) -> PathBuf {
    common::temp_marker(&format!("nixfied-output-{label}"))
}
