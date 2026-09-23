use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use nixfied_manifest::Manifest;

use crate::admission::AdmissionContext;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::manifest_loader::LoadedManifest;

pub fn check_closures(
    manifest: &Manifest,
    loaded: &LoadedManifest,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    for (closure_id, closure) in &manifest.closures {
        let store_path =
            require_store_path("closure.storePath", &closure.store_path, loaded, context)?;
        let executable =
            require_store_path("closure.executable", &closure.executable, loaded, context)?;
        if !store_path.exists() {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!("closure storePath does not exist: {}", store_path.display()),
            )
            .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
        }
        // Containment is asserted on the DECLARED paths: the executable the
        // manifest names must live under the storePath the manifest names. The
        // canonicalized form is used only for existence/exec-bit checks —
        // buildEnv-style packages (e.g. a Rust toolchain) legitimately
        // symlink `bin/<tool>` into a different store path inside the same
        // closure, and following the link must not break the attestation.
        if !Path::new(&closure.executable).starts_with(&closure.store_path) {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure executable {} is not inside storePath {}",
                    closure.executable, closure.store_path
                ),
            )
            .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
        }
        if closure.target_system != manifest.target.closure_system {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure {} targetSystem {} does not match closureSystem {}",
                    closure_id, closure.target_system, manifest.target.closure_system
                ),
            )
            .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
        }
        if !executable.exists() {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure executable does not exist: {}",
                    executable.display()
                ),
            )
            .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
        }
        // A closure invoked by any invocation is later run via `Command::new`, so
        // it must carry the executable bit no matter what `requiresExecutable`
        // declares. Enforce it at admission (CLOSURE_MISSING) instead of trusting
        // the Nix default and letting a non-executable invoked closure surface as
        // a ProcEscape after the manifest has already been admitted.
        let invoked = invocation_tool_ids(manifest).any(|tool| tool == closure_id.as_str());
        if closure.requires_executable || invoked {
            let metadata = executable.metadata().map_err(|error| {
                RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "failed to inspect executable {}: {error}",
                        executable.display()
                    ),
                )
                .with_manifest(&loaded.path, &loaded.computed_manifest_hash)
            })?;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "closure executable is not executable: {}",
                        executable.display()
                    ),
                )
                .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
            }
        }
    }
    Ok(())
}

/// Every closure id referenced as a tool by any invocation in the manifest.
fn invocation_tool_ids(manifest: &Manifest) -> impl Iterator<Item = &str> {
    let task_tools = manifest
        .tasks
        .values()
        .flat_map(|task| task.invocation.iter())
        .flat_map(|invocation| invocation.tools.iter());
    let lifecycle_tools = manifest.services.values().flat_map(|service| {
        let lifecycle = &service.lifecycle;
        std::iter::once(&lifecycle.start.invocation)
            .chain(lifecycle.ready.probe.invocation.iter())
            .chain(lifecycle.health.probe.invocation.iter())
            .flat_map(|invocation| invocation.tools.iter())
    });
    task_tools.chain(lifecycle_tools).map(|id| id.as_str())
}

fn require_store_path(
    field: &'static str,
    value: &str,
    loaded: &LoadedManifest,
    context: &AdmissionContext,
) -> RuntimeResult<std::path::PathBuf> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} is not absolute: {value}"),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
    }
    let Ok(canonical) = path.canonicalize() else {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} does not exist: {value}"),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
    };
    let Ok(canonical_store) = context.store_root.canonicalize() else {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!(
                "store root does not exist: {}",
                context.store_root.display()
            ),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash));
    };
    if canonical.starts_with(&canonical_store) {
        Ok(canonical)
    } else {
        Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!(
                "{field} is not under {}: {value}",
                context.store_root.display()
            ),
        )
        .with_manifest(&loaded.path, &loaded.computed_manifest_hash))
    }
}
