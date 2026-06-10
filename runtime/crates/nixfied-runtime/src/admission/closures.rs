use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use nixfied_model::Model;

use crate::admission::AdmissionContext;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

pub fn check_closures(
    model: &Model,
    loaded: &LoadedModel,
    context: &AdmissionContext,
) -> RuntimeResult<()> {
    for closure in &model.closures {
        let store_path =
            require_store_path("closure.storePath", &closure.store_path, loaded, context)?;
        let executable =
            require_store_path("closure.executable", &closure.executable, loaded, context)?;
        if !store_path.exists() {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!("closure storePath does not exist: {}", store_path.display()),
            )
            .with_model(&loaded.path, &loaded.computed_model_hash));
        }
        if !executable.starts_with(&store_path) {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure executable {} is not inside storePath {}",
                    executable.display(),
                    store_path.display()
                ),
            )
            .with_model(&loaded.path, &loaded.computed_model_hash));
        }
        if closure.target_system != model.target.closure_system {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure {} targetSystem {} does not match closureSystem {}",
                    closure.closure_id, closure.target_system, model.target.closure_system
                ),
            )
            .with_model(&loaded.path, &loaded.computed_model_hash));
        }
        if !executable.exists() {
            return Err(RuntimeError::new(
                ErrorCode::ClosureMissing,
                format!(
                    "closure executable does not exist: {}",
                    executable.display()
                ),
            )
            .with_model(&loaded.path, &loaded.computed_model_hash));
        }
        if closure.requires_executable {
            let metadata = executable.metadata().map_err(|error| {
                RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "failed to inspect executable {}: {error}",
                        executable.display()
                    ),
                )
                .with_model(&loaded.path, &loaded.computed_model_hash)
            })?;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "closure executable is not executable: {}",
                        executable.display()
                    ),
                )
                .with_model(&loaded.path, &loaded.computed_model_hash));
            }
        }
        for (exec_id, exec) in model
            .execs
            .iter()
            .filter(|(_, exec)| exec.closure_id == closure.closure_id)
        {
            if exec.executable != closure.executable {
                return Err(RuntimeError::new(
                    ErrorCode::ClosureMissing,
                    format!(
                        "exec {} executable {} does not match closure {} executable {}",
                        exec_id, exec.executable, closure.closure_id, closure.executable
                    ),
                )
                .with_model(&loaded.path, &loaded.computed_model_hash));
            }
        }
    }
    Ok(())
}

fn require_store_path(
    field: &'static str,
    value: &str,
    loaded: &LoadedModel,
    context: &AdmissionContext,
) -> RuntimeResult<std::path::PathBuf> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} is not absolute: {value}"),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    let Ok(canonical) = path.canonicalize() else {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!("{field} does not exist: {value}"),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    };
    let Ok(canonical_store) = context.store_root.canonicalize() else {
        return Err(RuntimeError::new(
            ErrorCode::ClosureMissing,
            format!(
                "store root does not exist: {}",
                context.store_root.display()
            ),
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
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
        .with_model(&loaded.path, &loaded.computed_model_hash))
    }
}
