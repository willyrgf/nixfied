//! Shared fixtures and helpers for the integration-test binaries. Each `tests/*.rs`
//! pulls this in with `mod common;`; no single binary uses every item, so dead
//! code is expected here rather than a sign of rot.
#![allow(dead_code)]
#![allow(unused_imports)]

use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::params;
use serde_json::Value;

use nixfied_manifest::fixtures::{self, SyntheticManifestOptions};
pub use nixfied_manifest::fixtures::{SYNTHETIC_EXECUTABLE, SYNTHETIC_START_ARGS};

pub fn add_slot_one(value: &mut Value, start: u16, end: u16) {
    value["slotPolicy"]["max"] = serde_json::json!(1);
    value["placement"]["slotPlacements"]["1"] = serde_json::json!({
        "slot": 1,
        "candidatePorts": {
            "start": start,
            "end": end
        }
    });
}

/// Raw CLI scenario: deliberately does not validate or repair adversarial bytes.
pub struct RuntimeFixture {
    pub tmp: TempDir,
    pub manifest_path: PathBuf,
    pub state_base: PathBuf,
}

impl RuntimeFixture {
    pub fn new(manifest: impl serde::Serialize) -> Self {
        let tmp = TempDir::new();
        let manifest_path = tmp.path.join("manifest.json");
        let state_base = tmp.path.join("state");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("fixture manifest should serialize"),
        )
        .expect("fixture manifest should be written");
        Self {
            tmp,
            manifest_path,
            state_base,
        }
    }

    pub fn command(&self, operation: &str, extra: &[&str]) -> Command {
        let mut command = Command::new(runtime_binary());
        command
            .arg(operation)
            .arg("--allow-non-store-manifest")
            .arg("--manifest")
            .arg(&self.manifest_path)
            .arg("--state-base")
            .arg(&self.state_base)
            .args(extra)
            .current_dir(&self.tmp.path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

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

pub fn test_child() -> PathBuf {
    fixture_executable("NIXFIED_TEST_CHILD")
}

pub fn test_sleep() -> String {
    fixture_executable("NIXFIED_TEST_SLEEP")
        .to_str()
        .unwrap()
        .to_owned()
}

pub fn test_shell() -> String {
    fixture_executable("NIXFIED_TEST_SHELL")
        .to_str()
        .unwrap()
        .to_owned()
}

fn fixture_executable(variable: &str) -> PathBuf {
    let configured = std::env::var_os(variable)
        .unwrap_or_else(|| panic!("{variable} must name the Nix-built test fixture"));
    let configured = PathBuf::from(configured);
    let canonical = configured.canonicalize().unwrap_or_else(|error| {
        panic!(
            "{variable} {} should canonicalize: {error}",
            configured.display()
        )
    });
    let metadata = fs::metadata(&canonical).expect("fixture executable should be inspectable");
    assert!(
        metadata.is_file(),
        "fixture executable must be a regular file"
    );
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "fixture must be executable"
    );
    closure_root_for_store_executable(&canonical)
        .expect("fixture executable must resolve under a Nix store closure root");
    // Preserve the declared basename: multicall programs dispatch through argv[0].
    configured
}

pub fn closure_root_for_store_executable(executable: &Path) -> Option<PathBuf> {
    let rest = executable.to_str()?.strip_prefix("/nix/store/")?;
    let package = rest.split('/').next()?;
    Some(Path::new("/nix/store").join(package))
}

/// The canonical admission fixture — a `synthetic` foreground service plus a
/// `smoke` task in slot 0 over the given candidate port window. Delegates to
/// `nixfied_manifest::fixtures`, the single source of truth for the manifest shape.
pub fn synthetic_manifest(
    executable: &str,
    start_args: &[&str],
    port_start: u16,
    port_end: u16,
) -> Value {
    fixtures::synthetic_manifest(&SyntheticManifestOptions {
        executable: executable.to_string(),
        start_args: start_args.iter().map(|s| s.to_string()).collect(),
        port_start,
        port_end,
        ..SyntheticManifestOptions::default()
    })
}

/// [`synthetic_manifest`] with the default executable and start arguments.
pub fn synthetic_manifest_default(port_start: u16, port_end: u16) -> Value {
    synthetic_manifest(
        SYNTHETIC_EXECUTABLE,
        SYNTHETIC_START_ARGS,
        port_start,
        port_end,
    )
}

/// A realised service executable for lifecycle/state fixtures that will undergo admission.
pub fn test_child_manifest(port_start: u16, port_end: u16) -> Value {
    synthetic_manifest(
        test_child().to_str().unwrap(),
        &["listen", "127.0.0.1", "${port}", "hold"],
        port_start,
        port_end,
    )
}

pub use nixfied_manifest::fixtures::{host_arch, host_os, host_system};

use nixfied_manifest::{Manifest, ServiceLifetime};
use nixfied_runtime::registry::Registry;
use nixfied_runtime::service::{
    AcquiredService, ServiceSelection, SlotEndpoints, record_run_created, run_slot_clean,
    start_service_for_slot,
};
use nixfied_runtime::slot::{SelectedSlot, select_slot};
use nixfied_runtime::state::{CleanupMode, CleanupOutcome, HostPlacement};
use nixfied_runtime::{RunAdmission, RuntimeResult};

/// Admit raw fixture bytes with explicit workspace and store boundaries.
pub fn admit_fixture_bytes(raw: &[u8], source_root: &Path, store_root: &Path) -> RunAdmission {
    use nixfied_runtime::admission::{AdmissionContext, InvocationRoot, StoreOriginPolicy};
    let path = source_root.join("manifest.json");
    fs::write(&path, raw).expect("fixture manifest should write");
    let mut context = AdmissionContext::current(StoreOriginPolicy::AllowNonStoreForTests);
    context.invocation_root = InvocationRoot::Path(source_root.to_owned());
    context.store_root = store_root.to_owned();
    nixfied_runtime::admit_run(&path, &context).expect("fixture manifest should admit")
}

pub fn fixture_admission(manifest: &Manifest, source_root: &Path) -> RunAdmission {
    admit_fixture_bytes(
        &serde_json::to_vec(manifest).expect("fixture should serialize"),
        source_root,
        Path::new("/nix/store"),
    )
}

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
    pub state_root: &'a str,
}

impl<'a> RegistryServiceRow<'a> {
    pub fn synthetic(service_instance_id: &'a str, state_root: &'a str) -> Self {
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
              runtime_compatibility_hash, target_identity_hash, service_lifetime,
              state_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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

/// Record a fixture run and start its `synthetic` service on the selected slot through the
/// generic runtime API.
pub fn start_fixture_service(
    admission: &RunAdmission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: impl Into<String>,
    selected_slot: &SelectedSlot<'_>,
    selected_port: u16,
    service_lifetime: ServiceLifetime,
) -> RuntimeResult<AcquiredService> {
    let run_id = run_id.into();
    record_run_created(registry, &run_id, admission, placement)?;
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
    _manifest: &Manifest,
    admission: &RunAdmission,
    placement: &HostPlacement,
    registry: &mut Registry,
    selected_slot: &SelectedSlot<'_>,
) -> RuntimeResult<CleanupOutcome> {
    run_slot_clean(
        admission.common(),
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

pub fn available_port_window(width: u16) -> u16 {
    assert!(width > 0, "port window width must be positive");
    for _ in 0..256 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
        let start = listener.local_addr().expect("local addr").port();
        drop(listener);
        let Some(end) = start.checked_add(width - 1) else {
            continue;
        };
        let held = (start..=end)
            .map(|port| TcpListener::bind(("127.0.0.1", port)))
            .collect::<Result<Vec<_>, _>>();
        if held.is_ok() {
            return start;
        }
    }
    panic!("could not find an available {width}-port window");
}

pub fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_named(&path, name)
        {
            return Some(found);
        }
    }
    None
}

pub fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    path.exists()
}

pub fn wait_for_named(root: &Path, name: &str, timeout: Duration) -> Option<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(path) = find_named(root, name) {
            return Some(path);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn wait_for_child_output(mut child: Child, timeout: Duration) -> Output {
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .expect("child status should be inspectable")
            .is_some()
        {
            return child
                .wait_with_output()
                .expect("child output should be collected");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .expect("timed-out child output should be collected");
            panic!(
                "child did not exit before timeout\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}
