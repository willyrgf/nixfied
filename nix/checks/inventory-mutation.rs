use nixfied_manifest::StdinPolicy;

#[test]
fn changed_inventory_is_the_decoder_and_encoder_domain() {
    for wire in [r#""null""#, r#""pipe""#] {
        let parsed: StdinPolicy = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), wire);
    }
    for wire in [r#""inherit""#, r#""other""#, "null", "0"] {
        assert!(serde_json::from_str::<StdinPolicy>(wire).is_err());
    }
}
