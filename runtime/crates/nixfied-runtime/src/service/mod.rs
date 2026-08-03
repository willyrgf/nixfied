mod endpoint;
mod identity;
mod process;
mod readiness;
mod registry;
mod task;

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

pub use identity::{compute_service_identity, service_address_hash, service_instance_id};
pub use process::{
    PrepareRunner, SelectedEndpoint, ServiceSelection, ServiceStartError, SlotEndpoints,
    StartedService, run_slot_clean, start_service_for_slot,
};
pub use registry::{mark_run_completed, mark_run_failed, record_run_created};
pub use task::{
    CompletedEvidence, PrepareTaskError, RunContext, TaskExecution, TaskExecutionError, TaskRun,
    run_dependent_task, run_dependent_task_cancellable,
};

pub(crate) use process::{
    process_escape_start_identity, process_group_has_live_member, process_is_live_with_identity,
    process_is_live_with_start_identity, terminate_process_group,
    terminate_process_tree_with_snapshot,
};
pub(crate) use registry::{
    ProcessRecord, TaskTerminalStatus, mark_process_escape, mark_service_stopped,
    mark_task_finished, release_unresolved_escape_ports, service_lifetime_as_str,
};
