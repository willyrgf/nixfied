#[test]
fn maintenance_argument_reaches_the_actual_native_parser_and_continuation() {
    let parse = |extra: &[&str]| {
        let mut args = command_args(&["--model", "model", "--state-base", "state"]);
        args.extend(command_args(extra));
        parse_run_options(&args)
    };
    assert_eq!(
        parse(&[])
            .unwrap()
            .resolve(RunOutputMode::Summary)
            .timeout_ms,
        5000
    );
    assert_eq!(
        parse(&["--budget-ms", "+7"])
            .unwrap()
            .resolve(RunOutputMode::Summary)
            .timeout_ms,
        7
    );
    assert_eq!(
        parse(&["--budget-ms", "1", "--budget-ms", "0"])
            .unwrap()
            .resolve(RunOutputMode::Summary)
            .timeout_ms,
        0
    );
    assert_eq!(
        parse(&["--budget-ms", "18446744073709551615"])
            .unwrap()
            .budget_ms,
        Some(u64::MAX)
    );
    for args in [
        vec!["--budget-ms"],
        vec!["--budget-ms", "--task"],
        vec!["--budget-ms", "-1"],
        vec!["--budget-ms", " 1"],
        vec!["--budget-ms", "18446744073709551616"],
        vec!["--budget-ms", "bad", "--budget-ms", "1"],
    ] {
        assert!(parse(&args).is_err());
    }
    assert!(RUN_HELP.contains("  --budget-ms <operation-budget-milliseconds> Fixture operation budget\n  -h, --help"));
}

#[test]
fn maintenance_native_failure_selection_changes_values_without_changing_shape() {
    use nixfied_runtime::ErrorCode;
    // Two versions of a native fixture branch. They use exactly the same
    // declarations and generated error type; no policy enters the metadata.
    fn before(task: RuntimeError, cleanup: RuntimeError) -> RuntimeError {
        task.with_cause(cleanup)
    }
    fn after(task: RuntimeError, cleanup: RuntimeError) -> RuntimeError {
        cleanup.with_cause(task)
    }
    let input = || {
        (
            RuntimeError::new(ErrorCode::TaskFailed, "task"),
            RuntimeError::new(ErrorCode::CleanupRefused, "cleanup"),
        )
    };
    let (task, cleanup) = input();
    assert_eq!(
        serde_json::to_string(&before(task, cleanup)).unwrap(),
        r#"{"code":"TASK_FAILED","exitClass":"error","message":"task","details":{},"causes":[{"code":"CLEANUP_REFUSED","exitClass":"error","message":"cause: CLEANUP_REFUSED","details":{}}],"modelPath":null,"computedModelHash":null}"#
    );
    let (task, cleanup) = input();
    assert_eq!(
        serde_json::to_string(&after(task, cleanup)).unwrap(),
        r#"{"code":"CLEANUP_REFUSED","exitClass":"error","message":"cleanup","details":{},"causes":[{"code":"TASK_FAILED","exitClass":"error","message":"cause: TASK_FAILED","details":{}}],"modelPath":null,"computedModelHash":null}"#
    );
}
