use crate::admission::{AdmissionContext, StoreOriginPolicy};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::manifest_loader::{LoadedManifest, RawManifest};

pub fn check_raw_store_origin(
    raw_manifest: &RawManifest,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    if context.policy == StoreOriginPolicy::AllowNonStoreForTests {
        return Ok(());
    }
    if canonical_is_under_store(&raw_manifest.path, &context.store_root) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ManifestNotStoreOutput,
            format!(
                "manifest path {} is not under {}",
                raw_manifest.path.display(),
                context.store_root.display()
            ),
        )
        .with_manifest(&raw_manifest.path, &raw_manifest.computed_manifest_hash))
    }
}

pub fn check_store_origin(
    loaded: &LoadedManifest,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    if context.policy == StoreOriginPolicy::AllowNonStoreForTests {
        return Ok(());
    }
    if canonical_is_under_store(&loaded.path, &context.store_root) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ManifestNotStoreOutput,
            format!(
                "manifest path {} is not under {}",
                loaded.path.display(),
                context.store_root.display()
            ),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash))
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
