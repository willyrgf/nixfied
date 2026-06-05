use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Map, Value};
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
    StateUnwritable,
    StateUnowned,
    CleanupRefused,
    PortConflict,
    PortUnverifiable,
    ProcEscape,
    ReadinessTimeout,
    Canceled,
    LeaseStale,
    LeaseConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExitClass {
    Ok,
    Error,
}

#[derive(Debug, Error, Serialize)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct RuntimeError {
    pub code: ErrorCode,
    pub exit_class: ExitClass,
    pub message: String,
    pub details: Value,
    pub model_path: Option<PathBuf>,
    pub computed_model_hash: Option<String>,
}

impl RuntimeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            exit_class: ExitClass::Error,
            message: message.into(),
            details: Value::Object(Map::new()),
            model_path: None,
            computed_model_hash: None,
        }
    }

    pub fn unsupported_feature(feature: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ModelAdmission, message)
            .with_detail("unsupportedFeature", feature.into())
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        let value = serde_json::to_value(value)
            .unwrap_or_else(|_| Value::String("detail-serialization-failed".to_string()));
        match &mut self.details {
            Value::Object(details) => {
                details.insert(key.into(), value);
            }
            _ => {
                let mut details = Map::new();
                details.insert(key.into(), value);
                self.details = Value::Object(details);
            }
        }
        self
    }

    pub fn with_model(mut self, path: impl Into<PathBuf>, hash: impl Into<String>) -> Self {
        self.model_path = Some(path.into());
        self.computed_model_hash = Some(hash.into());
        self
    }

    pub fn with_model_if_missing(
        mut self,
        path: impl Into<PathBuf>,
        hash: impl Into<String>,
    ) -> Self {
        if self.model_path.is_none() {
            self.model_path = Some(path.into());
        }
        if self.computed_model_hash.is_none() {
            self.computed_model_hash = Some(hash.into());
        }
        self
    }
}
