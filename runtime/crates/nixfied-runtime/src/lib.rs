#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("nixfied-runtime supports only Linux and macOS");

pub mod admission;
pub mod cancellation;
pub mod control;
pub mod error;
pub mod execution;
pub mod manifest_loader;
pub mod output;
pub mod redaction;
pub mod registry;
pub mod service;
pub mod slot;
pub mod state;

pub use admission::{Admission, AdmissionContext, StoreOriginPolicy, source::AdmittedSource};
pub use error::{ErrorCode, RuntimeError, RuntimeResult};
pub use manifest_loader::{
    LoadedManifest, RawManifest, load_manifest, parse_loaded_manifest, read_raw_manifest,
};
