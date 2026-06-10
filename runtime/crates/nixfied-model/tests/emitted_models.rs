//! Verifies the Nix producer emits exactly what the runtime consumes. When
//! `NIXFIED_EMITTED_MODELS` is set (colon-separated `model.json` paths), each is
//! deserialized through the `Model` contract (with `deny_unknown_fields`) and
//! validated. A no-op when the env var is absent, so plain `cargo test` is green.

use nixfied_model::{Model, Validate};

#[test]
fn emitted_models_match_the_contract() {
    let Ok(paths) = std::env::var("NIXFIED_EMITTED_MODELS") else {
        return;
    };
    for path in paths.split(':').filter(|path| !path.is_empty()) {
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("read {path}: {error}"));
        let model: Model = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("deserialize {path}: {error}"));
        model
            .validate()
            .unwrap_or_else(|error| panic!("validate {path}: {error:?}"));
    }
}
