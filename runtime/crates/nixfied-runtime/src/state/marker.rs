use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use nixfied_manifest::{CleanupPolicy, PersistencePolicy, Target};
use serde::{Deserialize, Serialize};

use crate::admission::ControlAdmission;
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
    pub cleanup_policy: CleanupPolicy,
    pub persistence: PersistencePolicy,
    pub manifest_path: PathBuf,
    pub computed_manifest_hash: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target: Target,
}

impl StateIdentity {
    pub fn from_admission(admission: &ControlAdmission) -> Self {
        Self::for_slot(admission, "dev", 0)
    }

    pub fn from_selected_slot(
        admission: &ControlAdmission,
        selected_slot: &SelectedSlot<'_>,
    ) -> Self {
        Self::for_slot(admission, selected_slot.environment, selected_slot.slot)
    }

    pub fn for_slot(admission: &ControlAdmission, environment: &str, slot: u32) -> Self {
        let manifest = admission.manifest();
        Self {
            marker_identity: manifest.state.marker_identity.clone(),
            project_id: manifest.project.project_id.clone(),
            environment: environment.to_string(),
            slot,
            cleanup_policy: manifest.state.cleanup_policy.clone(),
            persistence: manifest.state.persistence.clone(),
            manifest_path: admission.manifest_path().to_path_buf(),
            computed_manifest_hash: admission.computed_manifest_hash().to_owned(),
            runtime_abi: admission.runtime_abi().to_owned(),
            toolchain_id: admission.toolchain_id().to_owned(),
            target: manifest.target.clone(),
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
    pub cleanup_policy: CleanupPolicy,
    pub persistence: PersistencePolicy,
    pub manifest_path: PathBuf,
    pub computed_manifest_hash: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target: Target,
}

impl StateMarker {
    pub fn slot(identity: &StateIdentity) -> Self {
        Self {
            marker_version: 2,
            marker_identity: identity.marker_identity.clone(),
            project_id: identity.project_id.clone(),
            environment: identity.environment.clone(),
            slot: identity.slot,
            state_kind: StateKind::Slot,
            service_instance_id: None,
            cleanup_policy: identity.cleanup_policy.clone(),
            persistence: identity.persistence.clone(),
            manifest_path: identity.manifest_path.clone(),
            computed_manifest_hash: identity.computed_manifest_hash.clone(),
            runtime_abi: identity.runtime_abi.clone(),
            toolchain_id: identity.toolchain_id.clone(),
            target: identity.target.clone(),
        }
    }

    /// Whether this marker's state root belongs to the requested identity at
    /// all: same project, environment, slot, and marker scheme. Ownership is
    /// deliberately blind to which manifest build last used the root — a manifest
    /// evolves, its slot does not.
    pub fn matches_ownership(&self, identity: &StateIdentity) -> bool {
        self.marker_version == 2
            && self.marker_identity == identity.marker_identity
            && self.project_id == identity.project_id
            && self.environment == identity.environment
            && self.slot == identity.slot
            && self.state_kind == StateKind::Slot
            && self.service_instance_id.is_none()
    }

    /// Ownership, framework ABI, and existing retention authorization gate
    /// access. Other differences update provenance without declaring application
    /// data compatibility or authorizing deletion.
    pub fn compare(&self, identity: &StateIdentity) -> MarkerComparison {
        if !self.matches_ownership(identity) {
            return MarkerComparison::RefuseOwnership;
        }
        if self.runtime_abi != identity.runtime_abi {
            return MarkerComparison::RefuseAbi;
        }
        if (self.persistence == PersistencePolicy::Persistent
            && identity.persistence != PersistencePolicy::Persistent)
            || (self.cleanup_policy == CleanupPolicy::Protected
                && identity.cleanup_policy != CleanupPolicy::Protected)
        {
            return MarkerComparison::RefuseRetention;
        }
        let provenance_matches = self.manifest_path == identity.manifest_path
            && self.computed_manifest_hash == identity.computed_manifest_hash
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
    RefuseRetention,
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
    /// Same owner and retention authorization, different recorded provenance.
    Upgrade { existing: StateMarker },
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
            refuse_unmarked_state_root(&placement.state_root)?;
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
        MarkerComparison::UpgradeProvenance => Ok(MarkerDecision::Upgrade { existing }),
        MarkerComparison::RefuseRetention => Err(RuntimeError::new(
            ErrorCode::CleanupRefused,
            "state preparation cannot weaken existing retention authorization",
        )),
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

/// A missing marker only means a fresh slot when the state root itself is
/// absent (or an empty directory). A state root with content but no marker is
/// state the runtime never claimed — adopting it would write a marker into an
/// unowned tree that cleanup rightly refuses, so the run refuses it too.
fn refuse_unmarked_state_root(state_root: &Path) -> RuntimeResult<()> {
    let metadata = match std::fs::symlink_metadata(state_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(RuntimeError::new(
                ErrorCode::StateUnowned,
                format!(
                    "failed to inspect state root {}: {error}",
                    state_root.display()
                ),
            ));
        }
    };
    if !metadata.file_type().is_dir() {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "state root {} exists but is not a directory",
                state_root.display()
            ),
        ));
    }
    let mut entries = std::fs::read_dir(state_root).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "failed to inspect state root {}: {error}",
                state_root.display()
            ),
        )
    })?;
    if entries.next().is_some() {
        return Err(RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "state root {} exists without a state marker; refusing to adopt unmarked state",
                state_root.display()
            ),
        ));
    }
    Ok(())
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
