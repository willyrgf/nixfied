mod endpoint;
mod identity;
mod process;
mod readiness;
mod registry;
mod socket;
mod task;

const OBSERVATION_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackedProcessIdentity {
    pub(crate) pid: u32,
    #[serde(default)]
    pub(crate) platform_start: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredProcessIdentity {
    #[serde(default)]
    pub(crate) platform_start: Option<String>,
    #[serde(default)]
    pub(crate) tracked_processes: Vec<TrackedProcessIdentity>,
}

impl StoredProcessIdentity {
    pub(crate) fn encode(
        pid: u32,
        pgid: i32,
        platform_start: Option<&str>,
        tracked_processes: Option<&[TrackedProcessIdentity]>,
    ) -> String {
        let observed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut identity = serde_json::json!({
            "pid": pid,
            "pgid": pgid,
            "platformStart": platform_start,
            "observedAtNanos": observed_at,
        });
        if let Some(tracked) = tracked_processes {
            identity["trackedProcesses"] = serde_json::json!(tracked);
        }
        identity.to_string()
    }
}

pub use identity::{compute_service_identity, service_address_hash, service_instance_id};
pub use process::{
    AcquiredService, BorrowedService, PrepareRunner, ReadinessFailure, ReadyService,
    SelectedEndpoint, ServiceInfo, ServiceSelection, SlotEndpoints, StartedService,
    StartingService, run_slot_clean, start_service_for_slot,
};
pub use registry::{mark_run_completed, mark_run_failed, record_run_created};
pub use task::{
    CompletedEvidence, RunContext, TaskExecution, TaskExecutionError, TaskRun,
    run_dependent_task_cancellable,
};

pub(crate) use process::{
    process_escape_start_identity, process_group_has_live_member, process_is_live_with_identity,
    process_is_live_with_start_identity, terminate_process_group,
    terminate_process_tree_with_snapshot,
};
pub(crate) use registry::{
    ProcessRecord, TaskTerminalStatus, mark_process_escape, mark_service_stopped,
    mark_task_finished, parse_service_lifetime, release_unresolved_escape_ports,
    service_lifetime_as_str,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_identity_wire_preserves_task_omission_and_service_array() {
        let tracked = [TrackedProcessIdentity {
            pid: 11,
            platform_start: Some("child".into()),
        }];
        for (tracking, suffix) in [
            (None, ""),
            (Some(&[][..]), ",\"trackedProcesses\":[]"),
            (
                Some(tracked.as_slice()),
                ",\"trackedProcesses\":[{\"pid\":11,\"platformStart\":\"child\"}]",
            ),
        ] {
            let encoded = StoredProcessIdentity::encode(7, 9, Some("native"), tracking);
            let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
            let timestamp = value["observedAtNanos"].as_u64().unwrap();
            assert!(timestamp > 0);
            assert_eq!(
                encoded,
                format!(
                    "{{\"observedAtNanos\":{timestamp},\"pgid\":9,\"pid\":7,\"platformStart\":\"native\"{suffix}}}"
                )
            );
        }
    }
}
