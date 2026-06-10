use nixfied_model::{MODEL_VERSION, Model, TOOLCHAIN_ID, runtime_abi};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

pub fn check_abi(model: &Model, loaded: &LoadedModel) -> RuntimeResult<()> {
    let runtime_abi = runtime_abi();
    if model.model_version != MODEL_VERSION
        || model.runtime_abi != runtime_abi
        || model.toolchain_id != TOOLCHAIN_ID
    {
        return Err(RuntimeError::new(
            ErrorCode::RuntimeAbiMismatch,
            format!(
                "expected modelVersion={MODEL_VERSION}, runtimeAbi={runtime_abi}, toolchainId={TOOLCHAIN_ID}; got modelVersion={}, runtimeAbi={}, toolchainId={}",
                model.model_version, model.runtime_abi, model.toolchain_id
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    Ok(())
}
