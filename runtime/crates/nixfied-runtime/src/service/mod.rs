pub mod identity;
pub mod process;
pub mod readiness;
pub mod registry;

pub use identity::{service_address_hash, service_instance_id};
pub use process::{SelectedEndpoint, StartedService, start_synthetic_service};
pub use readiness::wait_for_readiness_probe;
