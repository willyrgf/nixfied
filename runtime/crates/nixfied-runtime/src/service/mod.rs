pub mod identity;
pub mod ownership;
pub mod process;
pub mod readiness;
pub mod registry;
pub mod task;

pub use identity::{service_address_hash, service_instance_id};
pub use ownership::verify_endpoint_ownership;
pub use process::{
    SelectedEndpoint, StartedService, run_synthetic_service_clean_for_slot,
    start_synthetic_service, start_synthetic_service_for_slot,
};
pub use readiness::wait_for_readiness_probe;
pub use task::{run_dependent_task, run_dependent_task_cancellable};
