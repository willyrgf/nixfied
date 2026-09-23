use crate::admission::{AdmissionContext, StoreOriginPolicy};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::manifest_loader::RawManifest;

pub fn check_raw_store_origin(
    raw_manifest: &RawManifest,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    if context.policy == StoreOriginPolicy::AllowNonStoreForTests {
        return Ok(());
    }
    if canonical_under_store(&raw_manifest.path, &context.store_root).is_some() {
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

pub fn canonical_under_store(
    path: &std::path::Path,
    store_root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let canonical_path = path.canonicalize().ok()?;
    let canonical_store = store_root.canonicalize().ok()?;
    canonical_path
        .starts_with(canonical_store)
        .then_some(canonical_path)
}
