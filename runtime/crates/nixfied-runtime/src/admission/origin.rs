use std::path::{Path, PathBuf};

use crate::admission::StoreOriginPolicy;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::manifest_loader::RawManifest;

pub fn check_raw_store_origin(
    raw_manifest: &RawManifest,
    policy: StoreOriginPolicy,
    store: &StoreRoot<'_>,
) -> RuntimeResult<()> {
    if policy == StoreOriginPolicy::AllowNonStoreForTests {
        return Ok(());
    }
    if store.canonical_under(&raw_manifest.path).is_some() {
        Ok(())
    } else {
        Err(RuntimeError::new(
            ErrorCode::ManifestNotStoreOutput,
            format!(
                "manifest path {} is not under {}",
                raw_manifest.path.display(),
                store.declared.display()
            ),
        )
        .with_manifest(&raw_manifest.path, &raw_manifest.computed_manifest_hash))
    }
}

/// One local observation shared by this admission's confinement checks.
/// Failure is reported by the first applicable phase, preserving its diagnostic.
pub(super) struct StoreRoot<'a> {
    pub declared: &'a Path,
    pub canonical: Option<PathBuf>,
}

impl<'a> StoreRoot<'a> {
    pub fn observe(declared: &'a Path) -> Self {
        Self {
            declared,
            canonical: declared.canonicalize().ok(),
        }
    }

    pub fn canonical_under(&self, path: &Path) -> Option<PathBuf> {
        let canonical_path = path.canonicalize().ok()?;
        canonical_path
            .starts_with(self.canonical.as_ref()?)
            .then_some(canonical_path)
    }
}
