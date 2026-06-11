pub mod cleanup;
pub mod marker;
pub mod placement;
pub mod upgrade;

pub use cleanup::{CleanupOutcome, clean_marked_state, inspect_cleanup_target};
pub use marker::{
    MARKER_FILE_NAME, MarkerComparison, MarkerDecision, StateIdentity, StateMarker,
    commit_slot_marker, evaluate_slot_marker,
};
pub use placement::{
    HostPlacement, derive_host_placement, derive_host_placement_for_slot,
    materialize_registry_root, materialize_run_roots, materialize_state_root, state_base_from_env,
};
pub use upgrade::{UpgradeReport, prepare_slot_state};
