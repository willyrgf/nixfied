use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use nixfied_model::{CleanupPolicy, DirtyPolicy, Model, PersistencePolicy, SourceMode};
use nixfied_runtime::control::clean_reconciled_state;
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::slot::{first_candidate_port, select_slot};
use nixfied_runtime::state::{
    MARKER_FILE_NAME, StateIdentity, StateMarker, clean_marked_state, derive_host_placement,
    derive_host_placement_for_slot, inspect_cleanup_target, materialize_run_roots,
    write_slot_marker,
};
use nixfied_runtime::{Admission, AdmittedSource, ErrorCode};
use serde_json::{Value, json};

mod common;
use common::*;

#[test]
fn materializes_m0_roots_and_slot_marker() {
    let fixture = StateFixture::new();

    assert_eq!(
        fixture.layout.state_root,
        fixture.tmp.path.join("runtime-test/dev/0")
    );
    assert_eq!(
        fixture.layout.registry_dir,
        fixture.tmp.path.join("registry").join("runtime-test/dev/0")
    );
    assert_eq!(
        fixture.layout.registry_path(),
        fixture.layout.registry_dir.join("registry.sqlite3")
    );
    assert_eq!(
        fixture.layout.run_dir,
        fixture.layout.state_root.join("runs/run-1")
    );
    assert_eq!(fixture.layout.logs_dir, fixture.layout.run_dir.join("logs"));
    assert_eq!(
        fixture.layout.artifacts_dir,
        fixture.layout.run_dir.join("artifacts")
    );
    assert_eq!(
        fixture.layout.summary_path,
        fixture.layout.run_dir.join("summary.json")
    );
    assert!(fixture.layout.registry_dir.is_dir());
    assert!(fixture.layout.logs_dir.is_dir());
    assert!(fixture.layout.artifacts_dir.is_dir());
    assert!(!fixture.layout.summary_path.exists());

    let marker_path = fixture.layout.state_root.join(MARKER_FILE_NAME);
    let marker: StateMarker =
        serde_json::from_slice(&fs::read(marker_path).expect("marker should be readable"))
            .expect("marker should parse");
    assert!(marker.matches_identity(&fixture.identity));
    assert_eq!(marker.marker_version, 1);
    assert_eq!(marker.project_id, "runtime-test");
    assert_eq!(marker.environment, "dev");
    assert_eq!(marker.slot, 0);
    assert_eq!(marker.state_epoch, "1");
    assert_eq!(marker.cleanup_policy, CleanupPolicy::DeleteOnClean);
    assert_eq!(marker.persistence, PersistencePolicy::RunScoped);
}

#[test]
fn selects_explicit_slot_placement() {
    let tmp = TempDir::new();
    let mut value = fixture_model();
    add_slot_one(&mut value, 38180, 38190);
    let model: Model = serde_json::from_value(value).expect("model should parse");
    let selected = select_slot(&model, Some(1)).expect("slot 1 should select");

    let layout = derive_host_placement_for_slot(&model, &selected, "run-2", &tmp.path)
        .expect("slot placement should derive");

    assert_eq!(selected.slot, 1);
    assert_eq!(layout.state_root, tmp.path.join("runtime-test/dev/1"));
    assert_eq!(
        first_candidate_port(&selected.placement.candidate_ports).expect("port should select"),
        38180
    );
}

#[test]
fn slot_one_marker_records_selected_identity() {
    let tmp = TempDir::new();
    let mut value = fixture_model();
    add_slot_one(&mut value, 38180, 38190);
    let model: Model = serde_json::from_value(value).expect("model should parse");
    let admission = admission(&model, &tmp.path);
    let selected = select_slot(&model, Some(1)).expect("slot 1 should select");
    let layout = derive_host_placement_for_slot(&model, &selected, "run-2", &tmp.path)
        .expect("slot placement should derive");
    materialize_run_roots(&layout).expect("roots should materialize");
    let identity = StateIdentity::from_selected_slot(&model, &admission, &selected);

    let marker = write_slot_marker(&layout, &identity).expect("marker should be written");

    assert_eq!(marker.environment, "dev");
    assert_eq!(marker.slot, 1);
    assert!(marker.matches_identity(&identity));
}

#[test]
fn slot_out_of_range_is_refused() {
    let model = model();
    let error = select_slot(&model, Some(1)).expect_err("slot 1 is outside default M1 fixture");

    assert_eq!(error.code, ErrorCode::ModelAdmission);
}

#[cfg(unix)]
#[test]
fn materialization_refuses_symlinked_roots() {
    let tmp = TempDir::new();
    let model = model();
    let layout = derive_host_placement(&model, "run-1", &tmp.path).expect("layout should derive");
    fs::create_dir_all(&layout.state_root).expect("state root should be created");
    fs::create_dir_all(layout.registry_dir.parent().expect("registry has a parent"))
        .expect("registry parent should be created");
    let outside = tmp.path.join("outside-registry");
    fs::create_dir_all(&outside).expect("outside dir should be created");
    std::os::unix::fs::symlink(&outside, &layout.registry_dir)
        .expect("registry symlink should be created");

    let error = materialize_run_roots(&layout).expect_err("symlinked roots must be refused");

    assert_eq!(error.code, ErrorCode::StateUnwritable);
}

#[test]
fn marker_write_refuses_existing_marker_mismatch() {
    let fixture = StateFixture::new();
    let mut other = fixture.identity.clone();
    other.project_id = "other-project".to_string();

    let error = write_slot_marker(&fixture.layout, &other)
        .expect_err("marker mismatch must not be overwritten");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    let marker: StateMarker = serde_json::from_slice(
        &fs::read(fixture.layout.state_root.join(MARKER_FILE_NAME))
            .expect("marker should still be readable"),
    )
    .expect("marker should parse");
    assert_eq!(marker.project_id, "runtime-test");
}

#[test]
fn cleanup_refuses_unmarked_roots() {
    let fixture = StateFixture::new();
    let target = fixture.tmp.path.join("runtime-test/dev/unmarked");
    fs::create_dir_all(&target).expect("unmarked root should be created");

    let error = inspect_cleanup_target(&fixture.tmp.path, &target, &fixture.identity)
        .expect_err("unmarked target should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(target.exists());
}

#[test]
fn cleanup_refuses_path_escape() {
    let fixture = StateFixture::new();
    let outside = fixture.tmp.path.join("../outside-owned-state");
    fs::create_dir_all(&outside).expect("outside target should be created");
    let marker = StateMarker::slot(&fixture.identity);
    fs::write(
        outside.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("outside marker should be written");

    let error = inspect_cleanup_target(&fixture.tmp.path, &outside, &fixture.identity)
        .expect_err("path escape should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn cleanup_refuses_marker_mismatch() {
    let fixture = StateFixture::new();
    let mut marker = StateMarker::slot(&fixture.identity);
    marker.project_id = "other-project".to_string();
    fs::write(
        fixture.layout.state_root.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("marker should be replaced");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect_err("marker mismatch should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(fixture.layout.state_root.exists());
}

#[test]
fn cleanup_refuses_protected_state() {
    let fixture = StateFixture::new();
    let mut marker = StateMarker::slot(&fixture.identity);
    marker.cleanup_policy = CleanupPolicy::Protected;
    fs::write(
        fixture.layout.state_root.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("marker should be replaced");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect_err("protected state should be refused");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

#[test]
fn cleanup_refuses_persistent_state() {
    let fixture = StateFixture::new();
    let mut persistent = fixture.identity.clone();
    persistent.persistence = PersistencePolicy::Persistent;
    let marker = StateMarker::slot(&persistent);
    fs::write(
        fixture.layout.state_root.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(&marker).expect("marker JSON"),
    )
    .expect("marker should be replaced");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &persistent,
    )
    .expect_err("persistent state should be refused");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

#[cfg(unix)]
#[test]
fn cleanup_refuses_symlink_traversal() {
    let fixture = StateFixture::new();
    let outside = fixture.tmp.path.join("outside-file");
    fs::write(&outside, b"outside").expect("outside file should exist");
    std::os::unix::fs::symlink(&outside, fixture.layout.state_root.join("escape-link"))
        .expect("symlink should be created");

    let error = inspect_cleanup_target(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect_err("symlink in cleanup tree should be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(fixture.layout.state_root.exists());
}

#[test]
fn cleanup_refuses_active_registry_refs() {
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO run_leases (
           run_id, environment, slot, service_instance_id, owner_token, heartbeat_at, expires_at,
           status
         ) VALUES ('run-1', 'dev', 0, 'service-1', 'owner', 'now', 'later', 'active')",
    );
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO processes (
           process_key, environment, slot, pid, pgid, start_identity, command_json,
           run_id, status
         ) VALUES ('process-1', 'dev', 0, 1, 1, 'start', '{}', 'run-1', 'running')",
    );
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO ports (
           endpoint_key, environment, slot, service_instance_id, address, port,
           status, owner_process_key
         ) VALUES ('endpoint-1', 'dev', 0, 'service-1', '127.0.0.1', 38080, 'reserved', NULL)",
    );
}

#[test]
fn cleanup_deletes_matching_inactive_state() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();

    let outcome = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut registry,
    )
    .expect("inactive marked state should be deleted");

    assert!(outcome.cleanup_id.starts_with("cleanup-"));
    let cleanup_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM cleanups WHERE cleanup_id = ?1",
            [&outcome.cleanup_id],
            |row| row.get(0),
        )
        .expect("cleanup status should be recorded");
    assert_eq!(cleanup_status, "deleted");
    let cleanup_scope: (String, i64) = registry
        .connection()
        .query_row(
            "SELECT environment, slot FROM cleanups WHERE cleanup_id = ?1",
            [&outcome.cleanup_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cleanup scope should be recorded");
    assert_eq!(cleanup_scope, ("dev".to_string(), 0));
    let mut statement = registry
        .connection()
        .prepare(
            "
            SELECT event_type, environment, slot FROM events
            WHERE payload_json LIKE ?1
            ORDER BY seq
            ",
        )
        .expect("events should be queryable");
    let events = statement
        .query_map([format!("%{}%", outcome.cleanup_id)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .expect("events should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("events should collect");
    assert_eq!(
        events,
        [
            ("cleanup.intent".to_string(), "dev".to_string(), 0),
            ("cleanup.deleted".to_string(), "dev".to_string(), 0),
        ]
    );
    assert!(!fixture.layout.state_root.exists());
    drop(statement);

    // The registry lives outside the deleted state root, so a fresh handle (as a
    // later CLI invocation would open) still finds the prior cleanup evidence and
    // the clean stays idempotent.
    drop(registry);
    let mut reopened = fixture.registry();
    let repeated = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut reopened,
    )
    .expect("repeated cleanup should use prior deletion evidence");
    assert_eq!(repeated, outcome);
}

#[test]
fn clean_reconciles_stale_refs_before_marker_owned_delete() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(
            "
            INSERT INTO runs (
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (
              'run-stale', 'dev', 0, 'service-starting', '/nix/store/test-model/model.json',
              'computed-hash', 'nixfied-runtime-abi:1',
              'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            );
            INSERT INTO services (
              service_instance_id, environment, slot, service_name,
              service_address_hash, endpoint_identity_hash, state_identity_hash,
              runtime_compatibility_hash, target_identity_hash, status,
              endpoint_json, state_root
            ) VALUES (
              'service-stale', 'dev', 0, 'synthetic', 'address', 'endpoint', 'state',
              'runtime', 'target', 'probe-ready', '{}', '/tmp/stale'
            );
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status
            ) VALUES (
              'process-stale', 'dev', 0, 999999, 999999,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-stale', 'service-stale', 'running'
            );
            INSERT INTO ports (
              endpoint_key, environment, slot, service_instance_id, address, port,
              status, owner_process_key
            ) VALUES (
              'endpoint-stale', 'dev', 0, 'service-stale', '127.0.0.1', 38190,
              'active', 'process-stale'
            );
            ",
        )
        .expect("stale refs should be inserted");

    let outcome = clean_reconciled_state(
        &mut registry,
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect("stale refs should reconcile before cleanup");

    assert!(outcome.cleanup_id.starts_with("cleanup-"));
    assert!(!fixture.layout.state_root.exists());
    let process_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = 'process-stale'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    let port_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM ports WHERE endpoint_key = 'endpoint-stale'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");
    assert_eq!(process_status, "stale");
    assert_eq!(port_status, "stale");
}

#[test]
fn cleanup_finishes_interrupted_delete_when_target_is_already_absent() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let marker = StateMarker::slot(&fixture.identity);
    let marker_json = serde_json::to_string(&marker).expect("marker should serialize");
    let target_path = fixture
        .layout
        .state_root
        .canonicalize()
        .expect("state root should canonicalize")
        .display()
        .to_string();
    registry
        .connection_mut()
        .execute(
            "
            INSERT INTO cleanups (
              cleanup_id, environment, slot, target_path, marker_json, status, refusal_reason
            ) VALUES ('cleanup-interrupted', 'dev', 0, ?1, ?2, 'intent', NULL)
            ",
            [&target_path, &marker_json],
        )
        .expect("interrupted cleanup intent should be inserted");
    fs::remove_dir_all(&fixture.layout.state_root).expect("interrupted delete should remove root");

    let outcome = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut registry,
    )
    .expect("missing target with intent evidence should finish as deleted");
    let cleanup_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM cleanups WHERE cleanup_id = 'cleanup-interrupted'",
            [],
            |row| row.get(0),
        )
        .expect("cleanup status should query");
    let deleted_events: i64 = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'cleanup.deleted'",
            [],
            |row| row.get(0),
        )
        .expect("cleanup events should query");

    assert_eq!(outcome.cleanup_id, "cleanup-interrupted");
    assert_eq!(cleanup_status, "deleted");
    assert_eq!(deleted_events, 1);
}

#[test]
fn cleanup_delete_failure_records_failed_without_deleted_success() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let state_parent = fixture
        .layout
        .state_root
        .parent()
        .expect("state root should have parent")
        .to_path_buf();
    let original_mode = fs::metadata(&state_parent)
        .expect("state parent metadata should read")
        .permissions()
        .mode();
    let restore = PermissionRestore {
        path: state_parent.clone(),
        mode: original_mode,
    };
    fs::set_permissions(&state_parent, fs::Permissions::from_mode(0o500))
        .expect("state parent should be made non-writable");

    let result = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut registry,
    );
    drop(restore);
    let error = result.expect_err("delete failure should refuse cleanup");
    let cleanup_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM cleanups ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("cleanup status should query");
    let failed_events: i64 = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'cleanup.failed'",
            [],
            |row| row.get(0),
        )
        .expect("failed cleanup events should query");
    let deleted_events: i64 = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'cleanup.deleted'",
            [],
            |row| row.get(0),
        )
        .expect("deleted cleanup events should query");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert_eq!(cleanup_status, "failed");
    assert_eq!(failed_events, 1);
    assert_eq!(deleted_events, 0);
    assert!(fixture.layout.state_root.exists());
}

#[test]
fn clean_marks_active_port_stale_after_owner_process_is_proven_dead() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(
            "
            INSERT INTO runs (
              run_id, environment, slot, status, model_path, computed_model_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (
              'run-stale-port', 'dev', 0, 'service-starting', '/nix/store/test-model/model.json',
              'computed-hash', 'nixfied-runtime-abi:1',
              'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            );
            INSERT INTO services (
              service_instance_id, environment, slot, service_name,
              service_address_hash, endpoint_identity_hash, state_identity_hash,
              runtime_compatibility_hash, target_identity_hash, status,
              endpoint_json, state_root
            ) VALUES (
              'service-stale-port', 'dev', 0, 'synthetic', 'address', 'endpoint', 'state',
              'runtime', 'target', 'stopped', '{}', '/tmp/stale-port'
            );
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status
            ) VALUES (
              'process-stale-port', 'dev', 0, 999998, 999998,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-stale-port', 'service-stale-port', 'stopped'
            );
            INSERT INTO ports (
              endpoint_key, environment, slot, service_instance_id, address, port,
              status, owner_process_key
            ) VALUES (
              'endpoint-stale-port', 'dev', 0, 'service-stale-port', '127.0.0.1', 38191,
              'active', 'process-stale-port'
            );
            ",
        )
        .expect("stale port refs should be inserted");

    let outcome = clean_reconciled_state(
        &mut registry,
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
    )
    .expect("stale port should reconcile before cleanup");
    let port_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM ports WHERE endpoint_key = 'endpoint-stale-port'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");
    let port_events: i64 = registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'port.stale'",
            [],
            |row| row.get(0),
        )
        .expect("port events should query");

    assert!(outcome.cleanup_id.starts_with("cleanup-"));
    assert_eq!(port_status, "stale");
    assert_eq!(port_events, 1);
}

fn assert_cleanup_refused_with_active_ref(sql: &str) {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(sql)
        .expect("active ref should be inserted");

    let error = clean_marked_state(
        &fixture.layout.state_base,
        &fixture.layout.state_root,
        &fixture.identity,
        &mut registry,
    )
    .expect_err("active registry refs should refuse cleanup");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

struct PermissionRestore {
    path: PathBuf,
    mode: u32,
}

impl Drop for PermissionRestore {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

struct StateFixture {
    tmp: TempDir,
    layout: nixfied_runtime::state::HostPlacement,
    identity: StateIdentity,
}

impl StateFixture {
    fn new() -> Self {
        let tmp = TempDir::new();
        let model = model();
        let admission = admission(&model, &tmp.path);
        let layout =
            derive_host_placement(&model, "run-1", &tmp.path).expect("layout should derive");
        materialize_run_roots(&layout).expect("roots should materialize");
        let identity = StateIdentity::from_model(&model, &admission);
        write_slot_marker(&layout, &identity).expect("marker should be written");
        Self {
            tmp,
            layout,
            identity,
        }
    }

    fn registry(&self) -> Registry {
        Registry::open_or_create(
            self.layout.registry_path(),
            &RegistryIdentity::for_slot(
                &self.identity.project_id,
                &self.identity.environment,
                self.identity.slot,
                &self.identity.runtime_abi,
                &self.identity.toolchain_id,
            ),
        )
        .expect("registry should open")
    }
}

fn admission(model: &Model, source_root: &Path) -> Admission {
    Admission {
        model_path: PathBuf::from("/nix/store/test-model/model.json"),
        computed_model_hash: "computed-hash".to_string(),
        raw_len: 100,
        project_id: model.project.project_id.clone(),
        runtime_abi: model.runtime_abi.clone(),
        toolchain_id: model.toolchain_id.clone(),
        target_system: model.target.system.clone(),
        source: admitted_source(source_root),
        generator_json: serde_json::to_string(&model.generator).unwrap(),
        target_json: serde_json::to_string(&model.target).unwrap(),
        execution_model: nixfied_runtime::execution::lower(model).expect("model should lower"),
    }
}

fn admitted_source(source_root: &Path) -> AdmittedSource {
    AdmittedSource {
        codebase_id: "main".to_string(),
        logical_root: ".".to_string(),
        observed_root: source_root
            .canonicalize()
            .expect("source root should canonicalize"),
        source_mode: SourceMode::LiveWorkspace,
        source_identity: "live".to_string(),
        dirty_policy: DirtyPolicy::Warn,
        admission_fingerprint_policy: "live-fingerprint".to_string(),
    }
}

fn model() -> Model {
    serde_json::from_value(fixture_model()).expect("fixture model should parse")
}

fn add_slot_one(value: &mut Value, start: u16, end: u16) {
    value["slotPolicy"]["max"] = json!(1);
    value["placement"]["slotPlacements"]["1"] = json!({
        "slot": 1,
        "candidatePorts": {
            "start": start,
            "end": end
        }
    });
}

fn fixture_model() -> Value {
    common::synthetic_model_default(38080, 38090)
}
