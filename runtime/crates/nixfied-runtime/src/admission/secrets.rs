use nixfied_model::Model;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

pub fn check_secrets(model: &Model, loaded: &LoadedModel) -> RuntimeResult<()> {
    if model.secrets.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            "non-empty secrets are unsupported in M0",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash))
    }
}
