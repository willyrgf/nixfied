use std::path::PathBuf;

use nixfied_manifest::{PersistencePolicy, Target};
use serde::{Deserialize, Serialize};

use crate::admission::ControlAdmission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::filesystem::{Directory, EntryKind, is_temporary_of};
use crate::registry::Registry;
use crate::slot::SelectedSlot;
use crate::state::tree::{ApplicationTree, MARKER, Observed, read_root_marker};

pub const MARKER_FILE_NAME: &str = ".nixfied-state.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateIdentity {
    pub marker_identity: String,
    pub project_id: String,
    pub environment: String,
    pub slot: u32,
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
            persistence: manifest.state.persistence,
            manifest_path: admission.manifest_path().to_path_buf(),
            computed_manifest_hash: admission.computed_manifest_hash().to_owned(),
            runtime_abi: admission.runtime_abi().to_owned(),
            toolchain_id: admission.toolchain_id().to_owned(),
            target: manifest.target.clone(),
        }
    }
}

pub const MARKER_VERSION: u32 = 3;

/// The ownership and retention authority for one application-data generation.
/// Its generation distinguishes successive trees in cleanup history; it never
/// declares application-data compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateMarker {
    pub marker_version: u32,
    pub marker_identity: String,
    pub project_id: String,
    pub environment: String,
    pub slot: u32,
    pub persistence: PersistencePolicy,
    pub data_generation: String,
    pub manifest_path: PathBuf,
    pub computed_manifest_hash: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target: Target,
}

impl StateMarker {
    /// A marker for a newly created application tree with a fresh generation.
    pub fn slot(identity: &StateIdentity) -> RuntimeResult<Self> {
        Ok(Self::with_generation(
            identity,
            format!("gen-{}", crate::token::random_hex()?),
        ))
    }

    /// Refresh provenance for an existing tree while preserving its generation.
    fn refreshed(identity: &StateIdentity, existing: &Self) -> Self {
        Self::with_generation(identity, existing.data_generation.clone())
    }

    fn with_generation(identity: &StateIdentity, data_generation: String) -> Self {
        Self {
            marker_version: MARKER_VERSION,
            marker_identity: identity.marker_identity.clone(),
            project_id: identity.project_id.clone(),
            environment: identity.environment.clone(),
            slot: identity.slot,
            persistence: identity.persistence,
            data_generation,
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
        self.marker_version == MARKER_VERSION
            && !self.data_generation.is_empty()
            && self.marker_identity == identity.marker_identity
            && self.project_id == identity.project_id
            && self.environment == identity.environment
            && self.slot == identity.slot
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
        if self.persistence == PersistencePolicy::Persistent
            && identity.persistence != PersistencePolicy::Persistent
        {
            return MarkerComparison::RefuseRetention;
        }
        let provenance_matches = self.manifest_path == identity.manifest_path
            && self.computed_manifest_hash == identity.computed_manifest_hash
            && self.toolchain_id == identity.toolchain_id
            && self.target == identity.target
            && self.persistence == identity.persistence;
        if provenance_matches {
            MarkerComparison::Match
        } else {
            MarkerComparison::RefreshProvenance
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerComparison {
    Match,
    RefreshProvenance,
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
    Refresh { existing: StateMarker },
}

/// Inspect the slot's marker (read-only) through the slot guard's held
/// descriptor and classify what the run must do before using the state root.
/// Refusals are ownership or runtime-ABI mismatches; a provenance mismatch is
/// returned as a refresh decision for the caller to process, never silently
/// absorbed.
pub fn evaluate_slot_marker(
    registry: &Registry,
    identity: &StateIdentity,
) -> RuntimeResult<MarkerDecision> {
    let tree = ApplicationTree::new(registry.authority(), identity)?;
    let root = match tree.open()? {
        Observed::Absent => return Ok(MarkerDecision::Fresh),
        Observed::Present { root, .. } => root,
    };
    let Some(existing) = read_root_marker(&root)? else {
        refuse_unmarked_state_root(&tree, &root)?;
        return Ok(MarkerDecision::Fresh);
    };
    match existing.compare(identity) {
        MarkerComparison::Match => Ok(MarkerDecision::Adopt(existing)),
        MarkerComparison::RefreshProvenance => Ok(MarkerDecision::Refresh { existing }),
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

/// A missing marker only means a fresh slot when the state root is empty, or
/// holds only temporaries of an interrupted first marker publication. Other
/// content is state the runtime never claimed — adopting it would write a
/// marker into an unowned tree that cleanup rightly refuses, so the run
/// refuses it too.
fn refuse_unmarked_state_root(tree: &ApplicationTree, root: &Directory) -> RuntimeResult<()> {
    for name in root.entry_names().map_err(|error| tree.io_error(error))? {
        if !interrupted_publication(root, &name).map_err(|error| tree.io_error(error))? {
            return Err(RuntimeError::new(
                ErrorCode::StateUnowned,
                format!(
                    "state root {} exists without a state marker; refusing to adopt unmarked state",
                    tree.path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn interrupted_publication(root: &Directory, name: &std::ffi::CStr) -> std::io::Result<bool> {
    Ok(is_temporary_of(name, MARKER) && matches!(root.entry(name)?, Some(EntryKind::File(_))))
}

/// Publish the marker for a fresh application tree, creating the tree through
/// the slot guard's held descriptor. Temporaries of an interrupted earlier
/// publication are removed first. The caller must have processed an
/// [`evaluate_slot_marker`] decision first — this is the post-decision commit,
/// not a guard.
pub fn commit_slot_marker(
    registry: &Registry,
    identity: &StateIdentity,
) -> RuntimeResult<StateMarker> {
    let tree = ApplicationTree::new(registry.authority(), identity)?;
    let root = tree.materialize()?;
    for name in root.entry_names().map_err(|error| tree.io_error(error))? {
        if interrupted_publication(&root, &name).map_err(|error| tree.io_error(error))? {
            root.remove_entry(&name, false)
                .map_err(|error| tree.io_error(error))?;
        }
    }
    let marker = StateMarker::slot(identity)?;
    tree.publish_marker(&root, &marker)?;
    Ok(marker)
}

/// Refresh provenance of an existing tree without changing its generation.
pub fn refresh_slot_marker(
    registry: &Registry,
    identity: &StateIdentity,
    existing: &StateMarker,
) -> RuntimeResult<StateMarker> {
    let tree = ApplicationTree::new(registry.authority(), identity)?;
    let root = tree.materialize()?;
    let marker = StateMarker::refreshed(identity, existing);
    tree.publish_marker(&root, &marker)?;
    Ok(marker)
}
