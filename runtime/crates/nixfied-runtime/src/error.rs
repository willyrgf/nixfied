use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ModelNotStoreOutput,
    ModelInvalid,
    ModelAdmission,
    RuntimeAbiMismatch,
    SourceMismatch,
    PlatformUnsupported,
    ClosureMissing,
    RegistryCorrupt,
}

#[derive(Debug, Error, Serialize)]
#[error("{code:?}: {message}")]
pub struct RuntimeError {
    pub code: ErrorCode,
    pub message: String,
    pub model_path: Option<PathBuf>,
    pub computed_model_hash: Option<String>,
}

impl RuntimeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            model_path: None,
            computed_model_hash: None,
        }
    }

    pub fn with_model(mut self, path: impl Into<PathBuf>, hash: impl Into<String>) -> Self {
        self.model_path = Some(path.into());
        self.computed_model_hash = Some(hash.into());
        self
    }
}
