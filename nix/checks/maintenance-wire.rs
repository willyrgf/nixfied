use nixfied_manifest::FixtureTaskSpec;

// The added field has an actual native use, not only a decoder membership test.
fn select(task: &FixtureTaskSpec) -> bool {
    task.fixture_enabled
}

#[test]
fn required_boolean_rejects_missing_null_and_wrong_type() {
    for raw in [
        r#"{"kind":"composite","defaultOutput":"summary","serviceLifetime":"run-scoped","servicesRequired":[]}"#,
        r#"{"kind":"composite","defaultOutput":"summary","serviceLifetime":"run-scoped","servicesRequired":[],"fixtureEnabled":null}"#,
        r#"{"kind":"composite","defaultOutput":"summary","serviceLifetime":"run-scoped","servicesRequired":[],"fixtureEnabled":"true"}"#,
    ] {
        assert!(serde_json::from_str::<FixtureTaskSpec>(raw).is_err());
    }
}

#[test]
fn added_boolean_is_constructed_serialized_and_consumed_natively() {
    for (raw, expected) in [
        (
            r#"{"kind":"composite","defaultOutput":"summary","serviceLifetime":"run-scoped","servicesRequired":[],"fixtureEnabled":true}"#,
            true,
        ),
        (
            r#"{"kind":"composite","defaultOutput":"summary","serviceLifetime":"run-scoped","servicesRequired":[],"fixtureEnabled":false}"#,
            false,
        ),
    ] {
        let task: FixtureTaskSpec = serde_json::from_str(raw).unwrap();
        assert_eq!(select(&task), expected);
        let output = serde_json::to_value(task).unwrap();
        assert_eq!(output["fixtureEnabled"], expected);
        assert!(output.get("invocation").is_none());
    }
}
