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

    #[error("{field} must contain exactly {expected} entries, got {actual}")]
    ExpectedLen {
        field: &'static str,
        expected: usize,
        actual: usize,
    },

    #[error("{field} must be empty")]
    MustBeEmpty { field: &'static str },

    #[error("{field} must not contain a host-absolute path: {value}")]
    HostAbsolutePath { field: &'static str, value: String },

    #[error(
        "closure {closure_id} targetSystem {target_system} does not match target.closureSystem {closure_system}"
    )]
    ClosureTargetMismatch {
        closure_id: String,
        target_system: String,
        closure_system: String,
    },

    #[error("{reference_kind} references undeclared id {id}")]
    UndeclaredReference {
        reference_kind: &'static str,
        id: String,
    },

    #[error("operation binding {binding} does not exist in model primitives")]
    UnknownOperationBinding { binding: String },
}

#[derive(Debug, Error)]
pub enum ModelValidationError {
    #[error("model JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Contract(#[from] ValidationError),
}
