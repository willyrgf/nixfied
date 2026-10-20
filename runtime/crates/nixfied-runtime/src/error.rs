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
        "lastRound",
        "logsDir",
        "mismatchedFields",
        "portConflict",
        "projections",
        "registryDir",
        "registryPath",
        "runDir",
        "runId",
        "runSummaryPath",
        "sessionObservation",
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

/// Both steps ran: the first failure stays primary and a second failure
/// becomes its cause.
pub fn both<T>(first: RuntimeResult<T>, second: RuntimeResult<()>) -> RuntimeResult<T> {
    match (first, second) {
        (first, Ok(())) => first,
        (Ok(_), Err(second)) => Err(second),
        (Err(first), Err(second)) => Err(first.with_cause(second)),
    }
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

    /// Attach a subordinate failure together with its own causes, which
    /// precede it.
    pub fn absorb(mut self, mut subordinate: RuntimeError) -> Self {
        self.causes.extend(subordinate.causes.drain(..));
        self.with_cause(subordinate)
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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SelectorDetail {
    Current,
    RunId {
        #[serde(rename = "runId")]
        run: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum SelectionDetail {
    Unenrolled,
    Absent {
        selector: SelectorDetail,
    },
    Seeking {
        watermark: i64,
    },
    Pinned {
        #[serde(rename = "runId")]
        run: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ObservationSelection(SelectionDetail);

impl ObservationSelection {
    pub fn unenrolled() -> Self {
        Self(SelectionDetail::Unenrolled)
    }
    pub fn absent(selector: &crate::control::observation::Selector) -> Self {
        Self(SelectionDetail::Absent {
            selector: match selector.exact_run() {
                None => SelectorDetail::Current,
                Some(run) => SelectorDetail::RunId {
                    run: run.to_owned(),
                },
            },
        })
    }
    pub fn seeking(seeking: &crate::registry::observation::Seeking) -> Self {
        Self(SelectionDetail::Seeking {
            watermark: seeking.watermark(),
        })
    }
    pub fn pinned(run: &crate::registry::observation::PinnedRun) -> Self {
        Self(SelectionDetail::Pinned {
            run: run.run_id().to_owned(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum EstablishmentDetail {
    NotRequested,
    NotEstablished {
        #[serde(rename = "candidateRunId")]
        run: String,
    },
    Uncertain {
        #[serde(rename = "candidateRunId")]
        run: String,
    },
    Established {
        #[serde(rename = "runId")]
        run: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ObservationEstablishment(EstablishmentDetail);

impl ObservationEstablishment {
    pub fn not_requested() -> Self {
        Self(EstablishmentDetail::NotRequested)
    }
    fn checked_run(run: &str) -> RuntimeResult<String> {
        if !crate::registry::session::valid_run_id(run) {
            return Err(RuntimeError::new(
                ErrorCode::LifecycleFailed,
                "invalid bootstrap candidate identity",
            ));
        }
        Ok(run.to_owned())
    }
    pub fn not_established(run: &str) -> RuntimeResult<Self> {
        Ok(Self(EstablishmentDetail::NotEstablished {
            run: Self::checked_run(run)?,
        }))
    }
    pub fn uncertain(run: &str) -> RuntimeResult<Self> {
        Ok(Self(EstablishmentDetail::Uncertain {
            run: Self::checked_run(run)?,
        }))
    }
    pub fn established(run: &str) -> RuntimeResult<Self> {
        Ok(Self(EstablishmentDetail::Established {
            run: Self::checked_run(run)?,
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationEvidence {
    stdout_path: String,
    stderr_path: String,
}

impl ObservationEvidence {
    pub(crate) fn new(
        placement: &crate::state::HostPlacement,
        stdout: &crate::output::EvidenceRelativePath,
        stderr: &crate::output::EvidenceRelativePath,
    ) -> RuntimeResult<Self> {
        Ok(Self {
            stdout_path: crate::output::utf8_locator(&stdout.in_run(placement), "stdoutPath")?
                .to_owned(),
            stderr_path: crate::output::utf8_locator(&stderr.in_run(placement), "stderrPath")?
                .to_owned(),
        })
    }
}

#[derive(Serialize)]
struct ObservationCheckpoint<'a> {
    stage: ObservationStage,
    selection: &'a ObservationSelection,
    establishment: &'a ObservationEstablishment,
    #[serde(rename = "stepPath", skip_serializing_if = "Option::is_none")]
    step: Option<&'a crate::control::observation::RootStepPath>,
}

#[derive(Clone, Copy)]
/// Borrowed wait inputs and diagnostic provenance, with no selection or lifecycle authority.
pub struct ObservationWait<'a> {
    pub budget: &'a crate::cancellation::WaitBudget,
    pub cancellation: &'a crate::cancellation::CancellationToken,
    pub establishment: &'a ObservationEstablishment,
    pub step: Option<&'a crate::control::observation::RootStepPath>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SessionObservationDetail<'a> {
    SlotBusy {
        context: &'a crate::control::observation::Context,
    },
    RequestMismatch {
        field: RequestMismatchField,
        selection: &'a ObservationSelection,
    },
    Timeout(ObservationCheckpoint<'a>),
    Canceled(ObservationCheckpoint<'a>),
    BootstrapUncertain {
        selection: &'a ObservationSelection,
        establishment: &'a ObservationEstablishment,
    },
    Unavailable {
        reason: SessionUnavailableReason,
        selection: &'a ObservationSelection,
        #[serde(rename = "stepPath", skip_serializing_if = "Option::is_none")]
        step: Option<&'a crate::control::observation::RootStepPath>,
        #[serde(skip_serializing_if = "Option::is_none")]
        evidence: Option<&'a ObservationEvidence>,
    },
}

pub fn slot_busy(context: &crate::control::observation::Context) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::SlotBusy,
        "slot is already owned by another session",
    )
    .with_detail(
        "sessionObservation",
        SessionObservationDetail::SlotBusy { context },
    )
}

pub fn request_mismatch(
    field: RequestMismatchField,
    selection: &ObservationSelection,
) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::RequestMismatch,
        "selected session has a different declared request",
    )
    .with_detail(
        "sessionObservation",
        SessionObservationDetail::RequestMismatch { field, selection },
    )
}

impl ObservationWait<'_> {
    pub fn checkpoint(
        &self,
        stage: ObservationStage,
        selection: &ObservationSelection,
    ) -> RuntimeResult<()> {
        let checkpoint = ObservationCheckpoint {
            stage,
            selection,
            establishment: self.establishment,
            step: self.step,
        };
        let (code, message, detail) = if self.cancellation.is_canceled() {
            (
                ErrorCode::Canceled,
                "session observation was canceled",
                SessionObservationDetail::Canceled(checkpoint),
            )
        } else if self.budget.is_expired() {
            (
                ErrorCode::ObservationTimeout,
                "session observation deadline expired",
                SessionObservationDetail::Timeout(checkpoint),
            )
        } else {
            return Ok(());
        };
        Err(RuntimeError::new(code, message).with_detail("sessionObservation", detail))
    }
}

pub fn bootstrap_uncertain(
    selection: &ObservationSelection,
    establishment: &ObservationEstablishment,
) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::LifecycleFailed,
        "shared establishment reply is unavailable or malformed",
    )
    .with_detail(
        "sessionObservation",
        SessionObservationDetail::BootstrapUncertain {
            selection,
            establishment,
        },
    )
}

pub fn session_unavailable(
    reason: SessionUnavailableReason,
    selection: &ObservationSelection,
    step: Option<&crate::control::observation::RootStepPath>,
    evidence: Option<&ObservationEvidence>,
) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::SessionUnavailable,
        "selected session cannot satisfy observation",
    )
    .with_detail(
        "sessionObservation",
        SessionObservationDetail::Unavailable {
            reason,
            selection,
            step,
            evidence,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_errors_keep_exact_tagged_envelopes_and_safe_nested_causes() {
        let selection = ObservationSelection::unenrolled();
        let establishment = ObservationEstablishment::uncertain("run-1").unwrap();
        let step = crate::control::observation::RootStepPath::parse("owner.init").unwrap();
        let canceled = crate::cancellation::CancellationToken::new();
        canceled.cancel();
        let budget = crate::cancellation::WaitBudget::new(std::num::NonZeroU64::new(1).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(2));
        let error = ObservationWait {
            budget: &budget,
            cancellation: &canceled,
            establishment: &establishment,
            step: Some(&step),
        }
        .checkpoint(ObservationStage::Task, &selection)
        .unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::Canceled,
            "signal beats the already expired budget"
        );
        let canceled_detail = serde_json::json!({"kind":"canceled","stage":"task", "selection":{"state":"unenrolled"},
            "establishment":{"state":"uncertain","candidateRunId":"run-1"},"stepPath":"owner.init"});
        assert_eq!(
            error.details,
            serde_json::json!({"sessionObservation":canceled_detail})
        );
        let cause = RuntimeCause::from_error(error);
        assert_eq!(
            cause.details,
            serde_json::json!({"sessionObservation":canceled_detail})
        );
        let timeout = ObservationWait {
            budget: &budget,
            cancellation: &crate::cancellation::CancellationToken::new(),
            establishment: &establishment,
            step: None,
        }
        .checkpoint(ObservationStage::Establishment, &selection)
        .unwrap_err();
        assert_eq!(timeout.code, ErrorCode::ObservationTimeout);
        assert_eq!(
            timeout.details,
            serde_json::json!({"sessionObservation":{"kind":"timeout", "stage":"establishment",
            "selection":{"state":"unenrolled"},"establishment":{"state":"uncertain","candidateRunId":"run-1"}}})
        );
        let pinned = ObservationSelection(SelectionDetail::Pinned {
            run: "run-1".into(),
        });
        let established = ObservationEstablishment::established("run-1").unwrap();
        for (signal, kind, code) in [
            (true, "canceled", ErrorCode::Canceled),
            (false, "timeout", ErrorCode::ObservationTimeout),
        ] {
            let cancellation = crate::cancellation::CancellationToken::new();
            if signal {
                cancellation.cancel();
            }
            let error = ObservationWait {
                budget: &budget,
                cancellation: &cancellation,
                establishment: &established,
                step: Some(&step),
            }
            .checkpoint(ObservationStage::Task, &pinned)
            .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(
                error.details,
                serde_json::json!({"sessionObservation":{
                    "kind":kind,"stage":"task","selection":{"state":"pinned","runId":"run-1"},
                    "establishment":{"state":"established","runId":"run-1"},"stepPath":"owner.init"
                }})
            );
        }
        assert_eq!(
            bootstrap_uncertain(&selection, &establishment).details,
            serde_json::json!({"sessionObservation":{
            "kind":"bootstrap-uncertain","selection":{"state":"unenrolled"},"establishment":{"state":"uncertain","candidateRunId":"run-1"}}})
        );
        assert_eq!(
            request_mismatch(RequestMismatchField::Source, &selection).details,
            serde_json::json!({"sessionObservation":{
            "kind":"request-mismatch","field":"source","selection":{"state":"unenrolled"}}})
        );
        assert_eq!(
            session_unavailable(
                SessionUnavailableReason::Absent,
                &selection,
                Some(&step),
                None
            )
            .details,
            serde_json::json!({"sessionObservation":{"kind":"unavailable","reason":"absent",
            "selection":{"state":"unenrolled"},"stepPath":"owner.init"}})
        );
        let location =
            crate::state::derive_slot_location("project", "dev", 0, std::path::Path::new("/state"))
                .unwrap();
        let context = crate::control::observation::Context::from_location(&location).unwrap();
        assert_eq!(
            slot_busy(&context).details,
            serde_json::json!({"sessionObservation":{"kind":"slot-busy","context":{
            "projectId":"project","environment":"dev","slot":0,"stateBase":"/state","stateRoot":"/state/data/project/dev/0",
            "registryDir":"/state/registry/project/dev/0","registryPath":"/state/registry/project/dev/0/registry.sqlite3"}}})
        );
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
