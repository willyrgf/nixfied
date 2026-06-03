use nixfied_model::{DirtyPolicy, Model, SourceMode};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::model_loader::LoadedModel;

pub fn check_source(model: &Model, loaded: &LoadedModel) -> RuntimeResult<()> {
    let [codebase] = model.codebases.as_slice() else {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            "M0 requires exactly one codebase",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    };
    if codebase.codebase_id != "main" || codebase.source_mode != SourceMode::LiveWorkspace {
        return Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            "M0 requires codebase main with live-workspace sourceMode",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash));
    }
    match codebase.source_policy.dirty_policy {
        DirtyPolicy::Allow | DirtyPolicy::Warn => Ok(()),
        DirtyPolicy::Reject => Err(RuntimeError::new(
            ErrorCode::SourceMismatch,
            "M0 runtime cannot prove live workspace cleanliness for dirtyPolicy=reject",
        )
        .with_model(&loaded.path, &loaded.computed_model_hash)),
    }
}
