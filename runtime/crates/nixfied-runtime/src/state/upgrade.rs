//! In-place slot state upgrade: the bridge between the marker decision and a
//! runnable state root. A slot is owned by a project/environment/slot identity,
//! not by one model build — when the marker shows the slot was last used by a
//! different build of the same model, the run upgrades the slot instead of
//! refusing it: live services of the old build are torn down through the
//! registry, the state root is cleaned when the model's declared state epoch
//! changed, and the marker is rewritten with the new provenance. The upgrade is
//! recorded as a registry event, so the marker file itself never grows history.

use serde::Serialize;

use crate::control::{ProcessFilter, down_processes};
use crate::error::RuntimeResult;
use crate::registry::{EventInsert, Registry, append_event};
use crate::state::cleanup::{CleanupMode, clean_marked_state};
use crate::state::marker::{
    MarkerDecision, StateIdentity, commit_slot_marker, evaluate_slot_marker,
};
use crate::state::placement::{HostPlacement, materialize_state_root};

/// What [`prepare_slot_state`] did to make the slot usable for this identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeReport {
    pub upgraded: bool,
    pub cleaned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_model_hash: Option<String>,
}

impl UpgradeReport {
    fn unchanged() -> Self {
        Self {
            upgraded: false,
            cleaned: false,
            from_model_hash: None,
        }
    }
}

/// Evaluate the slot marker and make the state root usable for the requested
/// identity: adopt it as-is, or upgrade it in place (teardown of the old
/// build's live processes, optional epoch-gated state clean, marker rewrite).
/// The registry must already be open — its evidence drives the teardown — and
/// the run's state dirs are materialized here, after any clean.
pub fn prepare_slot_state(
    placement: &HostPlacement,
    identity: &StateIdentity,
    registry: &mut Registry,
    timeout_ms: u64,
) -> RuntimeResult<UpgradeReport> {
    let decision = evaluate_slot_marker(placement, identity)?;
    let report = match decision {
        MarkerDecision::Fresh => {
            materialize_state_root(placement)?;
            commit_slot_marker(placement, identity)?;
            UpgradeReport::unchanged()
        }
        MarkerDecision::Adopt(_) => {
            materialize_state_root(placement)?;
            UpgradeReport::unchanged()
        }
        MarkerDecision::Upgrade {
            clean_state,
            existing,
        } => {
            // Stop everything a different model build left running on this
            // slot before touching its state. Same-hash processes are left
            // alone: a concurrent run of the same model is the service start
            // conflict gate's problem, not an upgrade.
            down_processes(
                registry,
                timeout_ms,
                ProcessFilter::ModelHashNot(&identity.computed_model_hash),
            )?;
            if clean_state {
                // The epoch changed: the model declares the old state
                // incompatible. The marker-gated clean enforces the cleanup
                // policy, so protected/persistent state refuses here instead
                // of being deleted by an upgrade.
                clean_marked_state(
                    &placement.state_base,
                    &placement.state_root,
                    identity,
                    registry,
                    CleanupMode::Standard,
                )?;
            }
            record_upgrade_event(registry, identity, &existing, clean_state)?;
            materialize_state_root(placement)?;
            commit_slot_marker(placement, identity)?;
            UpgradeReport {
                upgraded: true,
                cleaned: clean_state,
                from_model_hash: Some(existing.computed_model_hash),
            }
        }
    };
    Ok(report)
}

fn record_upgrade_event(
    registry: &mut Registry,
    identity: &StateIdentity,
    existing: &crate::state::marker::StateMarker,
    cleaned: bool,
) -> RuntimeResult<()> {
    let payload = serde_json::json!({
        "fromModelHash": existing.computed_model_hash,
        "toModelHash": identity.computed_model_hash,
        "fromModelPath": existing.model_path,
        "toModelPath": identity.model_path,
        "fromEpoch": existing.state_epoch,
        "toEpoch": identity.state_epoch,
        "cleaned": cleaned,
    })
    .to_string();
    let registry_identity = registry.identity().clone();
    let mut event = EventInsert::new("state.upgraded", payload);
    event.computed_model_hash = Some(identity.computed_model_hash.clone());
    append_event(registry.connection_mut(), &registry_identity, &event)?;
    Ok(())
}
