//! Shared fixtures and helpers for the integration-test binaries. Each `tests/*.rs`
//! pulls this in with `mod common;`; no single binary uses every item, so dead
//! code is expected here rather than a sign of rot.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use nixfied_manifest::Manifest;
use nixfied_manifest::fixtures::{self, SyntheticManifestOptions};
use nixfied_runtime::registry::session::record_run_created;
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::service::{
    ServiceSelection, SlotEndpoints, StartingService, start_service_for_slot,
};
use nixfied_runtime::slot::SelectedSlot;
use nixfied_runtime::state::HostPlacement;
use nixfied_runtime::{RunAdmission, RuntimeResult};

pub fn add_slot_one(value: &mut Value, start: u16, end: u16) {
    value["slotPolicy"]["max"] = json!(1);
    value["placement"]["slotPlacements"]["1"] = json!({
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
        self.command_at(&self.state_base, operation, extra)
    }

    /// The same manifest against another state base, as an independent root.
    pub fn command_at(&self, state_base: &Path, operation: &str, extra: &[&str]) -> Command {
        let mut command = Command::new(runtime_binary());
        command
            .arg(operation)
            .arg("--allow-non-store-manifest")
            .arg("--manifest")
            .arg(&self.manifest_path);
        // `check` reads no state and takes no state base.
        if operation != "check" {
            command.arg("--state-base").arg(state_base);
        }
        command
            .args(extra)
            .current_dir(&self.tmp.path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    pub fn output(&self, operation: &str, extra: &[&str]) -> Output {
        self.command(operation, extra)
            .output()
            .expect("runtime command should execute")
    }

    /// The slot registry this fixture's commands wrote, opened read-only.
    pub fn registry(&self) -> rusqlite::Connection {
        registry_ro(&self.state_base)
    }
}

#[track_caller]
pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
    fixtures::synthetic_manifest(&SyntheticManifestOptions {
        port_start,
        port_end,
        ..SyntheticManifestOptions::default()
    })
}

/// The test child's listener that holds its endpoint until stopped.
pub const LISTEN_HOLD: &[&str] = &["listen", "127.0.0.1", "${port}", "hold"];

/// The fixture over the realised test child, started with `start_args`.
pub fn test_child_service(start_args: &[&str], port_start: u16, port_end: u16) -> Value {
    synthetic_manifest(
        test_child()
            .to_str()
            .expect("test child path should be UTF-8"),
        start_args,
        port_start,
        port_end,
    )
}

/// A realised listener service for lifecycle/state fixtures that will undergo admission.
pub fn test_child_manifest(port_start: u16, port_end: u16) -> Value {
    test_child_service(LISTEN_HOLD, port_start, port_end)
}

/// Replace the smoke task's argv tail (`run[1..]`), keeping the program word.
pub fn set_task_run_args(value: &mut Value, args: &[&str]) {
    let run = value["tasks"]["smoke"]["invocation"]["run"]
        .as_array_mut()
        .expect("run is an array");
    run.truncate(1);
    run.extend(args.iter().map(|arg| json!(arg)));
}

/// Remove a task's authored deadline so only cancellation or failure ends it.
pub fn clear_task_deadline(value: &mut Value, task: &str) {
    value["tasks"][task]["invocation"]
        .as_object_mut()
        .expect("task invocation is an object")
        .remove("timeoutMs");
}

/// Clone the fixture's `smoke` task under `name` with its own arguments.
pub fn add_task_clone(value: &mut Value, name: &str, requires: &[&str], args: &[&str]) {
    let mut task = value["tasks"]["smoke"].clone();
    task["operationId"] = json!(format!("task.{name}.run"));
    task["requires"] = json!(requires);
    task["logRefs"] = json!([format!("task.{name}")]);
    let run = task["invocation"]["run"]
        .as_array_mut()
        .expect("task run is an array");
    run.truncate(1);
    run.extend(args.iter().map(|arg| json!(arg)));
    value["tasks"][name] = task;
}

/// Clone the fixture's `synthetic` service under `name` with its own endpoint
/// and start arguments.
pub fn add_service_clone(value: &mut Value, name: &str, start_args: &[&str], connects_to: &[&str]) {
    let mut service = value["services"]["synthetic"].clone();
    for operation in ["start", "ready", "health", "stop", "clean"] {
        service["lifecycle"][operation]["operationId"] =
            json!(format!("service.{name}.{operation}"));
    }
    let run = service["lifecycle"]["start"]["invocation"]["run"]
        .as_array_mut()
        .expect("start run is an array");
    run.truncate(1);
    run.extend(start_args.iter().map(|arg| json!(arg)));
    let endpoint = format!("{name}-tcp");
    service["endpoints"] =
        json!({ endpoint.clone(): { "endpointId": endpoint, "host": "127.0.0.1" } });
    service["primaryEndpoint"] = json!(endpoint);
    service["logRefs"] = json!([format!("service.{name}")]);
    service["connectsTo"] = json!(connects_to);
    value["services"][name] = service;
}

/// Bind `synthetic` to a prepare task named `endpoint-prepare` that runs `args`.
pub fn add_endpoint_prepare(value: &mut Value, args: &[&str]) {
    value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({ "task": "endpoint-prepare" });
    add_task_clone(value, "endpoint-prepare", &[], args);
}

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

/// Record a fixture run and start its `synthetic` service in the registry's
/// slot through the generic runtime API. The production runtime crate is
/// service-name agnostic, so the concrete name lives here in test support. An
/// empty `endpoint_ports` starts an endpoint-less service.
pub fn start_fixture_service(
    admission: &RunAdmission,
    placement: &HostPlacement,
    registry: &mut Registry,
    run_id: &str,
    endpoint_ports: &std::collections::BTreeMap<String, u16>,
    cancellation: &nixfied_runtime::cancellation::CancellationToken,
    prepare_runner: Option<nixfied_runtime::service::PrepareRunner<'_>>,
) -> RuntimeResult<StartingService> {
    let slot = u32::try_from(registry.identity().slot).expect("registry slot is a u32");
    let selected_slot =
        nixfied_runtime::slot::select_slot(admission.common().manifest(), Some(slot))?;
    record_run_created(registry, run_id, admission, placement)?;
    start_service_for_slot(
        admission,
        placement,
        registry,
        run_id,
        &selected_slot,
        ServiceSelection {
            launcher: &runtime_binary(),
            session_checkpoint: &|| Ok(()),
            service_name: "synthetic",
            endpoint_ports,
            slot_endpoints: &SlotEndpoints::new(),
            run_timeout_ms: 5000,
            cancellation,
            prepare_runner,
        },
    )
}

/// The synthetic fixture's single endpoint, `synthetic-tcp`, on `port`.
pub fn synthetic_endpoint(port: u16) -> std::collections::BTreeMap<String, u16> {
    std::collections::BTreeMap::from([("synthetic-tcp".to_string(), port)])
}

/// A unique temporary directory removed on drop.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let path = temp_marker("nixfied-test");
        fs::create_dir_all(&path).expect("temp dir should be created");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
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
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        unique_suffix()
    ))
}

pub fn unique_suffix() -> u128 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be available")
        .as_nanos();
    now + u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

/// Loopback ports this process handed out, never reused, with the flocks that
/// keep other test processes off them for this process's lifetime.
struct PortReservations {
    ports: BTreeSet<u16>,
    locks: Vec<fs::File>,
}

static PORT_RESERVATIONS: Mutex<PortReservations> = Mutex::new(PortReservations {
    ports: BTreeSet::new(),
    locks: Vec::new(),
});

/// Flock `port` in a harness-owned directory beside the fixed production lock
/// root, so concurrent test processes never share one endpoint; `None` when
/// another process holds it.
fn lock_test_port(port: u16) -> Option<fs::File> {
    let directory =
        Path::new("/tmp").join(format!("nixfied-test-ports-{}", unsafe { libc::geteuid() }));
    fs::create_dir_all(&directory).expect("test port lock directory should be created");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(port.to_string()))
        .expect("test port lock file should open");
    file.try_lock().ok().map(|()| file)
}

/// A window of `width` consecutive free loopback ports reserved for this test:
/// no other caller in this process or in a concurrent test process gets them.
pub fn available_port_window(width: u16) -> u16 {
    assert!(width > 0, "port window width must be positive");
    let mut reservations = PORT_RESERVATIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    for _ in 0..256 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
        let start = listener.local_addr().expect("local addr").port();
        drop(listener);
        let Some(end) = start.checked_add(width - 1) else {
            continue;
        };
        if (start..=end).any(|port| reservations.ports.contains(&port)) {
            continue;
        }
        let Some(locks) = (start..=end)
            .map(lock_test_port)
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let held = (start..=end)
            .map(|port| TcpListener::bind(("127.0.0.1", port)))
            .collect::<Result<Vec<_>, _>>();
        if held.is_ok() {
            reservations.ports.extend(start..=end);
            reservations.locks.extend(locks);
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

/// Probe until it yields a value or `timeout` elapses; the probe always runs
/// at least once and once more at the deadline.
pub fn poll<T>(timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// [`poll`] that fails the test when `what` never happens.
#[track_caller]
pub fn poll_until<T>(timeout: Duration, what: &str, probe: impl FnMut() -> Option<T>) -> T {
    poll(timeout, probe).unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

pub fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    poll(timeout, || path.exists().then_some(())).is_some()
}

pub fn wait_for_named(root: &Path, name: &str, timeout: Duration) -> Option<PathBuf> {
    poll(timeout, || find_named(root, name))
}

/// A decimal pid a child wrote to `path`.
#[track_caller]
pub fn wait_for_pid_file(path: &Path) -> u32 {
    poll_until(Duration::from_secs(2), "a child pid file", || {
        fs::read_to_string(path).ok()?.trim().parse().ok()
    })
}

pub fn wait_for_child_output(mut child: Child, timeout: Duration) -> Output {
    let exited = poll(timeout, || {
        child
            .try_wait()
            .expect("child status should be inspectable")
    });
    if exited.is_none() {
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
    child
        .wait_with_output()
        .expect("child output should be collected")
}

/// Whether any non-zombie process remains in the group. A stopped child stays
/// a zombie until its owner reaps it, so a raw `kill(-pgid, 0)` would count it.
pub fn process_group_has_non_zombie_member(pgid: i32) -> bool {
    let output = Command::new("ps")
        .arg("-axo")
        .arg("pgid=,stat=")
        .output()
        .expect("ps should inspect process groups");
    assert_success(&output);
    String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next().and_then(|field| field.parse::<i32>().ok()) == Some(pgid)
            && !fields.next().unwrap_or("").starts_with('Z')
    })
}

/// Concurrent harness threads fork children that briefly share every open
/// lock description until exec; a fixture's own slot is otherwise uncontended.
pub fn registry_guard(placement: &HostPlacement) -> nixfied_runtime::state::ownership::SlotGuard {
    poll_until(Duration::from_secs(5), "fixture slot authority", || {
        nixfied_runtime::state::ownership::SlotGuard::try_acquire(
            placement,
            &nixfied_runtime::cancellation::CancellationToken::new(),
        )
        .expect("fixture slot authority")
    })
}

/// Open the slot registry recorded for `placement` and `manifest`.
pub fn open_slot_registry(
    placement: &HostPlacement,
    manifest: &Manifest,
    selected_slot: &SelectedSlot<'_>,
) -> Registry {
    Registry::open_or_create(
        registry_guard(placement),
        &RegistryIdentity::for_slot(
            &manifest.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &manifest.runtime_abi,
            &manifest.toolchain_id,
        ),
    )
    .expect("slot registry should open")
}

/// The only registry under `state_base`, opened read-only, once it exists.
pub fn try_registry_ro(state_base: &Path) -> Option<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(
        find_named(state_base, "registry.sqlite3")?,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()
}

/// The only registry under `state_base`, opened read-only.
#[track_caller]
pub fn registry_ro(state_base: &Path) -> rusqlite::Connection {
    try_registry_ro(state_base).expect("a registry should exist and open read-only")
}

pub fn observe_registry(
    registry: &Registry,
) -> nixfied_runtime::RuntimeResult<nixfied_runtime::control::PsReport> {
    let reader = nixfied_runtime::registry::RegistryReader::open_existing(
        registry.path(),
        registry.identity(),
    )?
    .expect("fixture registry exists");
    nixfied_runtime::control::ps(&reader)
}

/// The registry identity of the schema-level registry tests.
pub fn registry_identity() -> RegistryIdentity {
    RegistryIdentity::for_slot(
        "minimal",
        "dev",
        0,
        "nixfied-runtime-abi:1",
        "nixfied-toolchain:1",
    )
}

/// The placement of `run_id` in `selected_slot`.
pub fn slot_placement(
    manifest: &Manifest,
    selected_slot: &SelectedSlot<'_>,
    run_id: &str,
    state_base: &Path,
) -> RuntimeResult<HostPlacement> {
    nixfied_runtime::state::derive_slot_placement(
        &manifest.project.project_id,
        selected_slot.environment,
        selected_slot.slot,
        run_id,
        state_base,
    )
}

/// The placement of `run_id` in the manifest's default slot.
pub fn default_placement(
    manifest: &Manifest,
    run_id: &str,
    state_base: &Path,
) -> RuntimeResult<HostPlacement> {
    slot_placement(
        manifest,
        &nixfied_runtime::slot::select_slot(manifest, None)?,
        run_id,
        state_base,
    )
}

/// The state identity of the manifest's default slot.
pub fn default_state_identity(
    admission: &nixfied_runtime::ControlAdmission,
) -> nixfied_runtime::state::StateIdentity {
    nixfied_runtime::state::StateIdentity::from_selected_slot(
        admission,
        &nixfied_runtime::slot::select_slot(admission.manifest(), None).unwrap(),
    )
}

/// A registry placement for `identity` whose slot authority is free.
pub fn registry_placement(root: &Path, identity: &RegistryIdentity) -> HostPlacement {
    let placement = nixfied_runtime::state::placement::derive_slot_placement(
        &identity.project_id,
        &identity.environment,
        identity.slot.try_into().unwrap(),
        "registry-test",
        root,
    )
    .unwrap();
    registry_guard(&placement).release().unwrap();
    placement
}

pub fn assert_empty_unversioned_database(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let count = |sql: &str| conn.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(count("PRAGMA user_version"), 0);
    assert_eq!(
        count("SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'"),
        0
    );
}

/// Seed a session row in the registry's own slot, below the supported writer
/// boundary.
pub fn seed_run(connection: &rusqlite::Connection, run_id: &str, outcome: Option<&str>) {
    connection
        .execute(
            "INSERT INTO runs (run_id, execution_outcome, manifest_path,
               computed_manifest_hash, runtime_abi, toolchain_id, generator_json, target_json,
               source_json, owner_identity, diagnostic_path)
             SELECT ?1, ?2, '/nix/store/test-manifest/manifest.json', 'hash',
               runtime_abi, toolchain_id, '{}', '{}', '[]', '{}', 'diagnostics.log'
             FROM registry_meta",
            rusqlite::params![run_id, outcome],
        )
        .expect("fixture run should be seeded");
}

/// A process row seeded below the supported writer boundary.
pub struct SeedProcess<'a> {
    pub key: &'a str,
    pub run_id: &'a str,
    /// Also the process group.
    pub pid: i64,
    pub start_identity: &'a str,
    pub status: &'a str,
    pub ownership: &'a str,
    /// A service's declared name; a task has none.
    pub service: Option<&'a str>,
    pub presentation: &'a str,
}

impl Default for SeedProcess<'_> {
    fn default() -> Self {
        Self {
            key: "process-1",
            run_id: "run-1",
            pid: 999_999,
            start_identity: r#"{"platformStart":"missing"}"#,
            status: "running",
            ownership: "unresolved",
            service: None,
            presentation: "hidden",
        }
    }
}

pub fn seed_process(connection: &rusqlite::Connection, process: SeedProcess<'_>) {
    connection
        .execute(
            "INSERT INTO processes (process_key, pid, pgid, start_identity, command_json,
               run_id, service_name, role, status, ownership, source_label, presentation,
               stdout_path, stderr_path, stop_signal, stop_timeout_ms, containment)
             VALUES (?1, ?2, ?2, ?3, '{}', ?4, ?5,
               CASE WHEN ?5 IS NULL THEN 'task' ELSE 'service' END, ?6, ?7, 'fixture', ?8,
               'logs/' || ?1 || '.out', 'logs/' || ?1 || '.err', 15, 1000, 'process-group')",
            rusqlite::params![
                process.key,
                process.pid,
                process.start_identity,
                process.run_id,
                process.service,
                process.status,
                process.ownership,
                process.presentation,
            ],
        )
        .expect("fixture process should be seeded");
}

/// Seed immutable endpoint evidence owned by the process `owner`.
pub fn seed_port(connection: &rusqlite::Connection, owner: &str, endpoint_id: &str, port: u16) {
    connection
        .execute(
            "INSERT INTO ports (owner_process_key, endpoint_id, address, port)
             VALUES (?1, ?2, '127.0.0.1', ?3)",
            rusqlite::params![owner, endpoint_id, port],
        )
        .expect("fixture port should be seeded");
}
