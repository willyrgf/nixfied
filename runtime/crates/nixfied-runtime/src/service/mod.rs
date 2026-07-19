mod endpoint;
pub mod identity;
pub mod process;
mod readiness;
pub mod registry;
pub mod task;

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
    SelectedEndpoint, ServiceSelection, SlotEndpoints, StartedService, run_slot_clean,
    start_service_for_slot,
};
pub use task::{RunContext, run_dependent_task, run_dependent_task_cancellable};
