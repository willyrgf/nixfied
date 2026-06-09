use nixfied_model::Model;

use crate::error::{RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

pub fn check_secrets(model: &Model, loaded: &LoadedModel) -> RuntimeResult<()> {
    if model.secrets.is_empty() {
        Ok(())
    } else {
        Err(
            RuntimeError::unsupported_feature("secrets", "non-empty secrets are unsupported")
                .with_detail("secretCount", model.secrets.len())
                .with_model(&loaded.path, &loaded.computed_model_hash),
        )
    }
}
