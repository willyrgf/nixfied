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
    OutputModeInvalid,
    OutputModeConflict,
    TaskSelectionInvalid,
    OutputProjectionFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExitClass {
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCause {
    pub code: ErrorCode,
    pub exit_class: ExitClass,
    pub message: String,
    pub details: Value,
}

impl RuntimeCause {
    pub fn from_error(error: RuntimeError) -> Self {
        Self {
            code: error.code,
            exit_class: error.exit_class,
            message: format!(
                "cause: {}",
                serde_json::to_value(error.code)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{:?}", error.code))
            ),
            details: cause_details(error.code, error.details),
        }
    }
}

/// Causes are a typed summary boundary, not a second copy of an arbitrary
/// runtime error. In particular, formatted OS messages are intentionally left
/// out; callers still receive the structured diagnostic fields that are safe and
/// useful for the public projection (including task evidence and projection
/// outcomes).
fn cause_details(code: ErrorCode, details: Value) -> Value {
    const SAFE_KEYS: &[&str] = &[
        "compositeSteps",
        "declaredTasks",
        "endpoint",
        "expectedRegistryIdentity",
        "failedNodeId",
        "failedService",
        "foundRegistryIdentity",
        "logsDir",
        "mismatchedFields",
        "nixfiedOwner",
        "portConflict",
        "projections",
        "registryDir",
        "registryPath",
        "runDir",
        "runId",
        "runSummaryPath",
        "slot",
        "stateRoot",
        "summaryPath",
        "stderrPath",
        "stdoutPath",
        "task",
        "taskRun",
        "unknownTask",
    ];
    let Value::Object(details) = details else {
        return Value::Object(Map::new());
    };
    let mut safe = Map::new();
    for key in SAFE_KEYS {
        if let Some(value) = details.get(*key) {
            safe.insert((*key).to_string(), value.clone());
        }
    }
    // A projection error has one additional structured field; all other
    // details are intentionally omitted from a cause unless they are explicitly
    // listed above. Keep the match exhaustive at the code boundary so adding a
    // new error class prompts a conscious public-cause decision.
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
        | ErrorCode::SecretLeakBlocked
        | ErrorCode::OutputModeInvalid
        | ErrorCode::OutputModeConflict
        | ErrorCode::TaskSelectionInvalid
        | ErrorCode::OutputProjectionFailed => {}
    }
    Value::Object(safe)
}

#[derive(Debug, Error, Serialize)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct RuntimeError {
    pub code: ErrorCode,
    pub exit_class: ExitClass,
    pub message: String,
    pub details: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<RuntimeCause>,
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
            causes: Vec::new(),
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

    pub fn with_cause(mut self, cause: RuntimeError) -> Self {
        self.causes.push(RuntimeCause::from_error(cause));
        self
    }

    pub fn with_causes<I>(mut self, causes: I) -> Self
    where
        I: IntoIterator<Item = RuntimeError>,
    {
        self.causes
            .extend(causes.into_iter().map(RuntimeCause::from_error));
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
    // here until it is added, forcing the public error-contract snapshot and the
    // capability descriptor below to be updated deliberately. The contract is
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
            | ErrorCode::SecretLeakBlocked
            | ErrorCode::OutputModeInvalid
            | ErrorCode::OutputModeConflict
            | ErrorCode::TaskSelectionInvalid
            | ErrorCode::OutputProjectionFailed => {}
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
        ErrorCode::OutputModeInvalid,
        ErrorCode::OutputModeConflict,
        ErrorCode::TaskSelectionInvalid,
        ErrorCode::OutputProjectionFailed,
    ];

    const ALL_EXIT_CLASSES: &[ExitClass] = &[ExitClass::Ok, ExitClass::Error];

    fn wire(value: impl Serialize) -> String {
        serde_json::to_value(value)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .expect("contract enum serializes to a string")
    }

    /// The public error contract is part of the capability descriptor that drives
    /// `runtimeAbi` (docs/CONTRACT.md). Every error code and exit class must be
    /// listed there, so the chain holds: a new variant fails
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

    #[test]
    fn causes_are_omitted_when_empty_and_are_non_recursive_when_present() {
        let empty = serde_json::to_value(RuntimeError::new(ErrorCode::TaskFailed, "task failed"))
            .expect("runtime error should serialize");
        assert!(empty.get("causes").is_none());

        let cause = RuntimeError::new(ErrorCode::TaskFailed, "task smoke failed")
            .with_detail("taskRun", serde_json::json!({ "success": false }));
        let compound = RuntimeError::new(
            ErrorCode::OutputProjectionFailed,
            "selected task output replay failed",
        )
        .with_detail(
            "projections",
            serde_json::json!([{
                "stream": "stdout",
                "operation": "write",
                "kind": "broken-pipe",
                "path": "<stdout>",
                "bytesWritten": 4
            }]),
        )
        .with_cause(cause);
        let wire = serde_json::to_value(compound).expect("compound error should serialize");
        assert_eq!(wire["code"], serde_json::json!("OUTPUT_PROJECTION_FAILED"));
        assert_eq!(wire["causes"][0]["code"], serde_json::json!("TASK_FAILED"));
        assert_eq!(wire["causes"][0]["exitClass"], serde_json::json!("error"));
        assert_eq!(
            wire["causes"][0]["details"]["taskRun"]["success"],
            serde_json::json!(false)
        );
        assert!(wire["causes"][0].get("causes").is_none());
    }

    #[test]
    fn causes_project_typed_details_without_raw_os_messages() {
        let compound = RuntimeError::new(
            ErrorCode::OutputProjectionFailed,
            "selected task output replay failed",
        )
        .with_cause(
            RuntimeError::new(
                ErrorCode::TaskFailed,
                "failed to signal process group: secret-value and /private/path",
            )
            .with_detail("error", "raw operating-system text")
            .with_detail("taskRun", serde_json::json!({"success": false})),
        );
        let wire = serde_json::to_value(compound).expect("compound error should serialize");
        let cause = &wire["causes"][0];
        assert_eq!(cause["code"], serde_json::json!("TASK_FAILED"));
        assert_eq!(cause["message"], serde_json::json!("cause: TASK_FAILED"));
        assert_eq!(
            cause["details"]["taskRun"]["success"],
            serde_json::json!(false)
        );
        assert!(cause["details"].get("error").is_none());
    }
}
