pub mod cleanup;
pub mod marker;
pub mod ownership;
pub mod placement;
pub mod preparation;
pub(crate) mod tree;

pub use cleanup::{
    CleanupMode, CleanupOutcome, RetentionOutcome, apply_retention, clean_marked_state,
};
pub use marker::{
    MARKER_FILE_NAME, MARKER_VERSION, MarkerDecision, StateIdentity, StateMarker,
    commit_slot_marker, evaluate_slot_marker,
};
pub use placement::{HostPlacement, SlotIdentity, derive_slot_placement, state_base_from_env};
pub use preparation::{PreparationReport, prepare_slot_state};
