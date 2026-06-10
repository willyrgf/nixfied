pub mod abi;
pub mod closures;
pub mod origin;
pub mod secrets;
pub mod source;
pub mod target;

use std::path::PathBuf;

use nixfied_model::Model;

use crate::error::RuntimeResult;
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
    pub source: source::AdmittedSource,
    /// Provenance serialized once at admission for the run record, so the executor
    /// records it without reading the raw `Model`.
    pub generator_json: String,
    pub target_json: String,
    /// The lowered, executable view of the model. Admission proves a concrete plan
    /// exists for every slot/selection; the executor consumes only this.
    pub execution_model: ExecutionModel,
}

impl Admission {
    pub fn check(loaded: &LoadedModel, context: &AdmissionContext) -> RuntimeResult<Self> {
        origin::check_store_origin(loaded, context)?;
        abi::check_abi(&loaded.model, loaded)?;
        target::check_target(&loaded.model, loaded, context)?;
        let source = source::check_source(&loaded.model, loaded)?;
        closures::check_closures(&loaded.model, loaded, context)?;
        secrets::check_secrets(&loaded.model, loaded)?;
        let execution_model = lower(&loaded.model)?;
        prove_all_plans_feasible(&execution_model)?;
        Ok(from_loaded(&loaded.model, loaded, source, execution_model))
    }
}

fn from_loaded(
    model: &Model,
    loaded: &LoadedModel,
    source: source::AdmittedSource,
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
