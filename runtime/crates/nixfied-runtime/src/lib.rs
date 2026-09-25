#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("nixfied-runtime supports only Linux and macOS");

pub mod admission;
pub mod background;
pub mod cancellation;
mod child;
pub mod control;
pub mod error;
pub mod execution;
mod filesystem;
pub mod launch;
pub mod manifest_loader;
pub mod output;
pub mod presenter;
pub mod redaction;
pub mod registry;
pub mod service;
pub mod session_control;
pub mod slot;
mod spawn;
pub mod state;
mod template;
mod token;

pub use admission::{
    AdmissionContext, ControlAdmission, RunAdmission, StoreOriginPolicy, admit_control, admit_run,
    source::AdmittedSource,
};
pub use error::{ErrorCode, RuntimeError, RuntimeResult};
pub use manifest_loader::{
    LoadedManifest, RawManifest, load_manifest, parse_loaded_manifest, read_raw_manifest,
};
