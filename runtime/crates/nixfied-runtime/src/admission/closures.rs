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
    for (closure_id, closure) in &model.closures {
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
                    closure_id, closure.target_system, model.target.closure_system
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
        // A closure invoked by any invocation is later run via `Command::new`, so
        // it must carry the executable bit no matter what `requiresExecutable`
        // declares. Enforce it at admission (CLOSURE_MISSING) instead of trusting
        // the Nix default and letting a non-executable invoked closure surface as
        // a ProcEscape after the model has already been admitted.
        let invoked = invocation_tool_ids(model).any(|tool| tool == closure_id.as_str());
        if closure.requires_executable || invoked {
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
    }
    Ok(())
}

/// Every closure id referenced as a tool by any invocation in the model.
fn invocation_tool_ids(model: &Model) -> impl Iterator<Item = &str> {
    let task_tools = model
        .tasks
        .values()
        .flat_map(|task| task.invocation.iter())
        .flat_map(|invocation| invocation.tools.iter());
    let lifecycle_tools = model.services.values().flat_map(|service| {
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
