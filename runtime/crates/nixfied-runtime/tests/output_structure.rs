use std::error::Error;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use nixfied_runtime::control::{DownReport, ProcessObservation, PsReport};
use nixfied_runtime::error::{ErrorCode, RuntimeCause, RuntimeError};
use nixfied_runtime::service::TaskRun;
use nixfied_runtime::state::CleanupOutcome;
use serde_json::{Value, json};

const TASK: &str = r#"{"taskId":"task","stepPath":"root.step","processKey":"process","exitCode":null,"timedOut":false,"canceled":false,"success":true,"durationMs":7,"stdoutPath":"/stdout","stderrPath":"/stderr","summaryPath":"/summary"}"#;

#[test]
fn task_evidence_keeps_unknown_field_tolerance_and_explicit_null() {
    for supplied in [false, true] {
        let mut input: Value = serde_json::from_str(TASK).unwrap();
        input["futureField"] = json!({"unknown":"accepted"});
        if !supplied {
            input.as_object_mut().unwrap().remove("exitCode");
        }
        let task: TaskRun = serde_json::from_value(input).unwrap();
        assert_eq!(task.exit_code, None);
        assert_eq!(serde_json::to_string(&task).unwrap(), TASK);
    }
    let mut input: Value = serde_json::from_str(TASK).unwrap();
    input["exitCode"] = json!(2147483648_i64);
    assert!(serde_json::from_value::<TaskRun>(input).is_err());
}

#[test]
fn native_paths_keep_non_utf8_failure_instead_of_becoming_lossy() {
    let invalid = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
    let mut task: TaskRun = serde_json::from_str(TASK).unwrap();
    task.stdout_path = invalid.clone();
    assert!(serde_json::to_value(task).is_err());
    let cleanup = CleanupOutcome {
        cleanup_id: "cleanup".into(),
        deleted_path: invalid.clone(),
    };
    assert!(serde_json::to_value(cleanup).is_err());
    let mut error = RuntimeError::new(ErrorCode::StateUnwritable, "failed");
    error.manifest_path = Some(invalid);
    assert!(serde_json::to_value(error).is_err());
}

#[test]
fn runtime_error_is_one_owned_native_error_with_open_details() {
    let error = RuntimeError::new(ErrorCode::TaskFailed, "task failed").with_details(Value::Null);
    assert_eq!(error.to_string(), "TaskFailed: task failed");
    assert!(error.source().is_none());
    assert_eq!(
        serde_json::to_string(&error).unwrap(),
        r#"{"code":"TASK_FAILED","exitClass":"error","message":"task failed","details":null,"manifestPath":null,"computedManifestHash":null}"#
    );
    // This annotation proves the existing boxed native storage at compilation.
    let causes: Box<Vec<RuntimeCause>> = error.causes;
    assert!(causes.is_empty());
    for details in [
        Value::Null,
        json!([]),
        json!(true),
        json!(42),
        json!("raw OS text"),
    ] {
        let cause = RuntimeCause::from_error(
            RuntimeError::new(ErrorCode::RegistryCorrupt, "unsafe infrastructure message")
                .with_details(details),
        );
        assert_eq!(cause.details, json!({}));
        assert_eq!(cause.message, "cause: REGISTRY_CORRUPT");
    }
}

#[test]
fn native_detail_serialization_failure_keeps_its_existing_fallback() {
    struct Unserializable;
    impl serde::Serialize for Unserializable {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("fixture conversion failed"))
        }
    }
    let error = RuntimeError::new(ErrorCode::PortConflict, "conflict")
        .with_detail("portConflict", Unserializable);
    assert_eq!(
        error.details,
        json!({"portConflict":"detail-serialization-failed"})
    );
}

#[test]
fn control_and_cleanup_outputs_keep_presence_order_and_native_values() {
    let report = PsReport {
        processes: vec![ProcessObservation {
            process_key: "process".into(),
            run_id: "run".into(),
            service_instance_id: None,
            pid: 12,
            pgid: -3,
            registry_status: "escaped".into(),
            reconciled_status: "stale".into(),
            service_lifetime: None,
            borrower_count: -1,
            live: false,
        }],
    };
    assert_eq!(
        serde_json::to_string(&report).unwrap(),
        r#"{"processes":[{"processKey":"process","runId":"run","serviceInstanceId":null,"pid":12,"pgid":-3,"registryStatus":"escaped","reconciledStatus":"stale","serviceLifetime":null,"borrowerCount":-1,"live":false}]}"#
    );
    assert_eq!(
        serde_json::to_string(&DownReport {
            stopped: vec!["p".into()],
            stale: vec![]
        })
        .unwrap(),
        r#"{"stopped":["p"],"stale":[]}"#
    );
    assert_eq!(
        serde_json::to_string(&CleanupOutcome {
            cleanup_id: "c".into(),
            deleted_path: "/state".into()
        })
        .unwrap(),
        r#"{"cleanupId":"c","deletedPath":"/state"}"#
    );
}
