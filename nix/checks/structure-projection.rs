
#[test]
fn required_open_json_rejects_missing() {
    let accepted = r#"{"details":null}"#;
    let value: PresenceFixture = serde_json::from_str(accepted).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(),
        r#"{"optional":null,"details":null}"#);
    for rejected in [r#"{"optional":null}"#, r#"{}"#] {
        assert!(serde_json::from_str::<PresenceFixture>(rejected).is_err(), "{rejected}");
    }
    for details in ["null", "[]", "{}", "42", "false", "\"secret-bearing text\""] {
        let input = format!(r#"{{"optional":null,"details":{details}}}"#);
        let parsed: PresenceFixture = serde_json::from_str(&input).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), input);
    }
}

#[test]
fn borrowed_no_decoder_omits_empty_without_a_default() {
    assert_eq!(serde_json::to_string(&BorrowedFixture { values: &[] }).unwrap(), "{}");
    let values = ["one".to_owned()];
    assert_eq!(serde_json::to_string(&BorrowedFixture { values: &values }).unwrap(),
        r#"{"values":["one"]}"#);
}

#[test]
fn unusual_wire_name_uses_rust_escaping_and_preserves_json_bytes() {
    let value = EscapingFixture { ordinary: "value".to_owned() };
    let expected = r#"{"wire\u0001\b\f\r\n\t\"\\b\\u0001 λ":"value"}"#;
    assert_eq!(serde_json::to_string(&value).unwrap(), expected);
    let decoded: EscapingFixture = serde_json::from_str(expected).unwrap();
    assert_eq!(decoded.ordinary, "value");
}
