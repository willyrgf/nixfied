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
    // Execution-class failures: the model admitted cleanly, execution failed.
    // These must never be reported for a pre-execution (admission) problem, so
    // that `ModelAdmission` appearing after admission is, by construction, a leak.
    TaskFailed,
    LifecycleFailed,
    DependencyUnavailable,
    SecretUnavailable,
    SecretLeakBlocked,
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

#[cfg(test)]
mod tests {
    use super::*;

    // Exhaustive sentinels: a new `ErrorCode`/`ExitClass` variant fails to compile
    // here until it is added, forcing the public error-contract snapshot below (and
    // the matching list in AGENTS.md) to be updated deliberately. The contract is
    // public API for the current ABI — it must not drift silently.
    fn _error_code_is_listed(code: ErrorCode) {
        match code {
            ErrorCode::ModelNotStoreOutput
            | ErrorCode::ModelInvalid
            | ErrorCode::ModelAdmission
            | ErrorCode::RuntimeAbiMismatch
            | ErrorCode::SourceMismatch
            | ErrorCode::PlatformUnsupported
            | ErrorCode::ClosureMissing
            | ErrorCode::RegistryCorrupt
            | ErrorCode::StateUnwritable
            | ErrorCode::StateUnowned
            | ErrorCode::CleanupRefused
            | ErrorCode::PortConflict
            | ErrorCode::PortUnverifiable
            | ErrorCode::ProcEscape
            | ErrorCode::ReadinessTimeout
            | ErrorCode::Canceled
            | ErrorCode::LeaseStale
            | ErrorCode::LeaseConflict
            | ErrorCode::TaskFailed
            | ErrorCode::LifecycleFailed
            | ErrorCode::DependencyUnavailable
            | ErrorCode::SecretUnavailable
            | ErrorCode::SecretLeakBlocked => {}
        }
    }

    fn _exit_class_is_listed(class: ExitClass) {
        match class {
            ExitClass::Ok | ExitClass::Error => {}
        }
    }

    /// Every public error code, in contract order. The sentinel above guarantees a
    /// new variant cannot be added without being seen; this list and the snapshot
    /// pin the exact wire values.
    const ALL_ERROR_CODES: &[ErrorCode] = &[
        ErrorCode::ModelNotStoreOutput,
        ErrorCode::ModelInvalid,
        ErrorCode::ModelAdmission,
        ErrorCode::RuntimeAbiMismatch,
        ErrorCode::SourceMismatch,
        ErrorCode::PlatformUnsupported,
        ErrorCode::ClosureMissing,
        ErrorCode::RegistryCorrupt,
        ErrorCode::StateUnwritable,
        ErrorCode::StateUnowned,
        ErrorCode::CleanupRefused,
        ErrorCode::PortConflict,
        ErrorCode::PortUnverifiable,
        ErrorCode::ProcEscape,
        ErrorCode::ReadinessTimeout,
        ErrorCode::Canceled,
        ErrorCode::LeaseStale,
        ErrorCode::LeaseConflict,
        ErrorCode::TaskFailed,
        ErrorCode::LifecycleFailed,
        ErrorCode::DependencyUnavailable,
        ErrorCode::SecretUnavailable,
        ErrorCode::SecretLeakBlocked,
    ];

    const ALL_EXIT_CLASSES: &[ExitClass] = &[ExitClass::Ok, ExitClass::Error];

    fn wire(value: impl Serialize) -> String {
        serde_json::to_value(value)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .expect("contract enum serializes to a string")
    }

    /// The public error contract is part of the capability descriptor that drives
    /// `runtimeAbi` (AGENTS.md: "public API for the current ABI"). Every error code
    /// and exit class must be listed there, so the chain holds: a new variant fails
    /// to compile in the sentinels above, listing it in `capability.txt` is then the
    /// only way past this test, and that edit rotates the ABI digest (caught by the
    /// `runtime_abi_snapshot` test) — the error vocabulary cannot change silently or
    /// without the runtime/model identity check noticing.
    #[test]
    fn public_error_contract_is_in_capability_descriptor() {
        use std::collections::BTreeSet;

        // Reference the sentinels so their exhaustive matches compile (a new
        // variant fails the build here) rather than being dead code.
        _error_code_is_listed(ErrorCode::Canceled);
        _exit_class_is_listed(ExitClass::Ok);

        let tokens: BTreeSet<&str> = nixfied_model::constants::CAPABILITY_DESCRIPTOR
            .split_whitespace()
            .collect();

        for code in ALL_ERROR_CODES {
            let value = wire(*code);
            assert!(
                tokens.contains(value.as_str()),
                "error code {value} is missing from the capability descriptor"
            );
        }
        for class in ALL_EXIT_CLASSES {
            let value = wire(*class);
            assert!(
                tokens.contains(value.as_str()),
                "exit class {value} is missing from the capability descriptor"
            );
        }
    }
}
