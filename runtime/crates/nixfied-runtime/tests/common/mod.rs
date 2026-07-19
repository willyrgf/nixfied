//! Shared fixtures and helpers for the integration-test binaries. Each `tests/*.rs`
//! pulls this in with `mod common;`; no single binary uses every item, so dead
//! code is expected here rather than a sign of rot.
#![allow(dead_code)]
#![allow(unused_imports)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::params;
use serde_json::Value;

use nixfied_model::fixtures::{self, SyntheticModelOptions};
pub use nixfied_model::fixtures::{SYNTHETIC_EXECUTABLE, SYNTHETIC_START_ARGS};

pub fn runtime_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_nixfied-runtime") {
        return PathBuf::from(path);
    }
    let current = std::env::current_exe().expect("current test executable should be known");
    current
        .parent()
        .and_then(Path::parent)
        .expect("test binary should be inside target profile directory")
        .join("nixfied-runtime")
}

pub fn nix_store_executable(names: &[&str]) -> Option<PathBuf> {
    if let Some(executable) = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .find_map(|dir| executable_from_dir(&dir, names))
    {
        return Some(executable);
    }
    fs::read_dir("/nix/store").ok()?.find_map(|entry| {
        let package_root = entry.ok()?.path();
        executable_from_dir(&package_root.join("bin"), names)
    })
}

fn executable_from_dir(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    for name in names {
        let candidate = dir.join(name);
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        if metadata.permissions().mode() & 0o111 == 0 {
            continue;
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !canonical.starts_with("/nix/store") {
            continue;
        }
        if candidate.starts_with("/nix/store") {
            if spawnable(&candidate) {
                return Some(candidate);
            }
            continue;
        }
        let store_named = canonical.with_file_name(name);
        if store_named.exists() && spawnable(&store_named) {
            return Some(store_named);
        }
        if canonical.file_name() == Some(std::ffi::OsStr::new(name)) && spawnable(&canonical) {
            return Some(canonical);
        }
    }
    None
}

fn spawnable(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn closure_root_for_store_executable(executable: &Path) -> Option<PathBuf> {
    let rest = executable.to_str()?.strip_prefix("/nix/store/")?;
    let package = rest.split('/').next()?;
    Some(Path::new("/nix/store").join(package))
}

/// The canonical admission fixture — a `synthetic` foreground service plus a
/// `smoke` task in slot 0 over the given candidate port window. Delegates to
/// `nixfied_model::fixtures`, the single source of truth for the model shape.
pub fn synthetic_model(
    executable: &str,
    start_args: &[&str],
    port_start: u16,
    port_end: u16,
) -> Value {
    fixtures::synthetic_model(&SyntheticModelOptions {
        executable: executable.to_string(),
        start_args: start_args.iter().map(|s| s.to_string()).collect(),
        port_start,
        port_end,
        ..SyntheticModelOptions::default()
    })
}

/// [`synthetic_model`] with the default executable and start arguments.
pub fn synthetic_model_default(port_start: u16, port_end: u16) -> Value {
    synthetic_model(
        SYNTHETIC_EXECUTABLE,
        SYNTHETIC_START_ARGS,
        port_start,
        port_end,
    )
}

pub use nixfied_model::fixtures::{host_arch, host_os, host_system};

use nixfied_model::Model;
use nixfied_model::ServiceLifetime;
use nixfied_runtime::registry::Registry;
use nixfied_runtime::service::{
    ServiceSelection, SlotEndpoints, StartedService, run_slot_clean, start_service_for_slot,
};
use nixfied_runtime::slot::{SelectedSlot, select_slot};
use nixfied_runtime::state::{CleanupMode, CleanupOutcome, HostPlacement};
use nixfied_runtime::{Admission, RuntimeResult};

/// The fixture service name. The production runtime crate is service-name
/// agnostic — it starts whatever `ServiceSelection` names — so the concrete
/// `synthetic` name lives here in test support, not in the runtime.
pub const SYNTHETIC_SERVICE_NAME: &str = "synthetic";

pub struct RegistryServiceRow<'a> {
    pub service_instance_id: &'a str,
    pub environment: &'a str,
    pub slot: i64,
    pub service_name: &'a str,
    pub service_address_hash: &'a str,
    pub endpoint_identity_hash: &'a str,
    pub state_identity_hash: &'a str,
    pub runtime_compatibility_hash: &'a str,
    pub target_identity_hash: &'a str,
    pub service_lifetime: ServiceLifetime,
    pub status: &'a str,
    pub endpoint_json: &'a str,
    pub state_root: &'a str,
}

impl<'a> RegistryServiceRow<'a> {
    pub fn synthetic(service_instance_id: &'a str, status: &'a str, state_root: &'a str) -> Self {
        Self {
            service_instance_id,
            environment: "dev",
            slot: 0,
            service_name: SYNTHETIC_SERVICE_NAME,
            service_address_hash: "address",
            endpoint_identity_hash: "endpoint",
            state_identity_hash: "state",
            runtime_compatibility_hash: "runtime",
            target_identity_hash: "target",
            service_lifetime: ServiceLifetime::RunScoped,
            status,
            endpoint_json: "{}",
            state_root,
        }
    }
}

pub fn insert_registry_service(registry: &mut Registry, row: &RegistryServiceRow<'_>) {
    registry
        .connection_mut()
        .execute(
            "
            INSERT INTO services (
              service_instance_id, environment, slot, service_name,
              service_address_hash, endpoint_identity_hash, state_identity_hash,
              runtime_compatibility_hash, target_identity_hash, service_lifetime, status,
              endpoint_json, state_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ",
            params![
                row.service_instance_id,
                row.environment,
                row.slot,
                row.service_name,
                row.service_address_hash,
                row.endpoint_identity_hash,
                row.state_identity_hash,
                row.runtime_compatibility_hash,
                row.target_identity_hash,
                service_lifetime_wire(row.service_lifetime),
                row.status,
                row.endpoint_json,
                row.state_root,
            ],
        )
        .expect("service row should insert");
}

fn service_lifetime_wire(lifetime: ServiceLifetime) -> &'static str {
    match lifetime {
        ServiceLifetime::RunScoped => "run-scoped",
        ServiceLifetime::UntilIdle => "until-idle",
        ServiceLifetime::PersistentUntilDown => "persistent-until-down",
    }
}

/// Start the fixture's `synthetic` service on the default slot through the
/// generic runtime API.
pub fn start_synthetic_service(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_port: u16,
) -> RuntimeResult<StartedService> {
    let selected_slot = select_slot(model, None)?;
    start_synthetic_service_for_slot(
        admission,
        placement,
        registry,
        run_id,
        &selected_slot,
        selected_port,
    )
}

pub fn start_synthetic_service_with_lifetime(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_port: u16,
    service_lifetime: ServiceLifetime,
) -> RuntimeResult<StartedService> {
    let selected_slot = select_slot(model, None)?;
    start_synthetic_service_for_slot_with_lifetime(
        admission,
        placement,
        registry,
        run_id,
        &selected_slot,
        selected_port,
        service_lifetime,
    )
}

/// Start the fixture's `synthetic` service on a chosen slot through the generic
/// runtime API, with no wired endpoints.
pub fn start_synthetic_service_for_slot(
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_slot: &SelectedSlot<'_>,
    selected_port: u16,
) -> RuntimeResult<StartedService> {
    start_synthetic_service_for_slot_with_lifetime(
        admission,
        placement,
        registry,
        run_id,
        selected_slot,
        selected_port,
        ServiceLifetime::RunScoped,
    )
}

pub fn start_synthetic_service_for_slot_with_lifetime(
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_slot: &SelectedSlot<'_>,
    selected_port: u16,
    service_lifetime: ServiceLifetime,
) -> RuntimeResult<StartedService> {
    // The synthetic fixture binds a single endpoint, `synthetic-tcp`.
    let endpoint_ports =
        std::collections::BTreeMap::from([("synthetic-tcp".to_string(), selected_port)]);
    start_service_for_slot(
        admission,
        placement,
        registry,
        run_id,
        selected_slot,
        ServiceSelection {
            service_name: SYNTHETIC_SERVICE_NAME,
            service_lifetime,
            endpoint_ports: &endpoint_ports,
            slot_endpoints: &SlotEndpoints::new(),
            run_timeout_ms: 5000,
            cancellation: &nixfied_runtime::cancellation::CancellationToken::new(),
            prepare_runner: None,
        },
    )
}

/// Clean the synthetic fixture's slot. The fixture's `dev` environment is exactly
/// `[synthetic]`, so the generic slot clean equals cleaning the single service
/// plus the slot state.
pub fn run_synthetic_service_clean_for_slot(
    model: &Model,
    admission: &Admission,
    placement: &HostPlacement,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
) -> RuntimeResult<CleanupOutcome> {
    run_slot_clean(
        model,
        admission,
        placement,
        registry,
        selected_slot,
        CleanupMode::Standard,
    )
}

/// A unique temporary directory removed on drop.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "nixfied-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The trailing JSON document a runtime command writes to stderr, after any
/// human-readable progress lines (which never contain `{`).
pub fn stderr_json(bytes: &[u8]) -> Value {
    let text = String::from_utf8_lossy(bytes);
    let start = text
        .find('{')
        .expect("stderr should contain a JSON document");
    serde_json::from_str(&text[start..]).expect("stderr JSON should parse")
}

/// A unique temporary path (not created) with the given prefix.
pub fn temp_marker(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    path
}

pub fn unique_suffix() -> u128 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be available")
        .as_nanos();
    now + u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}
