use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use nixfied_model::{CleanupPolicy, Model, PersistencePolicy, Target};
use serde::{Deserialize, Serialize};

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::slot::SelectedSlot;
use crate::state::placement::HostPlacement;

pub const MARKER_FILE_NAME: &str = ".nixfied-state.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateIdentity {
    pub marker_identity: String,
    pub project_id: String,
    pub environment: String,
    pub slot: u32,
    pub state_epoch: String,
    pub cleanup_policy: CleanupPolicy,
    pub persistence: PersistencePolicy,
    pub model_path: PathBuf,
    pub computed_model_hash: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target: Target,
}

impl StateIdentity {
    pub fn from_model(model: &Model, admission: &Admission) -> Self {
        Self::for_slot(model, admission, "dev", 0)
    }

    pub fn from_selected_slot(
        model: &Model,
        admission: &Admission,
        selected_slot: &SelectedSlot<'_>,
    ) -> Self {
        Self::for_slot(
            model,
            admission,
            selected_slot.environment,
            selected_slot.slot,
        )
    }

    pub fn for_slot(model: &Model, admission: &Admission, environment: &str, slot: u32) -> Self {
        Self {
            marker_identity: model.state.marker_identity.clone(),
            project_id: model.project.project_id.clone(),
            environment: environment.to_string(),
            slot,
            state_epoch: model.state.state_epoch.clone(),
            cleanup_policy: model.state.cleanup_policy.clone(),
            persistence: model.state.persistence.clone(),
            model_path: admission.model_path.clone(),
            computed_model_hash: admission.computed_model_hash.clone(),
            runtime_abi: admission.runtime_abi.clone(),
            toolchain_id: admission.toolchain_id.clone(),
            target: model.target.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateMarker {
    pub marker_version: u32,
    pub marker_identity: String,
    pub project_id: String,
    pub environment: String,
    pub slot: u32,
    pub state_kind: StateKind,
    pub service_instance_id: Option<String>,
    pub state_epoch: String,
    pub cleanup_policy: CleanupPolicy,
    pub persistence: PersistencePolicy,
    pub model_path: PathBuf,
    pub computed_model_hash: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target: Target,
}

impl StateMarker {
    pub fn slot(identity: &StateIdentity) -> Self {
        Self {
            marker_version: 1,
            marker_identity: identity.marker_identity.clone(),
            project_id: identity.project_id.clone(),
            environment: identity.environment.clone(),
            slot: identity.slot,
            state_kind: StateKind::Slot,
            service_instance_id: None,
            state_epoch: identity.state_epoch.clone(),
            cleanup_policy: identity.cleanup_policy.clone(),
            persistence: identity.persistence.clone(),
            model_path: identity.model_path.clone(),
            computed_model_hash: identity.computed_model_hash.clone(),
            runtime_abi: identity.runtime_abi.clone(),
            toolchain_id: identity.toolchain_id.clone(),
            target: identity.target.clone(),
        }
    }

    /// Whether this marker's state root belongs to the requested identity at
    /// all: same project, environment, slot, and marker scheme. Ownership is
    /// deliberately blind to which model build last used the root — a model
    /// evolves, its slot does not.
    pub fn matches_ownership(&self, identity: &StateIdentity) -> bool {
        self.marker_version == 1
            && self.marker_identity == identity.marker_identity
            && self.project_id == identity.project_id
            && self.environment == identity.environment
            && self.slot == identity.slot
            && self.state_kind == StateKind::Slot
            && self.service_instance_id.is_none()
    }

    /// Classify this marker against the requested identity. Ownership and
    /// runtime ABI gate access; everything else is provenance — a difference
    /// there means the same slot was last used by another model build and
    /// must be upgraded in place, not refused. A state-epoch difference is the
    /// model's declared state-compatibility boundary, so it upgrades with a
    /// state clean.
    pub fn compare(&self, identity: &StateIdentity) -> MarkerComparison {
        if !self.matches_ownership(identity) {
            return MarkerComparison::RefuseOwnership;
        }
        if self.runtime_abi != identity.runtime_abi {
            return MarkerComparison::RefuseAbi;
        }
        if self.state_epoch != identity.state_epoch {
            return MarkerComparison::UpgradeEpoch;
        }
        let provenance_matches = self.model_path == identity.model_path
            && self.computed_model_hash == identity.computed_model_hash
            && self.toolchain_id == identity.toolchain_id
            && self.target == identity.target
            && self.persistence == identity.persistence
            && self.cleanup_policy == identity.cleanup_policy;
        if provenance_matches {
            MarkerComparison::Match
        } else {
            MarkerComparison::UpgradeProvenance
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerComparison {
    Match,
    UpgradeProvenance,
    UpgradeEpoch,
    RefuseOwnership,
    RefuseAbi,
}

/// What a run must do with the slot's state root before using it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerDecision {
    /// No marker on disk — first use of the slot.
    Fresh,
    /// The marker matches the requested identity exactly.
    Adopt(StateMarker),
    /// Same owner, different model build: tear down what the old model left
    /// behind, clean the state root when the state epoch changed, and rewrite
    /// the marker.
    Upgrade {
        clean_state: bool,
        existing: StateMarker,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateKind {
    Slot,
}

/// Inspect the slot's marker (read-only) and classify what the run must do
/// before using the state root. Refusals are ownership or runtime-ABI
/// mismatches; a provenance mismatch is returned as an upgrade decision for
/// the caller to process, never silently absorbed.
pub fn evaluate_slot_marker(
    placement: &HostPlacement,
    identity: &StateIdentity,
) -> RuntimeResult<MarkerDecision> {
    let path = marker_path(&placement.state_root);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnowned,
                    format!("state marker is a symlink at {}", path.display()),
                ));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(MarkerDecision::Fresh);
        }
        Err(error) => {
            return Err(RuntimeError::new(
                ErrorCode::StateUnowned,
                format!("failed to inspect state marker {}: {error}", path.display()),
            ));
        }
    }
    let existing = read_marker(&placement.state_root)?;
    match existing.compare(identity) {
        MarkerComparison::Match => Ok(MarkerDecision::Adopt(existing)),
        MarkerComparison::UpgradeProvenance => Ok(MarkerDecision::Upgrade {
            clean_state: false,
            existing,
        }),
        MarkerComparison::UpgradeEpoch => Ok(MarkerDecision::Upgrade {
            clean_state: true,
            existing,
        }),
        MarkerComparison::RefuseOwnership => Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "existing state marker is owned by a different project/environment/slot identity",
        )),
        MarkerComparison::RefuseAbi => Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            "existing state marker was written under a different runtime ABI",
        )),
    }
}

/// Write the slot marker for the requested identity, overwriting any previous
/// marker. The caller must have processed an [`evaluate_slot_marker`] decision
/// first — this is the post-decision commit, not a guard.
pub fn commit_slot_marker(
    placement: &HostPlacement,
    identity: &StateIdentity,
) -> RuntimeResult<StateMarker> {
    let marker = StateMarker::slot(identity);
    let path = marker_path(&placement.state_root);
    if let Ok(metadata) = std::fs::symlink_metadata(&path)
        && metadata.file_type().is_symlink()
    {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("state marker is a symlink at {}", path.display()),
        ));
    }
    let bytes = serde_json::to_vec_pretty(&marker).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to serialize state marker: {error}"),
        )
    })?;
    std::fs::write(&path, bytes).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to write state marker {}: {error}", path.display()),
        )
    })?;
    Ok(marker)
}

pub fn read_marker(target: &Path) -> RuntimeResult<StateMarker> {
    let path = marker_path(target);
    let bytes = std::fs::read(&path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "state marker is missing or unreadable at {}: {error}",
                path.display()
            ),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("state marker is invalid at {}: {error}", path.display()),
        )
    })
}

pub fn marker_path(target: &Path) -> PathBuf {
    target.join(MARKER_FILE_NAME)
}
