use nixfied_manifest::Manifest;

use crate::admission::AdmissionContext;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::manifest_loader::LoadedManifest;

pub fn check_target(
    manifest: &Manifest,
    loaded: &LoadedManifest,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    if manifest.target.system != context.host_system
        || manifest.target.closure_system != context.host_system
        || manifest.target.os != host_os()
        || manifest.target.arch != host_arch()
    {
        return Err(RuntimeError::new(
            ErrorCode::PlatformUnsupported,
            format!(
                "manifest target system={} os={} arch={} closureSystem={} does not match host system={} os={} arch={}",
                manifest.target.system,
                manifest.target.os,
                manifest.target.arch,
                manifest.target.closure_system,
                context.host_system,
                host_os(),
                host_arch()
            ),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
    }
    Ok(())
}

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn host_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
}
