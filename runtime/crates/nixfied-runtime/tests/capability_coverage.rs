//! The capability descriptor must name every wire field, so a model struct that
//! gains a field without a matching descriptor edit cannot slip through with an
//! unrotated ABI digest (the failure mode the descriptor exists to prevent).
//!
//! Serialize a fully-populated model, collect every struct field name — skipping
//! the data keys of map-typed fields, the only place object keys are ids rather
//! than schema fields — and require each to appear as a token in the descriptor.

use std::collections::BTreeSet;

use nixfied_model::constants::CAPABILITY_DESCRIPTOR;
use serde_json::{Value, json};

mod common;
use common::*;

/// Fields whose JSON object keys are data (ids), not schema field names; their
/// keys are skipped while their values (structs) are still walked.
const MAP_FIELDS: &[&str] = &[
    "services",
    "closures",
    "tasks",
    "steps",
    "environments",
    "slotPlacements",
    "endpoints",
    "env",
];

fn collect_fields(value: &Value, in_map: bool, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if !in_map {
                    out.insert(key.clone());
                }
                collect_fields(child, MAP_FIELDS.contains(&key.as_str()), out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_fields(item, false, out);
            }
        }
        _ => {}
    }
}

#[test]
fn capability_descriptor_names_every_wire_field() {
    // The synthetic fixture has no composite; inject one so the StepSpec
    // fields are exercised too.
    let mut model = synthetic_model_default(23080, 23090);
    model["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "steps": { "only": { "task": "smoke", "dependsOn": [] } }
    });

    let mut fields = BTreeSet::new();
    collect_fields(&model, false, &mut fields);

    let tokens: BTreeSet<&str> = CAPABILITY_DESCRIPTOR.split_whitespace().collect();
    let missing: Vec<&String> = fields
        .iter()
        .filter(|field| !tokens.contains(field.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "capability descriptor is missing wire fields: {missing:?}"
    );
}
