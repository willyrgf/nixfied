use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use nixfied_model::{CleanupPolicy, Model, Target};
use serde::{Deserialize, Serialize};

use crate::admission::Admission;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
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
    pub model_path: PathBuf,
    pub computed_model_hash: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target: Target,
}

impl StateIdentity {
    pub fn from_model(model: &Model, admission: &Admission) -> Self {
        Self {
            marker_identity: model.state.marker_identity.clone(),
            project_id: model.project.project_id.clone(),
            environment: "dev".to_string(),
            slot: 0,
            state_epoch: model.state.state_epoch.clone(),
            cleanup_policy: model.state.cleanup_policy.clone(),
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
            model_path: identity.model_path.clone(),
            computed_model_hash: identity.computed_model_hash.clone(),
            runtime_abi: identity.runtime_abi.clone(),
            toolchain_id: identity.toolchain_id.clone(),
            target: identity.target.clone(),
        }
    }

    pub fn matches_identity(&self, identity: &StateIdentity) -> bool {
        self.marker_version == 1
            && self.marker_identity == identity.marker_identity
            && self.project_id == identity.project_id
            && self.environment == identity.environment
            && self.slot == identity.slot
            && self.state_kind == StateKind::Slot
            && self.service_instance_id.is_none()
            && self.state_epoch == identity.state_epoch
            && self.model_path == identity.model_path
            && self.computed_model_hash == identity.computed_model_hash
            && self.runtime_abi == identity.runtime_abi
            && self.toolchain_id == identity.toolchain_id
            && self.target == identity.target
    }

    pub fn matches_declared_marker(&self, identity: &StateIdentity) -> bool {
        self.matches_identity(identity) && self.cleanup_policy == identity.cleanup_policy
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateKind {
    Slot,
}

pub fn write_slot_marker(
    placement: &HostPlacement,
    identity: &StateIdentity,
) -> RuntimeResult<StateMarker> {
    let marker = StateMarker::slot(identity);
    let path = marker_path(&placement.state_root);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnowned,
                    format!("state marker is a symlink at {}", path.display()),
                ));
            }
            let existing = read_marker(&placement.state_root)?;
            if !existing.matches_declared_marker(identity) {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnowned,
                    "existing state marker does not match the requested state identity",
                ));
            }
            return Ok(existing);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(RuntimeError::new(
                ErrorCode::StateUnowned,
                format!("failed to inspect state marker {}: {error}", path.display()),
            ));
        }
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
