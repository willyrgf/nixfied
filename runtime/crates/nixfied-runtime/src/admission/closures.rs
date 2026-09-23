use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use nixfied_manifest::Manifest;

use super::origin::StoreRoot;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

pub fn check_closures(manifest: &Manifest, store: &StoreRoot<'_>) -> RuntimeResult<()> {
    let invoked_tools = super::invocations(manifest)
        .flat_map(|invocation| invocation.tools.iter())
        .map(|tool| tool.as_str())
        .collect::<BTreeSet<_>>();
    for (closure_id, closure) in &manifest.closures {
        let store_path = require_store_path("closure.storePath", &closure.store_path, store)?;
        let executable = require_store_path("closure.executable", &closure.executable, store)?;
        if !store_path.exists() {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!("closure storePath does not exist: {}", store_path.display()),
            ));
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
            ));
        }
        if closure.target_system != manifest.target.closure_system {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure {} targetSystem {} does not match closureSystem {}",
                    closure_id, closure.target_system, manifest.target.closure_system
                ),
            ));
        }
        if !executable.exists() {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure executable does not exist: {}",
                    executable.display()
                ),
            ));
        }
        // A closure invoked by any invocation is later run via `Command::new`, so
        // it must carry the executable bit no matter what `requiresExecutable`
        // declares. Enforce it at admission (CLOSURE_MISSING) instead of trusting
        // the Nix default and letting a non-executable invoked closure surface as
        // a ProcEscape after the manifest has already been admitted.
        let invoked = invoked_tools.contains(closure_id.as_str());
        if closure.requires_executable || invoked {
            let metadata = executable.metadata().map_err(|error| {
                RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "failed to inspect executable {}: {error}",
                        executable.display()
                    ),
                )
            })?;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "closure executable is not executable: {}",
                        executable.display()
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn require_store_path(
    field: &'static str,
    value: &str,
    store: &StoreRoot<'_>,
) -> RuntimeResult<std::path::PathBuf> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} is not absolute: {value}"),
        ));
    }
    let Ok(canonical) = path.canonicalize() else {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} does not exist: {value}"),
        ));
    };
    let Some(canonical_store) = &store.canonical else {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("store root does not exist: {}", store.declared.display()),
        ));
    };
    if canonical.starts_with(canonical_store) {
        Ok(canonical)
    } else {
        Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} is not under {}: {value}", store.declared.display()),
        ))
    }
}
