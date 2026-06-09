use std::path::{Path, PathBuf};

use nixfied_model::{Model, Validate};
use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

#[derive(Debug, Clone)]
pub struct LoadedModel {
    pub path: PathBuf,
    pub raw_len: usize,
    pub computed_model_hash: String,
    pub model: Model,
}

#[derive(Debug, Clone)]
pub struct RawModel {
    pub path: PathBuf,
    pub raw: Vec<u8>,
    pub computed_model_hash: String,
}

impl RawModel {
    pub fn raw_len(&self) -> usize {
        self.raw.len()
    }
}

pub fn read_raw_model(path: impl AsRef<Path>) -> RuntimeResult<RawModel> {
    let path = path.as_ref().to_path_buf();
    let raw = std::fs::read(&path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelInvalid,
            format!("failed to read model bytes: {error}"),
        )
    })?;
    let computed_model_hash = hex::encode(Sha256::digest(&raw));

    Ok(RawModel {
        path,
        raw,
        computed_model_hash,
    })
}

pub fn parse_loaded_model(raw_model: RawModel) -> RuntimeResult<LoadedModel> {
    let model: Model = serde_json::from_slice(&raw_model.raw).map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelInvalid,
            format!("failed to parse model JSON: {error}"),
        )
        .with_model(&raw_model.path, &raw_model.computed_model_hash)
    })?;
    model.validate().map_err(|error| {
        let message = format!("model contract validation failed: {error}");
        let runtime_error = match &error {
            nixfied_model::ValidationError::RuntimeAbi { .. }
            | nixfied_model::ValidationError::ToolchainId { .. }
            | nixfied_model::ValidationError::ModelVersion { .. } => {
                RuntimeError::new(ErrorCode::RuntimeAbiMismatch, message)
            }
            nixfied_model::ValidationError::MustBeEmpty { field } if *field == "secrets" => {
                RuntimeError::unsupported_feature("secrets", message)
                    .with_detail("secretCount", model.secrets.len())
            }
            nixfied_model::ValidationError::MustBeEmpty { field } if *field == "workflows" => {
                RuntimeError::unsupported_feature("workflows", message)
                    .with_detail("workflowCount", model.workflows.len())
            }
            nixfied_model::ValidationError::MustBeEmpty { field }
                if *field == "secrets" || *field == "workflows" =>
            {
                RuntimeError::new(ErrorCode::ModelAdmission, message)
            }
            _ => RuntimeError::new(ErrorCode::ModelInvalid, message),
        };
        runtime_error.with_model(&raw_model.path, &raw_model.computed_model_hash)
    })?;

    let raw_len = raw_model.raw_len();
    Ok(LoadedModel {
        path: raw_model.path,
        raw_len,
        computed_model_hash: raw_model.computed_model_hash,
        model,
    })
}

pub fn load_model(path: impl AsRef<Path>) -> RuntimeResult<LoadedModel> {
    parse_loaded_model(read_raw_model(path)?)
}
