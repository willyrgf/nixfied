use nixfied_model::{InvocationSpec, Lifecycle, ServiceSpec, TaskDefaultOutput, TaskSpec};
use serde_json::{Value, json};

fn invocation() -> Value {
    json!({"tools":["tool"],"run":["tool"],"executable":"/nix/store/tool/bin/tool",
        "env":{},"codebaseId":"main","cwd":".","stdin":"null","timeoutMs":1})
}

fn lifecycle() -> Value {
    let terminal = json!({"success":"ok","failure":"failed"});
    let probe = json!({"kind":"tcp","timeoutMs":1,"retryIntervalMs":1,"maxAttempts":1});
    json!({
        "start":{"operationId":"start","invocation":invocation(),"terminal":terminal},
        "ready":{"operationId":"ready","probe":probe,"terminal":terminal},
        "health":{"operationId":"health","probe":probe,"terminal":terminal},
        "stop":{"operationId":"stop","signal":"TERM","timeoutMs":1,"terminal":terminal},
        "clean":{"operationId":"clean","terminal":terminal}
    })
}

#[test]
fn required_positive_timeout_preserves_full_u64_domain() {
    for timeout in [json!(1), json!(u64::MAX)] {
        let mut input = invocation();
        input["timeoutMs"] = timeout.clone();
        let parsed: InvocationSpec = serde_json::from_value(input).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap()["timeoutMs"], timeout);
    }
    for timeout in [Value::Null, json!(0), json!(-1), json!(1.5), json!("1")] {
        let mut input = invocation();
        input["timeoutMs"] = timeout;
        assert!(serde_json::from_value::<InvocationSpec>(input).is_err());
    }
    let mut input = invocation();
    input.as_object_mut().unwrap().remove("timeoutMs");
    assert!(serde_json::from_value::<InvocationSpec>(input).is_err());
    let overflow = invocation()
        .to_string()
        .replace("\"timeoutMs\":1", "\"timeoutMs\":18446744073709551616");
    assert!(serde_json::from_str::<InvocationSpec>(&overflow).is_err());
}

#[test]
fn prepare_absence_is_omitted_and_unknown_fields_reject() {
    for supplied in [false, true] {
        let mut input = lifecycle();
        if supplied {
            input["prepare"] = Value::Null;
        }
        let parsed: Lifecycle = serde_json::from_value(input).unwrap();
        assert!(parsed.prepare.is_none());
        assert_eq!(serde_json::to_value(parsed).unwrap(), lifecycle());
    }
    let mut input = lifecycle();
    input["unexpected"] = json!(true);
    assert!(serde_json::from_value::<Lifecycle>(input).is_err());
}

#[test]
fn endpoints_default_to_empty_but_null_and_invalid_hosts_reject() {
    let base = json!({"lifecycle":lifecycle(),"connectsTo":[],"stateRefs":[],"logRefs":[],"containment":"process-group"});
    for explicit in [false, true] {
        let mut input = base.clone();
        if explicit {
            input["endpoints"] = json!({});
        }
        let parsed: ServiceSpec = serde_json::from_value(input).unwrap();
        assert!(parsed.endpoints.is_empty());
        assert_eq!(serde_json::to_value(parsed).unwrap(), base);
    }
    for endpoints in [
        Value::Null,
        json!({"web":{"endpointId":"web","host":"0.0.0.0"}}),
    ] {
        let mut input = base.clone();
        input["endpoints"] = endpoints;
        assert!(serde_json::from_value::<ServiceSpec>(input).is_err());
    }
}

#[test]
fn task_defaults_and_unique_lists_keep_distinct_wire_policies() {
    let input = json!({"kind":"composite","serviceLifetime":"run-scoped"});
    let parsed: TaskSpec = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(parsed.default_output, TaskDefaultOutput::Summary);
    assert_eq!(TaskDefaultOutput::default(), TaskDefaultOutput::Summary);
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        r#"{"kind":"composite","defaultOutput":"summary","serviceLifetime":"run-scoped","servicesRequired":[]}"#
    );
    for field in [
        "defaultOutput",
        "servicesRequired",
        "steps",
        "requires",
        "artifactRefs",
    ] {
        let mut invalid = input.clone();
        invalid[field] = Value::Null;
        assert!(
            serde_json::from_value::<TaskSpec>(invalid).is_err(),
            "{field}"
        );
    }
    let mut invalid = input.clone();
    invalid["defaultOutput"] = json!("unknown");
    assert!(serde_json::from_value::<TaskSpec>(invalid).is_err());
    let mut duplicate = input;
    duplicate["servicesRequired"] = json!(["db", "db"]);
    assert!(serde_json::from_value::<TaskSpec>(duplicate).is_err());
}
