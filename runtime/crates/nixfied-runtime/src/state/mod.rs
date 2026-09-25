pub mod cleanup;
pub mod marker;
pub mod ownership;
pub mod placement;
pub mod preparation;

pub use cleanup::{
    CleanupMode, CleanupOutcome, RetentionOutcome, apply_retention, clean_marked_state,
    resume_pending_cleanup,
};
pub use marker::{
    MARKER_FILE_NAME, MARKER_VERSION, MarkerComparison, MarkerDecision, StateIdentity, StateMarker,
    commit_slot_marker, evaluate_slot_marker, refresh_slot_marker,
};
pub use placement::{
    HostPlacement, claim_run_evidence, derive_host_placement, derive_host_placement_for_slot,
    materialize_registry_root, materialize_run_roots, materialize_state_root, state_base_from_env,
};
pub use preparation::{PreparationReport, prepare_slot_state};
