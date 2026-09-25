use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Map, Value};

pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCause {
    pub code: ErrorCode,
    pub exit_class: ExitClass,
    pub message: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeError {
    pub code: ErrorCode,
    pub exit_class: ExitClass,
    pub message: String,
    pub details: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub causes: Box<Vec<RuntimeCause>>,
    pub manifest_path: Option<PathBuf>,
    pub computed_manifest_hash: Option<String>,
}

include!("generated/error.rs");

impl RuntimeCause {
    pub fn from_error(error: RuntimeError) -> Self {
        Self {
            code: error.code,
            exit_class: error.exit_class,
            message: cause_message(&error),
            details: cause_details(error.details),
        }
    }
}

fn cause_message(error: &RuntimeError) -> String {
    match error.code {
        // These execution messages are constructed from validated manifest ids
        // and terminal facts, so retaining them makes compound failures
        // actionable without admitting arbitrary infrastructure text.
        ErrorCode::TaskFailed => task_failure_cause_message(&error.details)
            .unwrap_or_else(|| format!("cause: {}", error_code_wire(error.code))),
        ErrorCode::Canceled | ErrorCode::DependencyUnavailable => error.message.clone(),
        ErrorCode::SecretLeakBlocked
            if matches!(
                error.message.as_str(),
                "captured stdout did not reach EOF before shutdown deadline"
                    | "captured stderr did not reach EOF before shutdown deadline"
            ) =>
        {
            error.message.clone()
        }
        _ => format!("cause: {}", error_code_wire(error.code)),
    }
}

fn task_failure_cause_message(details: &Value) -> Option<String> {
    let task_run = details.get("taskRun")?;
    let task_id = task_run.get("taskId")?.as_str()?;
    if task_run
        .get("timedOut")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some(format!("task {task_id} timed out"));
    }
    let exit_code = task_run
        .get("exitCode")
        .and_then(Value::as_i64)
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Some(format!("task {task_id} exited with code {exit_code}"))
}

pub fn error_code_wire(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

/// Causes are a typed summary boundary, not a second copy of an arbitrary
/// runtime error. In particular, formatted OS messages are intentionally left
/// out; callers still receive the structured diagnostic fields that are safe and
/// useful for the public projection (including task evidence and projection
/// outcomes).
fn cause_details(details: Value) -> Value {
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
    Value::Object(safe)
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeError {}

impl RuntimeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            exit_class: ExitClass::Error,
            message: message.into(),
            details: Value::Object(Map::new()),
            causes: Box::new(Vec::new()),
            manifest_path: None,
            computed_manifest_hash: None,
        }
    }

    pub fn unsupported_feature(feature: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ManifestAdmission, message)
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

    pub fn with_manifest(mut self, path: impl Into<PathBuf>, hash: impl Into<String>) -> Self {
        self.manifest_path = Some(path.into());
        self.computed_manifest_hash = Some(hash.into());
        self
    }

    pub fn with_manifest_if_missing(
        mut self,
        path: impl Into<PathBuf>,
        hash: impl Into<String>,
    ) -> Self {
        if self.manifest_path.is_none() {
            self.manifest_path = Some(path.into());
        }
        if self.computed_manifest_hash.is_none() {
            self.computed_manifest_hash = Some(hash.into());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn causes_are_omitted_when_empty_and_are_non_recursive_when_present() {
        let empty = serde_json::to_value(RuntimeError::new(ErrorCode::TaskFailed, "task failed"))
            .expect("runtime error should serialize");
        assert!(empty.get("causes").is_none());

        let cause = RuntimeError::new(ErrorCode::TaskFailed, "task smoke failed")
            .with_detail("taskRun", serde_json::json!({ "success": false }));
        let compound = RuntimeError::new(
            ErrorCode::OutputProjectionFailed,
            "live output delivery failed",
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
            "live output delivery failed",
        )
        .with_cause(
            RuntimeError::new(
                ErrorCode::TaskFailed,
                "failed to signal process group: secret-value and /private/path",
            )
            .with_detail("error", "raw operating-system text")
            .with_detail(
                "taskRun",
                serde_json::json!({
                    "taskId": "smoke",
                    "exitCode": 7,
                    "timedOut": false,
                    "success": false
                }),
            ),
        );
        let wire = serde_json::to_value(compound).expect("compound error should serialize");
        let cause = &wire["causes"][0];
        assert_eq!(cause["code"], serde_json::json!("TASK_FAILED"));
        assert_eq!(
            cause["message"],
            serde_json::json!("task smoke exited with code 7")
        );
        assert_eq!(
            cause["details"]["taskRun"]["success"],
            serde_json::json!(false)
        );
        assert!(cause["details"].get("error").is_none());
    }
}
