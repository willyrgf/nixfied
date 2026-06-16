use std::path::{Component, Path, PathBuf};

use nixfied_model::{DirtyPolicy, Model, SourceMode};
use serde::Serialize;

use crate::admission::AdmissionContext;
use crate::admission::origin;
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

pub fn check_source(
    model: &Model,
    loaded: &LoadedModel,
    context: &AdmissionContext,
) -> RuntimeResult<AdmittedSource> {
    let [codebase] = model.codebases.as_slice() else {
        return Err(
            RuntimeError::new(ErrorCode::SourceMismatch, "requires exactly one codebase")
                .with_model(&loaded.path, &loaded.computed_model_hash),
        );
    };
    if codebase.codebase_id.as_str() != "main" {
        return Err(
            RuntimeError::new(ErrorCode::SourceMismatch, "requires codebase main")
                .with_model(&loaded.path, &loaded.computed_model_hash),
        );
    }
    let observed_root = match codebase.source_mode {
        SourceMode::LiveWorkspace => {
            match codebase.source_policy.dirty_policy {
                DirtyPolicy::Allow | DirtyPolicy::Warn => {}
                DirtyPolicy::Reject => Err(RuntimeError::new(
                    ErrorCode::SourceMismatch,
                    "runtime cannot prove live workspace cleanliness for dirtyPolicy=reject",
                )
                .with_model(&loaded.path, &loaded.computed_model_hash))?,
            }
            resolve_live_observed_root(&codebase.logical_root, loaded)?
        }
        SourceMode::Snapshot | SourceMode::FlakeInput => resolve_immutable_observed_root(
            &codebase.source_identity,
            &codebase.logical_root,
            loaded,
            context,
        )?,
    };
    Ok(AdmittedSource {
        codebase_id: codebase.codebase_id.as_str().to_string(),
        logical_root: codebase.logical_root.clone(),
        observed_root,
        source_mode: codebase.source_mode.clone(),
        source_identity: codebase.source_identity.clone(),
        dirty_policy: codebase.source_policy.dirty_policy.clone(),
        admission_fingerprint_policy: codebase.source_policy.admission_fingerprint_policy.clone(),
    })
}

fn resolve_live_observed_root(logical_root: &str, loaded: &LoadedModel) -> RuntimeResult<PathBuf> {
    let logical_path = Path::new(logical_root);
    validate_logical_root(logical_root, logical_path, loaded)?;
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

fn resolve_immutable_observed_root(
    source_identity: &str,
    logical_root: &str,
    loaded: &LoadedModel,
    context: &AdmissionContext,
) -> RuntimeResult<PathBuf> {
    let source_root = Path::new(source_identity);
    if source_identity.is_empty() || !source_root.is_absolute() {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("immutable sourceIdentity must be an absolute store path: {source_identity}"),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    if !origin::canonical_is_under_store(source_root, &context.store_root) {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "immutable sourceIdentity {} is not under {}",
                source_root.display(),
                context.store_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    let canonical_source_root = source_root.canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "failed to canonicalize immutable sourceIdentity {}: {error}",
                source_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash)
    })?;
    if !canonical_source_root.is_dir() {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "immutable sourceIdentity {} is not a directory",
                canonical_source_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    let logical_path = Path::new(logical_root);
    validate_logical_root(logical_root, logical_path, loaded)?;
    let observed_root = canonical_source_root
        .join(logical_path)
        .canonicalize()
        .map_err(|error| {
            RuntimeError::new(
                ErrorCode::SourceMismatch,
                format!(
                    "failed to resolve immutable logicalRoot {logical_root} under {}: {error}",
                    canonical_source_root.display()
                ),
            )
            .with_model(&loaded.path, &loaded.computed_model_hash)
        })?;
    if !observed_root.is_dir() || !observed_root.starts_with(&canonical_source_root) {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "logicalRoot {} resolved outside immutable source root {}",
                observed_root.display(),
                canonical_source_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    Ok(observed_root)
}

fn validate_logical_root(
    logical_root: &str,
    logical_path: &Path,
    loaded: &LoadedModel,
) -> RuntimeResult<()> {
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
    Ok(())
}

fn disallowed_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::ParentDir | Component::RootDir | Component::Prefix(_)
    )
}
