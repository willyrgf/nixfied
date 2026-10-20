//! Slot marker and provenance preparation after predecessor recovery. Manifest
//! provenance never selects which processes recovery must settle.

use crate::control::Recovered;
use crate::error::RuntimeResult;
use crate::registry::{EventInsert, Registry};
use crate::state::marker::{
    MarkerDecision, StateIdentity, commit_slot_marker, evaluate_slot_marker, publish_slot_marker,
};

/// Prepare the marker-owned state root after the slot owner has completed
/// predecessor recovery, which the consumed proof attests: no process or
/// endpoint obligation remains and no deletion is pending, so no generation or
/// provenance rewrite can race an unfinished deletion. This function never
/// signals processes. The application tree is inspected and created only
/// through the slot guard's held descriptor.
/// Returns the previous manifest hash exactly when provenance was refreshed.
pub fn prepare_slot_state(
    identity: &StateIdentity,
    registry: &mut Registry,
    recovered: Recovered,
) -> RuntimeResult<Option<String>> {
    recovered.check(registry)?;
    let report = match evaluate_slot_marker(registry, identity)? {
        MarkerDecision::Fresh => {
            commit_slot_marker(registry, identity)?;
            None
        }
        MarkerDecision::Adopt => None,
        MarkerDecision::Refresh { existing } => {
            record_provenance_refresh(registry, identity, &existing)?;
            publish_slot_marker(registry, identity, Some(&existing))?;
            Some(existing.computed_manifest_hash)
        }
    };
    Ok(report)
}

fn record_provenance_refresh(
    registry: &mut Registry,
    identity: &StateIdentity,
    existing: &crate::state::marker::StateMarker,
) -> RuntimeResult<()> {
    let payload = serde_json::json!({
        "fromManifestHash": existing.computed_manifest_hash,
        "toManifestHash": identity.computed_manifest_hash,
        "fromManifestPath": existing.manifest_path,
        "toManifestPath": identity.manifest_path,
    })
    .to_string();
    registry.append_event(EventInsert::new("state.provenance-refreshed", &payload))?;
    Ok(())
}
