use crate::admission::{AdmissionContext, StoreOriginPolicy};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::{LoadedModel, RawModel};

pub fn check_raw_store_origin(
    raw_model: &RawModel,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    if context.policy == StoreOriginPolicy::AllowNonStoreForTests {
        return Ok(());
    }
    if canonical_is_under_store(&raw_model.path, &context.store_root) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ModelNotStoreOutput,
            format!(
                "model path {} is not under {}",
                raw_model.path.display(),
                context.store_root.display()
            ),
        )
        .with_model(&raw_model.path, &raw_model.computed_model_hash))
    }
}

pub fn check_store_origin(loaded: &LoadedModel, context: &AdmissionContext) -> RuntimeResult<()> {
    if context.policy == StoreOriginPolicy::AllowNonStoreForTests {
        return Ok(());
    }
    if canonical_is_under_store(&loaded.path, &context.store_root) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ModelNotStoreOutput,
            format!(
                "model path {} is not under {}",
                loaded.path.display(),
                context.store_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash))
    }
}

pub fn canonical_is_under_store(path: &std::path::Path, store_root: &std::path::Path) -> bool {
    let Ok(canonical_path) = path.canonicalize() else {
        return false;
    };
    let Ok(canonical_store) = store_root.canonicalize() else {
        return false;
    };
    canonical_path.starts_with(canonical_store)
}
