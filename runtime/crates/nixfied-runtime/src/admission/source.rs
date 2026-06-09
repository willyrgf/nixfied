use std::path::{Component, Path, PathBuf};

use nixfied_model::{DirtyPolicy, Model, SourceMode};
use serde::Serialize;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmittedSource {
    pub codebase_id: String,
    pub logical_root: String,
    pub observed_root: PathBuf,
    pub source_mode: SourceMode,
    pub source_identity: String,
    pub dirty_policy: DirtyPolicy,
    pub admission_fingerprint_policy: String,
}

pub fn check_source(model: &Model, loaded: &LoadedModel) -> RuntimeResult<AdmittedSource> {
    let [codebase] = model.codebases.as_slice() else {
        return Err(
            RuntimeError::new(ErrorCode::SourceMismatch, "requires exactly one codebase")
                .with_model(&loaded.path, &loaded.computed_model_hash),
        );
    };
    if codebase.codebase_id != "main" || codebase.source_mode != SourceMode::LiveWorkspace {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            "requires codebase main with live-workspace sourceMode",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    match codebase.source_policy.dirty_policy {
        DirtyPolicy::Allow | DirtyPolicy::Warn => {}
        DirtyPolicy::Reject => Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            "runtime cannot prove live workspace cleanliness for dirtyPolicy=reject",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash))?,
    }
    let observed_root = resolve_observed_root(&codebase.logical_root, loaded)?;
    Ok(AdmittedSource {
        codebase_id: codebase.codebase_id.clone(),
        logical_root: codebase.logical_root.clone(),
        observed_root,
        source_mode: codebase.source_mode.clone(),
        source_identity: codebase.source_identity.clone(),
        dirty_policy: codebase.source_policy.dirty_policy.clone(),
        admission_fingerprint_policy: codebase.source_policy.admission_fingerprint_policy.clone(),
    })
}

fn resolve_observed_root(logical_root: &str, loaded: &LoadedModel) -> RuntimeResult<PathBuf> {
    let logical_path = Path::new(logical_root);
    if logical_root.is_empty()
        || logical_path.is_absolute()
        || logical_path.components().any(disallowed_component)
    {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("logicalRoot must be a confined relative path: {logical_root}"),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    let invocation_root = std::env::current_dir().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("failed to inspect invocation root: {error}"),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash)
    })?;
    let invocation_root = invocation_root.canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "failed to canonicalize invocation root {}: {error}",
                invocation_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash)
    })?;
    let observed_root = invocation_root
        .join(logical_path)
        .canonicalize()
        .map_err(|error| {
            RuntimeError::new(
                ErrorCode::SourceMismatch,
                format!("failed to resolve logicalRoot {logical_root}: {error}"),
            )
            .with_model(&loaded.path, &loaded.computed_model_hash)
        })?;
    if !observed_root.is_dir() || !observed_root.starts_with(&invocation_root) {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "logicalRoot {} resolved outside the invocation root",
                observed_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    Ok(observed_root)
}

fn disallowed_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::ParentDir | Component::RootDir | Component::Prefix(_)
    )
}
