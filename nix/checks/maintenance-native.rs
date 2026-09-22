// Compile a new declaration at an ordinary native consumer, without rewriting
// the production parser. Its actual acquisition/precedence goldens stay native.
#[allow(dead_code)] // This fixture consumes the added argument, not every generated constant.
mod maintenance_syntax {
    use super::*;
    include!("generated/maintenance_commands.rs");

    #[test]
    fn maintenance_argument_has_native_type_default_and_help() {
        let initial: Option<RunBudgetMsValue> = RUN_BUDGET_MS_INITIAL;
        assert_eq!(initial, None);
        assert_eq!(RUN_BUDGET_MS, "--budget-ms");
        for (text, expected) in [("0", 0), ("+7", 7), ("18446744073709551615", u64::MAX)] {
            assert_eq!(text.parse::<RunBudgetMsValue>().unwrap(), expected);
        }
        assert!("18446744073709551616".parse::<RunBudgetMsValue>().is_err());
        assert!(RUN_HELP.contains("  --budget-ms <operation-budget-milliseconds> Fixture operation budget\n  -h, --help"));
    }
}
