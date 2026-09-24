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
    let state_base = state_base.as_ref().to_path_buf();
    if state_base.as_os_str().is_empty() {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            "state base cannot be empty",
        ));
    }
    let project = normal_component("projectId", &manifest.project.project_id)?;
    let environment = normal_component("environment", selected_slot.environment)?;
    let run_id = normal_component("runId", run_id)?;
    let slot_relative = project
        .join(environment)
        .join(selected_slot.slot.to_string());
    let state_root = application_root(&state_base, project, environment, selected_slot.slot);
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

pub fn materialize_run_roots(placement: &HostPlacement) -> RuntimeResult<()> {
    materialize_state_root(placement)
}

/// Materialize only the state base and registry dir. The registry must exist
/// before the slot marker is evaluated: the marker decision may need registry
/// evidence (stale processes from an older manifest build), and the registry
/// outlives a slot clean that deletes the state root.
pub fn materialize_registry_root(placement: &HostPlacement) -> RuntimeResult<()> {
    create_dir(&placement.state_base)?;
    let base = canonicalize_materialized("state base", &placement.state_base)?;
    materialize_owned_dir(&placement.state_base, &base, &placement.registry_dir)
}

/// Materialize application state and this run's retained evidence directories.
/// Runs after the marker decision so an upgrade-clean can delete previous
/// application data without deleting run evidence.
pub fn materialize_state_root(placement: &HostPlacement) -> RuntimeResult<()> {
    create_dir(&placement.state_base)?;
    let base = canonicalize_materialized("state base", &placement.state_base)?;
    materialize_owned_dir(&placement.state_base, &base, &placement.state_root)?;
    materialize_registry_root(placement)?;
    let root = canonicalize_materialized("registry directory", &placement.registry_dir)?;
    materialize_owned_dir(&placement.registry_dir, &root, &placement.run_dir)?;
    materialize_owned_dir(&placement.registry_dir, &root, &placement.logs_dir)?;
    materialize_owned_dir(&placement.registry_dir, &root, &placement.artifacts_dir)?;
    Ok(())
}

pub(crate) fn canonicalize_existing(label: &str, path: &Path) -> RuntimeResult<PathBuf> {
    path.canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("failed to canonicalize {label} {}: {error}", path.display()),
        )
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

fn materialize_owned_dir(
    owner_root: &Path,
    canonical_owner_root: &Path,
    path: &Path,
) -> RuntimeResult<()> {
    if !path.starts_with(owner_root) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!(
                "state path {} escapes owner root {}",
                path.display(),
                owner_root.display()
            ),
        ));
    }
    reject_existing_symlink_components(owner_root, path)?;
    create_dir(path)?;
    let canonical_path = canonicalize_materialized("state path", path)?;
    if !canonical_path.starts_with(canonical_owner_root) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!(
                "state path {} escapes owner root {}",
                canonical_path.display(),
                canonical_owner_root.display()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn reject_existing_symlink_components(
    owner_root: &Path,
    path: &Path,
) -> RuntimeResult<()> {
    let relative = path.strip_prefix(owner_root).map_err(|_| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!(
                "state path {} escapes owner root {}",
                path.display(),
                owner_root.display()
            ),
        )
    })?;
    let mut current = owner_root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(part) => current.push(part),
            Component::CurDir => continue,
            _ => {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnwritable,
                    format!("state path {} contains traversal", path.display()),
                ));
            }
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&current)
            && metadata.file_type().is_symlink()
        {
            return Err(RuntimeError::new(
                ErrorCode::StateUnwritable,
                format!("state path traverses symlink {}", current.display()),
            ));
        }
    }
    Ok(())
}

fn create_dir(path: &Path) -> RuntimeResult<()> {
    std::fs::create_dir_all(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to create {}: {error}", path.display()),
        )
    })
}

fn canonicalize_materialized(label: &str, path: &Path) -> RuntimeResult<PathBuf> {
    path.canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to canonicalize {label} {}: {error}", path.display()),
        )
    })
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
