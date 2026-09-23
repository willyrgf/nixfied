mod closures;
mod origin;
pub mod secrets;
pub mod source;
mod target;

use std::path::{Path, PathBuf};

use origin::StoreRoot;

use crate::error::RuntimeResult;
use crate::execution::{ExecutionManifest, lower};
use crate::manifest_loader::{LoadedManifest, parse_loaded_manifest, read_raw_manifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOriginPolicy {
    RequireStore,
    AllowNonStoreForTests,
}

#[derive(Debug, Clone)]
pub enum InvocationRoot {
    CurrentDirectory,
    Path(PathBuf),
}

#[derive(Debug, Clone)]
pub struct AdmissionContext {
    pub policy: StoreOriginPolicy,
    pub store_root: PathBuf,
    pub host_system: String,
    pub invocation_root: InvocationRoot,
}

impl AdmissionContext {
    pub fn current(policy: StoreOriginPolicy) -> Self {
        Self {
            policy,
            store_root: PathBuf::from("/nix/store"),
            host_system: host_system(),
            invocation_root: InvocationRoot::CurrentDirectory,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ControlAdmission {
    loaded: LoadedManifest,
    execution_manifest: ExecutionManifest,
    generator_json: String,
    target_json: String,
}

impl ControlAdmission {
    fn new(loaded: LoadedManifest, execution_manifest: ExecutionManifest) -> Self {
        Self {
            generator_json: serde_json::to_string(&loaded.manifest().generator).unwrap_or_default(),
            target_json: serde_json::to_string(&loaded.manifest().target).unwrap_or_default(),
            loaded,
            execution_manifest,
        }
    }

    pub fn manifest(&self) -> &nixfied_manifest::ValidatedManifest {
        self.loaded.manifest()
    }
    pub fn manifest_path(&self) -> &Path {
        self.loaded.path()
    }
    pub fn computed_manifest_hash(&self) -> &str {
        self.loaded.computed_manifest_hash()
    }
    pub fn raw_len(&self) -> usize {
        self.loaded.raw_len()
    }
    pub fn project_id(&self) -> &str {
        &self.manifest().project.project_id
    }
    pub fn runtime_abi(&self) -> &str {
        &self.manifest().runtime_abi
    }
    pub fn toolchain_id(&self) -> &str {
        &self.manifest().toolchain_id
    }
    pub fn target_system(&self) -> &str {
        &self.manifest().target.system
    }
    pub fn execution_manifest(&self) -> &ExecutionManifest {
        &self.execution_manifest
    }
    pub fn generator_json(&self) -> &str {
        &self.generator_json
    }
    pub fn target_json(&self) -> &str {
        &self.target_json
    }
}

#[derive(Debug, Clone)]
pub struct RunAdmission {
    common: ControlAdmission,
    source: source::AdmittedSource,
    secrets: secrets::ResolvedSecrets,
}

impl RunAdmission {
    pub fn common(&self) -> &ControlAdmission {
        &self.common
    }
    pub fn source(&self) -> &source::AdmittedSource {
        &self.source
    }
    pub fn secrets(&self) -> &secrets::ResolvedSecrets {
        &self.secrets
    }
}

/// Run/check admission owns the bytes and every prerequisite for child execution.
pub fn admit_run(path: &Path, context: &AdmissionContext) -> RuntimeResult<RunAdmission> {
    let (loaded, store) = load_for_admission(path, context)?;
    let (source, secrets, execution_manifest) = (|| {
        let source = source::check_source(loaded.manifest(), &context.invocation_root, &store)?;
        let secrets = secrets::resolve_secrets(loaded.manifest())?;
        let execution_manifest = finish_admission(&loaded, &store)?;
        Ok((source, secrets, execution_manifest))
    })()
    .map_err(|error: crate::error::RuntimeError| {
        error.with_manifest_if_missing(
            loaded.path().to_path_buf(),
            loaded.computed_manifest_hash().to_owned(),
        )
    })?;
    Ok(RunAdmission {
        common: ControlAdmission::new(loaded, execution_manifest),
        source,
        secrets,
    })
}

/// Recovery admits source-independent facts without fetching source or secret values.
pub fn admit_control(path: &Path, context: &AdmissionContext) -> RuntimeResult<ControlAdmission> {
    let (loaded, store) = load_for_admission(path, context)?;
    let execution_manifest = (|| {
        secrets::check_secret_references(loaded.manifest())?;
        finish_admission(&loaded, &store)
    })()
    .map_err(|error| {
        error.with_manifest_if_missing(
            loaded.path().to_path_buf(),
            loaded.computed_manifest_hash().to_owned(),
        )
    })?;
    Ok(ControlAdmission::new(loaded, execution_manifest))
}

fn load_for_admission<'a>(
    path: &Path,
    context: &'a AdmissionContext,
) -> RuntimeResult<(LoadedManifest, StoreRoot<'a>)> {
    let raw = read_raw_manifest(path)?;
    let store = StoreRoot::observe(&context.store_root);
    origin::check_raw_store_origin(&raw, context.policy, &store)?;
    let loaded = parse_loaded_manifest(raw)?;
    target::check_target(loaded.manifest(), context).map_err(|error| {
        error.with_manifest_if_missing(
            loaded.path().to_path_buf(),
            loaded.computed_manifest_hash().to_owned(),
        )
    })?;
    Ok((loaded, store))
}

fn finish_admission(
    loaded: &LoadedManifest,
    store: &StoreRoot<'_>,
) -> RuntimeResult<ExecutionManifest> {
    closures::check_closures(loaded.manifest(), store)?;
    lower(loaded.manifest())
}

fn host_system() -> String {
    let arch = std::env::consts::ARCH;
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    format!("{arch}-{os}")
}
