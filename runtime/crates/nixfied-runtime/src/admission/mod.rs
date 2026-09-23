pub mod abi;
pub mod closures;
pub mod origin;
pub mod secrets;
pub mod source;
pub mod target;

use std::path::PathBuf;

use nixfied_manifest::Manifest;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecutionManifest, lower, prove_all_plans_feasible};
use crate::manifest_loader::LoadedManifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOriginPolicy {
    RequireStore,
    AllowNonStoreForTests,
}

#[derive(Debug, Clone)]
pub struct AdmissionContext {
    pub policy: StoreOriginPolicy,
    pub store_root: PathBuf,
    pub host_system: String,
}

impl AdmissionContext {
    pub fn current(policy: StoreOriginPolicy) -> Self {
        Self {
            policy,
            store_root: PathBuf::from("/nix/store"),
            host_system: host_system(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Admission {
    pub manifest_path: PathBuf,
    pub computed_manifest_hash: String,
    pub raw_len: usize,
    pub project_id: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target_system: String,
    /// The resolved source root. Present for run admission; `None` for control
    /// admission (`ps`/`down`/`clean`), which must operate on a slot from the store
    /// manifest and registry alone and so does not resolve source from the caller.
    pub source: Option<source::AdmittedSource>,
    /// Provenance serialized once at admission for the run record, so the executor
    /// records it without reading the raw `Manifest`.
    pub generator_json: String,
    pub target_json: String,
    /// The lowered, executable view of the manifest. Admission proves a concrete plan
    /// exists for every slot/selection; the executor consumes only this.
    pub execution_manifest: ExecutionManifest,
    /// Secret material resolved once during run/check admission. Control admission
    /// only proves references and leaves this empty because ps/down/clean never
    /// spawn children.
    pub secrets: secrets::ResolvedSecrets,
}

impl Admission {
    /// Run admission: admit and lower the manifest, and resolve the declared source
    /// root so the executor can spawn execs from it.
    pub fn check(loaded: &LoadedManifest, context: &AdmissionContext) -> RuntimeResult<Self> {
        Self::admit(loaded, context, true)
    }

    /// Control admission for recovery commands (`ps`/`down`/`clean`). Admits and
    /// lowers the manifest but does NOT resolve the live workspace: control must
    /// reconcile, stop, and clean a slot from the store manifest and registry alone,
    /// so it cannot fail because the caller is outside the project root or the
    /// workspace has moved or been deleted while services stay registered.
    pub fn check_for_control(
        loaded: &LoadedManifest,
        context: &AdmissionContext,
    ) -> RuntimeResult<Self> {
        Self::admit(loaded, context, false)
    }

    fn admit(
        loaded: &LoadedManifest,
        context: &AdmissionContext,
        resolve_source: bool,
    ) -> RuntimeResult<Self> {
        // Attach manifest provenance to every admission failure, including lowering
        // and plan-feasibility errors which propagate raw. The bytes were already
        // read and hashed, so a `null` manifestPath/computedManifestHash on an invalid
        // manifest would be an inconsistent, weaker diagnostic than parse/origin/abi/
        // closure errors carry.
        Self::admit_checks(loaded, context, resolve_source).map_err(|error| {
            error.with_manifest_if_missing(
                loaded.path.clone(),
                loaded.computed_manifest_hash.clone(),
            )
        })
    }

    fn admit_checks(
        loaded: &LoadedManifest,
        context: &AdmissionContext,
        resolve_source: bool,
    ) -> RuntimeResult<Self> {
        origin::check_store_origin(loaded, context)?;
        abi::check_abi(&loaded.manifest, loaded)?;
        target::check_target(&loaded.manifest, loaded, context)?;
        let source = if resolve_source {
            Some(source::check_source(&loaded.manifest, loaded, context)?)
        } else {
            None
        };
        let secrets = if resolve_source {
            secrets::resolve_secrets(&loaded.manifest)?
        } else {
            secrets::check_secret_references(&loaded.manifest)?;
            secrets::ResolvedSecrets::empty()
        };
        closures::check_closures(&loaded.manifest, loaded, context)?;
        let execution_manifest = lower(&loaded.manifest)?;
        prove_all_plans_feasible(&execution_manifest)?;
        Ok(from_loaded(
            &loaded.manifest,
            loaded,
            source,
            execution_manifest,
            secrets,
        ))
    }

    /// The resolved source root, or a `SOURCE_MISMATCH` error when this is a
    /// control admission that did not resolve one. Every run-path caller holds a
    /// run admission, so the error is only reachable through a misuse.
    pub fn require_source(&self) -> RuntimeResult<&source::AdmittedSource> {
        self.source.as_ref().ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::SourceMismatch,
                "operation requires an admitted source that control admission does not resolve",
            )
        })
    }
}

fn from_loaded(
    manifest: &Manifest,
    loaded: &LoadedManifest,
    source: Option<source::AdmittedSource>,
    execution_manifest: ExecutionManifest,
    secrets: secrets::ResolvedSecrets,
) -> Admission {
    Admission {
        manifest_path: loaded.path.clone(),
        computed_manifest_hash: loaded.computed_manifest_hash.clone(),
        raw_len: loaded.raw_len,
        project_id: manifest.project.project_id.clone(),
        runtime_abi: manifest.runtime_abi.clone(),
        toolchain_id: manifest.toolchain_id.clone(),
        target_system: manifest.target.system.clone(),
        source,
        generator_json: serde_json::to_string(&manifest.generator).unwrap_or_default(),
        target_json: serde_json::to_string(&manifest.target).unwrap_or_default(),
        execution_manifest,
        secrets,
    }
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
