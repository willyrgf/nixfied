pub mod cleanup;
pub mod marker;
pub mod placement;

pub use cleanup::{CleanupOutcome, clean_marked_state, inspect_cleanup_target};
pub use marker::{MARKER_FILE_NAME, StateIdentity, StateMarker, write_slot_marker};
pub use placement::{
    HostPlacement, derive_host_placement, materialize_run_roots, state_base_from_env,
};
