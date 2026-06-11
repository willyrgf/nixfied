pub mod abi;
pub mod closures;
pub mod origin;
pub mod source;
pub mod target;

use std::path::PathBuf;

use nixfied_model::Model;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecutionModel, lower, prove_all_plans_feasible};
use crate::model_loader::LoadedModel;

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
    pub model_path: PathBuf,
    pub computed_model_hash: String,
    pub raw_len: usize,
    pub project_id: String,
    pub runtime_abi: String,
    pub toolchain_id: String,
    pub target_system: String,
    /// The resolved live workspace. Present for run admission; `None` for control
    /// admission (`ps`/`down`/`clean`), which must operate on a slot from the store
    /// model and registry alone and so does not resolve the caller's workspace.
    pub source: Option<source::AdmittedSource>,
    /// Provenance serialized once at admission for the run record, so the executor
    /// records it without reading the raw `Model`.
    pub generator_json: String,
    pub target_json: String,
    /// The lowered, executable view of the model. Admission proves a concrete plan
    /// exists for every slot/selection; the executor consumes only this.
    pub execution_model: ExecutionModel,
}

impl Admission {
    /// Run admission: admit and lower the model, and resolve the live workspace so
    /// the executor can spawn execs from it.
    pub fn check(loaded: &LoadedModel, context: &AdmissionContext) -> RuntimeResult<Self> {
        Self::admit(loaded, context, true)
    }

    /// Control admission for recovery commands (`ps`/`down`/`clean`). Admits and
    /// lowers the model but does NOT resolve the live workspace: control must
    /// reconcile, stop, and clean a slot from the store model and registry alone,
    /// so it cannot fail because the caller is outside the project root or the
    /// workspace has moved or been deleted while services stay registered.
    pub fn check_for_control(
        loaded: &LoadedModel,
        context: &AdmissionContext,
    ) -> RuntimeResult<Self> {
        Self::admit(loaded, context, false)
    }

    fn admit(
        loaded: &LoadedModel,
        context: &AdmissionContext,
        resolve_source: bool,
    ) -> RuntimeResult<Self> {
        // Attach model provenance to every admission failure, including lowering
        // and plan-feasibility errors which propagate raw. The bytes were already
        // read and hashed, so a `null` modelPath/computedModelHash on an invalid
        // model would be an inconsistent, weaker diagnostic than parse/origin/abi/
        // closure errors carry.
        Self::admit_checks(loaded, context, resolve_source).map_err(|error| {
            error.with_model_if_missing(loaded.path.clone(), loaded.computed_model_hash.clone())
        })
    }

    fn admit_checks(
        loaded: &LoadedModel,
        context: &AdmissionContext,
        resolve_source: bool,
    ) -> RuntimeResult<Self> {
        origin::check_store_origin(loaded, context)?;
        abi::check_abi(&loaded.model, loaded)?;
        target::check_target(&loaded.model, loaded, context)?;
        let source = if resolve_source {
            Some(source::check_source(&loaded.model, loaded)?)
        } else {
            None
        };
        closures::check_closures(&loaded.model, loaded, context)?;
        let execution_model = lower(&loaded.model)?;
        prove_all_plans_feasible(&execution_model)?;
        Ok(from_loaded(&loaded.model, loaded, source, execution_model))
    }

    /// The resolved live workspace, or a `SOURCE_MISMATCH` error when this is a
    /// control admission that did not resolve one. Every run-path caller holds a
    /// run admission, so the error is only reachable through a misuse.
    pub fn require_source(&self) -> RuntimeResult<&source::AdmittedSource> {
        self.source.as_ref().ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::SourceMismatch,
                "operation requires an admitted live source that control admission does not resolve",
            )
        })
    }
}

fn from_loaded(
    model: &Model,
    loaded: &LoadedModel,
    source: Option<source::AdmittedSource>,
    execution_model: ExecutionModel,
) -> Admission {
    Admission {
        model_path: loaded.path.clone(),
        computed_model_hash: loaded.computed_model_hash.clone(),
        raw_len: loaded.raw_len,
        project_id: model.project.project_id.clone(),
        runtime_abi: model.runtime_abi.clone(),
        toolchain_id: model.toolchain_id.clone(),
        target_system: model.target.system.clone(),
        source,
        generator_json: serde_json::to_string(&model.generator).unwrap_or_default(),
        target_json: serde_json::to_string(&model.target).unwrap_or_default(),
        execution_model,
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
