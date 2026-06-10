use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("expected modelVersion {expected}, got {actual}")]
    ModelVersion { expected: u32, actual: u32 },

    #[error("expected toolchainId {expected}, got {actual}")]
    ToolchainId {
        expected: &'static str,
        actual: String,
    },

    #[error("expected runtimeAbi {expected}, got {actual}")]
    RuntimeAbi {
        expected: &'static str,
        actual: String,
    },

    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },

    #[error("{field} must be {expected}, got {actual}")]
    UnsupportedValue {
        field: &'static str,
        expected: &'static str,
        actual: String,
    },

    #[error("{field} must contain exactly one entry")]
    ExpectedOne { field: &'static str },
}

#[derive(Debug, Error)]
pub enum ModelValidationError {
    #[error("model JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Contract(#[from] ValidationError),
}
