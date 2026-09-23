use nixfied_manifest::{MANIFEST_VERSION, Manifest, TOOLCHAIN_ID, runtime_abi};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::manifest_loader::LoadedManifest;

pub fn check_abi(manifest: &Manifest, loaded: &LoadedManifest) -> RuntimeResult<()> {
    let runtime_abi = runtime_abi();
    if manifest.manifest_version != MANIFEST_VERSION
        || manifest.runtime_abi != runtime_abi
        || manifest.toolchain_id != TOOLCHAIN_ID
    {
        return Err(RuntimeError::new(
            ErrorCode::RuntimeAbiMismatch,
            format!(
                "expected manifestVersion={MANIFEST_VERSION}, runtimeAbi={runtime_abi}, toolchainId={TOOLCHAIN_ID}; got manifestVersion={}, runtimeAbi={}, toolchainId={}",
                manifest.manifest_version, manifest.runtime_abi, manifest.toolchain_id
            ),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
    }
    Ok(())
}
