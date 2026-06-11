pub mod identity;
pub mod ownership;
pub mod process;
pub mod readiness;
pub mod registry;
pub mod task;

pub use identity::{compute_service_identity, service_address_hash, service_instance_id};
pub use ownership::verify_endpoint_ownership;
pub use process::{
    SelectedEndpoint, ServiceSelection, SlotEndpoints, StartedService, run_slot_clean,
    start_service_for_slot,
};
pub use readiness::wait_for_tcp_probe;
pub use task::{RunContext, run_dependent_task, run_dependent_task_cancellable};
