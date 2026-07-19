use std::path::{Component, Path, PathBuf};

use nixfied_model::Model;

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
    model: &Model,
    run_id: &str,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<HostPlacement> {
    let selected_slot = crate::slot::select_slot(model, None)?;
    derive_host_placement_for_slot(model, &selected_slot, run_id, state_base)
}

/// The directory layout the runtime owns. These were pinned constants in every
/// model; the runtime is their single source of truth.
const STATE_ROOT_TEMPLATE: &str = "${projectId}/${environment}/${slot}";
const REGISTRY_DIR: &str = "registry";
const RUN_DIR_TEMPLATE: &str = "runs/${runId}";
const LOGS_DIR_TEMPLATE: &str = "runs/${runId}/logs";
const ARTIFACTS_DIR_TEMPLATE: &str = "runs/${runId}/artifacts";

pub fn derive_host_placement_for_slot(
    model: &Model,
    selected_slot: &SelectedSlot<'_>,
    run_id: &str,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<HostPlacement> {
    let vars = TemplateVars {
        project_id: &model.project.project_id,
        environment: selected_slot.environment,
        slot: &selected_slot.slot.to_string(),
        run_id,
    };
    let state_base = state_base.as_ref().to_path_buf();
    if state_base.as_os_str().is_empty() {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            "state base cannot be empty",
        ));
    }
    let state_root_relative = relative_template_path("state root", STATE_ROOT_TEMPLATE, &vars)?;
    let state_root = state_base.join(&state_root_relative);
    // The registry holds cleanup evidence and must survive a slot clean, which
    // deletes the state root. Place it in a parallel tree under the state base
    // keyed by the same per-slot path, never inside the deleted state root.
    let registry_dir = state_base
        .join(relative_template_path("registry dir", REGISTRY_DIR, &vars)?)
        .join(&state_root_relative);
    let run_dir = state_root.join(relative_template_path("run dir", RUN_DIR_TEMPLATE, &vars)?);
    let logs_dir = state_root.join(relative_template_path(
        "logs dir",
        LOGS_DIR_TEMPLATE,
        &vars,
    )?);
    let artifacts_dir = state_root.join(relative_template_path(
        "artifacts dir",
        ARTIFACTS_DIR_TEMPLATE,
        &vars,
    )?);
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
    materialize_registry_root(placement)?;
    materialize_state_root(placement)
}

/// Materialize only the state base and registry dir. The registry must exist
/// before the slot marker is evaluated: the marker decision may need registry
/// evidence (stale processes from an older model build), and the registry
/// outlives a slot clean that deletes the state root.
pub fn materialize_registry_root(placement: &HostPlacement) -> RuntimeResult<()> {
    create_dir(&placement.state_base)?;
    let base = canonicalize_materialized("state base", &placement.state_base)?;
    materialize_owned_dir(&placement.state_base, &base, &placement.registry_dir)
}

/// Materialize the state root and this run's run/logs/artifacts dirs. Runs
/// after the marker decision so an upgrade-clean can delete the previous state
/// root before the new one is created.
pub fn materialize_state_root(placement: &HostPlacement) -> RuntimeResult<()> {
    create_dir(&placement.state_base)?;
    let base = canonicalize_materialized("state base", &placement.state_base)?;
    materialize_owned_dir(&placement.state_base, &base, &placement.state_root)?;
    let root = canonicalize_materialized("state root", &placement.state_root)?;
    materialize_owned_dir(&placement.state_root, &root, &placement.run_dir)?;
    materialize_owned_dir(&placement.state_root, &root, &placement.logs_dir)?;
    materialize_owned_dir(&placement.state_root, &root, &placement.artifacts_dir)?;
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

fn relative_template_path(
    field: &'static str,
    template: &str,
    vars: &TemplateVars<'_>,
) -> RuntimeResult<PathBuf> {
    let expanded = expand_template(template, vars);
    if expanded.contains("${") {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("{field} contains an unsupported template variable"),
        ));
    }
    let path = PathBuf::from(expanded);
    if !is_safe_relative_path(&path) {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("{field} must be a relative path without traversal"),
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

fn reject_existing_symlink_components(owner_root: &Path, path: &Path) -> RuntimeResult<()> {
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

fn expand_template(template: &str, vars: &TemplateVars<'_>) -> String {
    template
        .replace("${projectId}", vars.project_id)
        .replace("${environment}", vars.environment)
        .replace("${slot}", vars.slot)
        .replace("${runId}", vars.run_id)
}

fn is_safe_relative_path(path: &Path) -> bool {
    let mut has_normal_component = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_normal_component = true,
            Component::CurDir => {}
            _ => return false,
        }
    }
    has_normal_component
}

struct TemplateVars<'a> {
    project_id: &'a str,
    environment: &'a str,
    slot: &'a str,
    run_id: &'a str,
}
