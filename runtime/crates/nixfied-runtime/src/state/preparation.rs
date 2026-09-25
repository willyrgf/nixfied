//! Slot marker and provenance preparation after predecessor recovery. Manifest
//! provenance never selects which processes recovery must settle.

use serde::Serialize;

use crate::control::require_settled_slot;
use crate::error::RuntimeResult;
use crate::registry::{EventInsert, Registry};
use crate::state::cleanup::resume_pending_cleanup;
use crate::state::marker::{
    MarkerDecision, StateIdentity, commit_slot_marker, evaluate_slot_marker, publish_slot_marker,
};

/// What [`prepare_slot_state`] did to make the slot usable for this identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationReport {
    pub provenance_refreshed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_manifest_hash: Option<String>,
}

impl PreparationReport {
    fn unchanged() -> Self {
        Self {
            provenance_refreshed: false,
            from_manifest_hash: None,
        }
    }
}

/// Prepare the marker-owned state root after the slot owner has completed
/// predecessor recovery. This function never signals processes. Unsettled
/// registry evidence rejects before marker inspection or filesystem mutation.
/// The application tree is inspected and created only through the slot
/// guard's held descriptor.
pub fn prepare_slot_state(
    identity: &StateIdentity,
    registry: &mut Registry,
) -> RuntimeResult<PreparationReport> {
    require_settled_slot(registry)?;
    // An unfinished deletion blocks any new generation or provenance rewrite.
    resume_pending_cleanup(identity, registry)?;
    let report = match evaluate_slot_marker(registry, identity)? {
        MarkerDecision::Fresh => {
            commit_slot_marker(registry, identity)?;
            PreparationReport::unchanged()
        }
        MarkerDecision::Adopt(_) => PreparationReport::unchanged(),
        MarkerDecision::Refresh { existing } => {
            record_provenance_refresh(registry, identity, &existing)?;
            publish_slot_marker(registry, identity, Some(&existing))?;
            PreparationReport {
                provenance_refreshed: true,
                from_manifest_hash: Some(existing.computed_manifest_hash),
            }
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
    let mut event = EventInsert::new("state.provenance-refreshed", &payload);
    event.computed_manifest_hash = Some(&identity.computed_manifest_hash);
    registry.append_event(event)?;
    Ok(())
}
