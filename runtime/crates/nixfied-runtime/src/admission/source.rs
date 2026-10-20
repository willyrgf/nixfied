use std::path::{Component, Path, PathBuf};

use nixfied_manifest::{DirtyPolicy, SourceMode, ValidatedManifest};
use serde::Serialize;

use super::origin::StoreRoot;
use crate::admission::InvocationRoot;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

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

/// Checked historical provenance, without source or secret admission. Stored
/// bytes can construct this metadata value, never an `AdmittedSource`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedSource {
    codebase_id: String,
    logical_root: String,
    observed_root: String,
    source_mode: SourceMode,
    source_identity: String,
    dirty_policy: DirtyPolicy,
    admission_fingerprint_policy: String,
}

// A closed input DTO is deliberately separate from the output/proof value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecordedSourceWire {
    codebase_id: String,
    logical_root: String,
    observed_root: String,
    source_mode: SourceMode,
    source_identity: String,
    dirty_policy: DirtyPolicy,
    admission_fingerprint_policy: String,
}

impl RecordedSource {
    pub fn decode(json: &str) -> RuntimeResult<Self> {
        let wire: RecordedSourceWire =
            serde_json::from_str(json).map_err(|_| invalid_recorded_source("source_json"))?;
        wire.check()
    }

    /// Derive non-secret comparison metadata from actual admission. This
    /// checks lossless publication, without introducing a second admission.
    pub fn from_admitted(source: &AdmittedSource) -> RuntimeResult<Self> {
        let observed_root =
            crate::output::utf8_locator(&source.observed_root, "source observedRoot")?;
        RecordedSourceWire {
            codebase_id: source.codebase_id.clone(),
            logical_root: source.logical_root.clone(),
            observed_root: observed_root.to_owned(),
            source_mode: source.source_mode,
            source_identity: source.source_identity.clone(),
            dirty_policy: source.dirty_policy,
            admission_fingerprint_policy: source.admission_fingerprint_policy.clone(),
        }
        .check()
    }
}

impl RecordedSourceWire {
    pub(crate) fn from_recorded(source: &RecordedSource) -> Self {
        Self {
            codebase_id: source.codebase_id.clone(),
            logical_root: source.logical_root.clone(),
            observed_root: source.observed_root.clone(),
            source_mode: source.source_mode,
            source_identity: source.source_identity.clone(),
            dirty_policy: source.dirty_policy,
            admission_fingerprint_policy: source.admission_fingerprint_policy.clone(),
        }
    }
    fn check(self) -> RuntimeResult<RecordedSource> {
        if self.codebase_id != "main" {
            return Err(invalid_recorded_source("codebaseId"));
        }
        if self.logical_root.contains('\0')
            || validate_logical_root(&self.logical_root, Path::new(&self.logical_root)).is_err()
        {
            return Err(invalid_recorded_source("logicalRoot"));
        }
        if self.observed_root.contains('\0') || !Path::new(&self.observed_root).is_absolute() {
            return Err(invalid_recorded_source("observedRoot"));
        }
        if self.source_identity.is_empty()
            || (matches!(
                self.source_mode,
                SourceMode::Snapshot | SourceMode::FlakeInput
            ) && (self.source_identity.contains('\0')
                || !Path::new(&self.source_identity).is_absolute()))
        {
            return Err(invalid_recorded_source("sourceIdentity"));
        }
        Ok(RecordedSource {
            codebase_id: self.codebase_id,
            logical_root: self.logical_root,
            observed_root: self.observed_root,
            source_mode: self.source_mode,
            source_identity: self.source_identity,
            dirty_policy: self.dirty_policy,
            admission_fingerprint_policy: self.admission_fingerprint_policy,
        })
    }
}

fn invalid_recorded_source(field: &'static str) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RegistryCorrupt,
        format!("invalid recorded source field {field}"),
    )
}

pub(super) fn check_source(
    manifest: &ValidatedManifest,
    invocation_root: &InvocationRoot,
    store: &StoreRoot<'_>,
) -> RuntimeResult<AdmittedSource> {
    let codebase = &manifest.codebases[0]; // Structural construction proves one main codebase.
    let observed_root = match codebase.source_mode {
        SourceMode::LiveWorkspace => {
            match codebase.source_policy.dirty_policy {
                DirtyPolicy::Allow | DirtyPolicy::Warn => {}
                DirtyPolicy::Reject => Err(RuntimeError::new(
                    ErrorCode::SourceMismatch,
                    "runtime cannot prove live workspace cleanliness for dirtyPolicy=reject",
                ))?,
            }
            resolve_live_observed_root(&codebase.logical_root, invocation_root)?
        }
        SourceMode::Snapshot | SourceMode::FlakeInput => resolve_immutable_observed_root(
            &codebase.source_identity,
            &codebase.logical_root,
            store,
        )?,
    };
    Ok(AdmittedSource {
        codebase_id: codebase.codebase_id.as_str().to_string(),
        logical_root: codebase.logical_root.clone(),
        observed_root,
        source_mode: codebase.source_mode,
        source_identity: codebase.source_identity.clone(),
        dirty_policy: codebase.source_policy.dirty_policy,
        admission_fingerprint_policy: codebase.source_policy.admission_fingerprint_policy.clone(),
    })
}

fn resolve_live_observed_root(
    logical_root: &str,
    invocation_root: &InvocationRoot,
) -> RuntimeResult<PathBuf> {
    let logical_path = Path::new(logical_root);
    validate_logical_root(logical_root, logical_path)?;
    let invocation_root = match invocation_root {
        InvocationRoot::CurrentDirectory => std::env::current_dir().map_err(|error| {
            RuntimeError::new(
                ErrorCode::SourceMismatch,
                format!("failed to inspect invocation root: {error}"),
            )
        })?,
        InvocationRoot::Path(path) => path.clone(),
    };
    let invocation_root = invocation_root.canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "failed to canonicalize invocation root {}: {error}",
                invocation_root.display()
            ),
        )
    })?;
    resolve_confined_directory(
        &invocation_root,
        logical_path,
        &format!("logicalRoot {logical_root}"),
        "the invocation root",
    )
}

fn resolve_immutable_observed_root(
    source_identity: &str,
    logical_root: &str,
    store: &StoreRoot<'_>,
) -> RuntimeResult<PathBuf> {
    let source_root = Path::new(source_identity);
    if source_identity.is_empty() || !source_root.is_absolute() {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("immutable sourceIdentity must be an absolute store path: {source_identity}"),
        ));
    }
    let canonical_source_root = store.canonical_under(source_root).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "immutable sourceIdentity {} is not under {}",
                source_root.display(),
                store.declared.display()
            ),
        )
    })?;
    if !canonical_source_root.is_dir() {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "immutable sourceIdentity {} is not a directory",
                canonical_source_root.display()
            ),
        ));
    }
    let logical_path = Path::new(logical_root);
    validate_logical_root(logical_root, logical_path)?;
    resolve_confined_directory(
        &canonical_source_root,
        logical_path,
        &format!(
            "immutable logicalRoot {logical_root} under {}",
            canonical_source_root.display()
        ),
        &format!("immutable source root {}", canonical_source_root.display()),
    )
}

fn resolve_confined_directory(
    root: &Path,
    logical_path: &Path,
    resolve_context: &str,
    escape_context: &str,
) -> RuntimeResult<PathBuf> {
    let observed_root = root.join(logical_path).canonicalize().map_err(|error| {
        RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("failed to resolve {resolve_context}: {error}"),
        )
    })?;
    if !observed_root.is_dir() || !observed_root.starts_with(root) {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!(
                "logicalRoot {} resolved outside {escape_context}",
                observed_root.display()
            ),
        ));
    }
    Ok(observed_root)
}

fn validate_logical_root(logical_root: &str, logical_path: &Path) -> RuntimeResult<()> {
    if logical_root.is_empty()
        || logical_path.is_absolute()
        || logical_path.components().any(disallowed_component)
    {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            format!("logicalRoot must be a confined relative path: {logical_root}"),
        ));
    }
    Ok(())
}

fn disallowed_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::ParentDir | Component::RootDir | Component::Prefix(_)
    )
}
