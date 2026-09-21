use super::*;

#[path = "../tests/common/mod.rs"]
mod common;

#[test]
fn local_check_and_run_records_preserve_required_and_omitted_members() {
    let checked = CheckOutput {
        model_path: "/model".into(),
        computed_model_hash: "hash".into(),
        raw_len: 7,
        project_id: "project".into(),
        runtime_abi: "abi".into(),
        toolchain_id: "tool".into(),
        target_system: "system".into(),
        environment: "dev".into(),
        slot: 2,
    };
    let raw_len: usize = checked.raw_len;
    assert_eq!(raw_len, 7);
    assert_eq!(
        serde_json::to_string(&checked).unwrap(),
        r#"{"modelPath":"/model","computedModelHash":"hash","rawLen":7,"projectId":"project","runtimeAbi":"abi","toolchainId":"tool","targetSystem":"system","environment":"dev","slot":2}"#
    );
    let run = RunOutput {
        run_id: "run".into(),
        model_path: "/model".into(),
        computed_model_hash: "hash".into(),
        duration_ms: 3,
        services: vec![],
        tasks: vec![],
        task: None,
        summary_path: None,
        nodes: vec![],
        run_summary_path: None,
    };
    assert_eq!(
        serde_json::to_string(&run).unwrap(),
        r#"{"runId":"run","modelPath":"/model","computedModelHash":"hash","durationMs":3,"services":[],"tasks":[]}"#
    );
}

#[test]
fn aggregate_summary_keeps_native_pretty_bytes_and_write_failure() {
    let tmp = common::TempDir::new();
    let placement = nixfied_runtime::state::HostPlacement {
        state_base: tmp.path.clone(),
        state_root: tmp.path.clone(),
        registry_dir: tmp.path.clone(),
        run_dir: tmp.path.clone(),
        logs_dir: tmp.path.clone(),
        artifacts_dir: tmp.path.clone(),
        summary_path: tmp.path.join("summary.json"),
    };
    let redactor = Redactor::empty();
    let input = || RunSummary {
        placement: &placement,
        run_id: "run",
        run_succeeded: true,
        duration_ms: 3,
        nodes: &[],
        services: &[],
        tasks: &[],
        redactor: &redactor,
    };
    let path = write_run_summary(input()).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{\n  \"durationMs\": 3,\n  \"nodes\": [],\n  \"runId\": \"run\",\n  \"services\": [],\n  \"success\": true,\n  \"tasks\": []\n}"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let error = write_run_summary(input()).unwrap_err();
    assert_eq!(error.code, nixfied_runtime::ErrorCode::StateUnwritable);
    assert_eq!(exit_code(&error), 20);
}

#[test]
fn native_projection_failure_uses_typed_view_and_existing_exit_status() {
    let error = output_projection_io_error(
        OutputStream::Stdout,
        ProjectionOperation::Write,
        "<stdout>",
        io::Error::new(io::ErrorKind::BrokenPipe, "unsafe OS message"),
    );
    assert_eq!(exit_code(&error), 38);
    assert_eq!(
        error.details,
        serde_json::json!({"projections":[{
            "stream":"stdout","operation":"write","kind":"broken-pipe","path":"<stdout>","bytesWritten":0
        }]})
    );
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("unsafe OS message")
    );
}

fn command_args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn native_run_acquisition_repetition_and_integer_domains() {
    let parse = |extra: &[&str]| {
        let mut args = command_args(&["--model", "model", "--state-base", "state"]);
        args.extend(command_args(extra));
        parse_run_options(&args)
    };
    let initial = parse(&[]).unwrap();
    assert_eq!(initial.timeout_ms, 5000);
    assert_eq!(initial.selection.slot, None);
    assert_eq!(initial.output_mode, None);
    assert!(!initial.allow_non_store);
    for (args, code, message) in [
        (
            vec!["--output", "json", "--output", "bad"],
            nixfied_runtime::ErrorCode::OutputModeInvalid,
            "invalid --output value bad: expected summary, json, both, or task-output",
        ),
        (
            vec!["--output", "json", "--output", "both"],
            nixfied_runtime::ErrorCode::OutputModeConflict,
            "--output may be specified only once",
        ),
        (
            vec!["--task", "a", "--task"],
            nixfied_runtime::ErrorCode::TaskSelectionInvalid,
            "missing --task value",
        ),
        (
            vec!["--task", "a", "--task", "b"],
            nixfied_runtime::ErrorCode::TaskSelectionInvalid,
            "--task may be specified only once",
        ),
        (
            vec!["--model"],
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        ),
        (
            vec!["--output", "--slot"],
            nixfied_runtime::ErrorCode::OutputModeInvalid,
            "invalid --output value --slot: expected summary, json, both, or task-output",
        ),
    ] {
        let error = parse(&args).err().unwrap();
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
    }
    for flag in ["--summary", "--json", "--both", "--task-output"] {
        let error = parse(&[flag]).err().unwrap();
        assert_eq!(error.code, nixfied_runtime::ErrorCode::OutputModeInvalid);
        assert_eq!(
            error.message,
            format!("unsupported output flag {flag}; use --output <mode>")
        );
    }
    for token in ["--slot=1", "positional", "--", "-hh"] {
        assert_eq!(
            parse(&[token]).err().unwrap().message,
            format!("unknown run argument: {token}")
        );
    }
    let options = parse(&[
        "--model",
        "first",
        "--model",
        "--literal",
        "--state-base",
        "--path",
        "--task",
        "--a",
        "--allow-non-store-model",
        "--allow-non-store-model",
    ])
    .unwrap();
    assert_eq!(options.model_path, PathBuf::from("--literal"));
    assert_eq!(options.state_base, PathBuf::from("--path"));
    assert_eq!(options.task.as_deref(), Some("--a"));
    assert!(options.allow_non_store);
    for (flag, max, overflow) in [
        ("--slot", "4294967295", "4294967296"),
        (
            "--timeout-ms",
            "18446744073709551615",
            "18446744073709551616",
        ),
    ] {
        for value in ["0", "+1", max] {
            assert!(parse(&[flag, value]).is_ok(), "{flag} {value}");
        }
        for value in [" 1", "1 ", "-1", overflow, ""] {
            assert!(parse(&[flag, value]).is_err(), "{flag} {value}");
        }
        assert!(parse(&[flag]).is_err());
        assert!(parse(&[flag, "bad", flag, "1"]).is_err());
    }
    let options = parse(&[
        "--slot",
        "1",
        "--slot",
        "+2",
        "--timeout-ms",
        "0",
        "--timeout-ms",
        "7",
    ])
    .unwrap();
    assert_eq!(options.selection.slot, Some(2));
    assert_eq!(options.timeout_ms, 7);
    // Observe the native fallback in the current process without mutating its environment.
    let fallback = state_base_from_env();
    let cleared = parse(&["--state-base"]);
    match (fallback, cleared) {
        (Ok(expected), Ok(actual)) => assert_eq!(actual.state_base, expected),
        (Err(expected), Err(actual)) => assert_eq!(actual.message, expected.message),
        _ => panic!("trailing state flag did not restore native fallback"),
    }
}

#[test]
fn native_control_and_check_loops_keep_their_own_boundaries() {
    for command in [
        ControlCommand::Ps,
        ControlCommand::Down,
        ControlCommand::Clean,
    ] {
        let parse = |extra: &[&str]| {
            let mut args = command_args(&["--model", "model", "--state-base", "state"]);
            args.extend(command_args(extra));
            parse_control_options(command, &args)
        };
        let options = parse(&["--slot", "1", "--slot", "+2"]).unwrap();
        assert_eq!(options.selection.slot, Some(2));
        assert_eq!(options.timeout_ms, 5000);
        assert_eq!(
            parse(&["--model"]).err().unwrap().message,
            "missing --model path"
        );
        for value in ["-1", " 1", "4294967296"] {
            assert!(parse(&["--slot", value]).is_err());
        }
        assert_eq!(
            parse(&["--purge", "--purge"]).is_ok(),
            matches!(command, ControlCommand::Clean)
        );
        assert_eq!(
            parse(&["--timeout-ms", "+0"]).is_ok(),
            matches!(command, ControlCommand::Down)
        );
        assert!(parse(&["--slot=1"]).is_err());
        assert!(parse(&["--"]).is_err());
    }
    assert_eq!(
        check(&command_args(&["--model", "earlier", "--model"]))
            .unwrap_err()
            .message,
        "missing --model path"
    );
    for value in ["0", "+1", "4294967295"] {
        assert_eq!(
            check(&command_args(&["--slot", value]))
                .unwrap_err()
                .message,
            "missing --model path"
        );
    }
    for value in ["-1", " 1", "4294967296"] {
        assert!(
            check(&command_args(&["--slot", value]))
                .unwrap_err()
                .message
                .starts_with("invalid --slot value")
        );
    }
}

#[test]
fn native_early_projection_scans_the_last_operand_bearing_output() {
    for (args, expected) in [
        (
            vec!["run", "--output", "json", "--output"],
            ErrorOutputProjection::Json,
        ),
        (
            vec!["run", "--output", "json", "--output", "both"],
            ErrorOutputProjection::Both,
        ),
        (
            vec!["run", "--output", "both", "--output", "bad"],
            ErrorOutputProjection::Human,
        ),
        (
            vec!["check", "--output", "json"],
            ErrorOutputProjection::Human,
        ),
        (
            vec!["run", "--output", "summary"],
            ErrorOutputProjection::Human,
        ),
        (
            vec!["run", "--output", "task-output"],
            ErrorOutputProjection::Human,
        ),
    ] {
        assert_eq!(error_output_projection(&command_args(&args)), expected);
    }
}
