use std::path::{Component, Path, PathBuf};

use nixfied_manifest::Manifest;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::slot::SelectedSlot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPlacement {
    pub state_base: PathBuf,
    pub state_root: PathBuf,
    pub registry_dir: PathBuf,
    pub run_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub summary_path: PathBuf,
}

impl HostPlacement {
    pub fn registry_path(&self) -> PathBuf {
        self.registry_dir.join("registry.sqlite3")
    }
}

pub fn state_base_from_env() -> RuntimeResult<PathBuf> {
    if let Some(value) = std::env::var_os("NIXFIED_STATE_DIR")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    default_state_base()
}

pub fn default_state_base() -> RuntimeResult<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_STATE_HOME")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value).join("nixfied"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            "HOME is not set and NIXFIED_STATE_DIR was not provided",
        )
    })?;
    let home = PathBuf::from(home);
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Application Support/nixfied"))
    } else {
        Ok(home.join(".local/state/nixfied"))
    }
}

pub fn derive_host_placement(
    manifest: &Manifest,
    run_id: &str,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<HostPlacement> {
    let selected_slot = crate::slot::select_slot(manifest, None)?;
    derive_host_placement_for_slot(manifest, &selected_slot, run_id, state_base)
}

pub fn derive_host_placement_for_slot(
    manifest: &Manifest,
    selected_slot: &SelectedSlot<'_>,
    run_id: &str,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<HostPlacement> {
    derive_slot_placement(
        &manifest.project.project_id,
        selected_slot.environment,
        selected_slot.slot,
        run_id,
        state_base,
    )
}

/// Native slot placement shared by admitted execution and registry ownership.
pub fn derive_slot_placement(
    project: &str,
    environment: &str,
    slot: u32,
    run_id: &str,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<HostPlacement> {
    let state_base = state_base.as_ref().to_path_buf();
    if state_base.as_os_str().is_empty() {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            "state base cannot be empty",
        ));
    }
    let project = normal_component("projectId", project)?;
    let environment = normal_component("environment", environment)?;
    let run_id = normal_component("runId", run_id)?;
    let slot_relative = project.join(environment).join(slot.to_string());
    let state_root = application_root(&state_base, project, environment, slot);
    // Cleanup evidence survives deletion of the parallel slot state tree.
    let registry_dir = state_base.join("registry").join(&slot_relative);
    let run_dir = registry_dir.join("runs").join(run_id);
    let logs_dir = run_dir.join("logs");
    let artifacts_dir = run_dir.join("artifacts");
    let summary_path = run_dir.join("summary.json");
    Ok(HostPlacement {
        state_base,
        state_root,
        registry_dir,
        run_dir,
        logs_dir,
        artifacts_dir,
        summary_path,
    })
}

pub(crate) fn normal_component<'a>(field: &str, value: &'a str) -> RuntimeResult<&'a Path> {
    let path = Path::new(value);
    let mut components = path.components();
    let normal = matches!(components.next(), Some(Component::Normal(part)) if part == value)
        && components.next().is_none();
    if value.is_empty() || value.contains(['/', '\0']) || value.contains("${") || !normal {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("{field} must be a single normal path component without template syntax"),
        ));
    }
    Ok(path)
}

/// Application data and coordination/evidence have structurally disjoint roots.
pub(crate) fn application_root(
    base: &Path,
    project: &Path,
    environment: &Path,
    slot: u32,
) -> PathBuf {
    base.join("data")
        .join(project)
        .join(environment)
        .join(slot.to_string())
}
