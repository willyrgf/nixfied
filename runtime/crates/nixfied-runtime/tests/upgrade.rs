//! Marker upgrade semantics: a slot is owned by project/environment/slot, not
//! by one manifest build. These tests drive `prepare_slot_state` through the
//! second-run / changed-manifest / retention / interrupted-run matrix that
//! first surfaced in MFM's v2 adoption.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use nixfied_manifest::{CleanupPolicy, Manifest, PersistencePolicy};
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::state::{
    HostPlacement, MARKER_FILE_NAME, StateIdentity, StateMarker, commit_slot_marker,
    derive_host_placement, materialize_registry_root, materialize_run_roots, prepare_slot_state,
};
use nixfied_runtime::{ErrorCode, RunAdmission};
use serde_json::Value;

mod common;
use common::*;

#[test]
fn second_run_same_manifest_adopts_marker() {
    let fixture = UpgradeFixture::new();
    let identity = fixture.identity(false);
    let first = fixture
        .prepare("run-1", &identity)
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();

    let second = fixture
        .prepare("run-2", &identity)
        .expect("second run of the same manifest should adopt the slot");

    assert!(!first.upgraded);
    assert!(!second.upgraded);
    assert!(sentinel.exists(), "adopted state root must be preserved");
    assert_eq!(
        fixture.marker().computed_manifest_hash,
        expected_hash(&fixture.manifest, false)
    );
    assert_eq!(fixture.upgrade_event_count(), 0);
}

#[test]
fn changed_manifest_hash_updates_provenance_and_preserves_state_root() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();

    let report = fixture
        .prepare("run-2", &fixture.identity(true))
        .expect("a changed manifest hash should upgrade, not refuse");

    assert!(report.upgraded);
    assert_eq!(
        report.from_manifest_hash.as_deref(),
        Some(expected_hash(&fixture.manifest, false).as_str())
    );
    assert!(
        sentinel.exists(),
        "provenance changes must preserve the state root"
    );
    let marker = fixture.marker();
    assert_eq!(
        marker.computed_manifest_hash,
        expected_hash(&fixture.manifest, true)
    );
    assert_eq!(fixture.upgrade_event_count(), 1);
    let payload = fixture.last_upgrade_event_payload();
    assert_eq!(
        payload["fromManifestHash"],
        expected_hash(&fixture.manifest, false)
    );
    assert_eq!(
        payload["toManifestHash"],
        expected_hash(&fixture.manifest, true)
    );
    assert!(payload.get("cleaned").is_none());
}

#[test]
fn obsolete_epoch_and_old_marker_version_reject_without_data_mutation() {
    for obsolete_epoch in [false, true] {
        let fixture = UpgradeFixture::new();
        fixture.prepare("run-1", &fixture.identity(false)).unwrap();
        let sentinel = fixture.plant_sentinel();
        let path = fixture.state_root().join(MARKER_FILE_NAME);
        let mut value = serde_json::to_value(fixture.marker()).unwrap();
        if obsolete_epoch {
            value["stateEpoch"] = serde_json::json!("2");
        } else {
            value["markerVersion"] = serde_json::json!(1);
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        let error = fixture
            .prepare("run-2", &fixture.identity(true))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StateUnowned);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
        assert_eq!(fixture.upgrade_event_count(), 0);
    }
}

#[test]
fn pre_existing_unmarked_state_root_refuses() {
    let fixture = UpgradeFixture::new();
    let state_root = fixture.state_root();
    fs::create_dir_all(&state_root).expect("state root should be creatable");
    fs::write(state_root.join("leftover"), b"data").expect("leftover should be written");

    let error = fixture
        .prepare("run-1", &fixture.identity(false))
        .expect_err("a non-empty state root without a marker must be refused, not adopted");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert!(
        !state_root.join(MARKER_FILE_NAME).exists(),
        "the refusal must not write a marker into the unowned tree"
    );
}

#[test]
fn pre_existing_empty_state_root_is_fresh() {
    let fixture = UpgradeFixture::new();
    let state_root = fixture.state_root();
    fs::create_dir_all(&state_root).expect("state root should be creatable");

    let report = fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("an empty state root has no state to adopt and is a fresh slot");

    assert!(!report.upgraded);
    assert_eq!(
        fixture.marker().computed_manifest_hash,
        expected_hash(&fixture.manifest, false)
    );
}

#[test]
fn changed_ownership_refuses_state_unowned() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("first run should prepare a fresh slot");
    let mut foreign = fixture.identity(false);
    foreign.project_id = "other-project".to_string();

    let error = fixture
        .prepare("run-2", &foreign)
        .expect_err("a foreign owner must be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_eq!(fixture.marker().project_id, "runtime-test");
}

#[test]
fn runtime_abi_mismatch_refuses() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("first run should prepare a fresh slot");
    let mut marker = fixture.marker();
    marker.runtime_abi = "nixfied-runtime-abi:0-foreign".to_string();
    fixture.rewrite_marker(&marker);

    let error = fixture
        .prepare("run-2", &fixture.identity(true))
        .expect_err("a foreign runtime ABI must be refused, never upgraded");

    assert_eq!(error.code, ErrorCode::StateUnowned);
}

#[test]
fn changed_manifest_cannot_weaken_existing_retention() {
    for persistent in [false, true] {
        let fixture = UpgradeFixture::new();
        let mut protected = fixture.identity(false);
        if persistent {
            protected.persistence = PersistencePolicy::Persistent;
        } else {
            protected.cleanup_policy = CleanupPolicy::Protected;
        }
        fixture.prepare("run-1", &protected).unwrap();
        let sentinel = fixture.plant_sentinel();
        let before = fixture.marker();
        let error = fixture
            .prepare("run-2", &fixture.identity(true))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::CleanupRefused);
        assert_eq!(fixture.marker(), before);
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
        assert_eq!(fixture.upgrade_event_count(), 0);
    }
}

#[test]
fn predecessor_recovery_is_required_for_both_same_and_changed_manifest() {
    for changed in [false, true] {
        let tmp = TempDir::new();
        let manifest: Manifest = serde_json::from_value(synthetic_manifest(
            &common::test_sleep(),
            &["30"],
            23980,
            23990,
        ))
        .expect("manifest should parse");
        let admission_a = admission(&manifest, &tmp.path, false);
        let placement =
            derive_host_placement(&manifest, "run-a", &tmp.path).expect("layout derives");
        materialize_run_roots(&placement).expect("roots should materialize");
        let identity_a = StateIdentity::from_admission(admission_a.common());
        commit_slot_marker(&placement, &identity_a).expect("marker should be written");
        let mut registry = open_registry(&placement, &manifest);
        let service = start_fixture_service(
            &admission_a,
            &placement,
            &mut registry,
            "run-a",
            &nixfied_runtime::slot::select_slot(&manifest, None).unwrap(),
            23980,
        )
        .expect("old-manifest service should start");
        let pgid = service.info().pgid;
        let process_key = service.info().process_key.clone();

        let admission_b = admission(&manifest, &tmp.path, changed);
        let identity_b = StateIdentity::from_admission(admission_b.common());
        let placement_b =
            derive_host_placement(&manifest, "run-b", &tmp.path).expect("layout derives");
        let marker_path = placement.state_root.join(MARKER_FILE_NAME);
        let marker_before = fs::read(&marker_path).unwrap();
        for (status, expected) in [
            ("running", ErrorCode::CleanupRefused),
            // Even terminal process evidence cannot hide the open endpoint.
            ("stopped", ErrorCode::CleanupRefused),
            ("invalid", ErrorCode::RegistryCorrupt),
        ] {
            registry
                .connection()
                .execute(
                    "UPDATE processes SET status = ?2 WHERE process_key = ?1",
                    rusqlite::params![process_key, status],
                )
                .unwrap();
            let error = prepare_slot_state(&placement_b, &identity_b, &mut registry).unwrap_err();
            assert_eq!(error.code, expected);
            assert!(
                process_group_has_non_zombie_member(pgid),
                "preparation must not signal a predecessor"
            );
            assert_eq!(fs::read(&marker_path).unwrap(), marker_before);
        }
        registry
            .connection()
            .execute(
                "UPDATE processes SET status = 'running' WHERE process_key = ?1",
                [&process_key],
            )
            .unwrap();
        nixfied_runtime::control::down_owned_process_groups(&mut registry, 5000)
            .expect("exclusive recovery settles all manifest provenances");
        let report = prepare_slot_state(&placement_b, &identity_b, &mut registry)
            .expect("preparation follows successful recovery");

        assert_eq!(report.upgraded, changed);
        assert!(
            wait_for_group_exit(pgid, 5000),
            "the predecessor process group must be empty after recovery"
        );
        let process_status: String = registry
            .connection()
            .query_row(
                "SELECT status FROM processes WHERE process_key = ?1",
                [&process_key],
                |row| row.get(0),
            )
            .expect("process row should exist");
        assert_eq!(process_status, "stopped");
        drop(service);
    }
}

#[test]
fn interrupted_run_reconciles_then_upgrade_proceeds() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("first run should prepare a fresh slot");
    // A crashed runtime's leftovers: rows still active, process long dead.
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(
            "
            INSERT INTO runs (
              run_id, environment, slot, execution_outcome, manifest_path, computed_manifest_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            ) VALUES (
              'run-interrupted', 'dev', 0, NULL,
              '/nix/store/manifest-a/manifest.json', 'hash-a', 'nixfied-runtime-abi:1',
              'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            );
            ",
        )
        .expect("interrupted run row should insert");
    registry
        .connection_mut()
        .execute_batch(
            "
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status, service_name, role
            ) VALUES (
              'process-interrupted', 'dev', 0, 999999, 999999,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-interrupted', 'service-interrupted', 'running', 'synthetic', 'service'
            );
            ",
        )
        .expect("interrupted process row should insert");
    registry
        .close()
        .expect("interrupted predecessor releases slot authority");

    let report = fixture
        .prepare("run-2", &fixture.identity(true))
        .expect("the upgrade must reconcile interrupted leftovers, not trip on them");

    assert!(report.upgraded);
    let process_status: String = fixture
        .registry()
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = 'process-interrupted'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    assert_eq!(process_status, "stale");
    assert_eq!(
        fixture.marker().computed_manifest_hash,
        expected_hash(&fixture.manifest, true)
    );
}

fn wait_for_group_exit(pgid: i32, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if !process_group_has_non_zombie_member(pgid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Whether any non-zombie process remains in the group. A stopped child stays
/// a zombie until its owner reaps it, so a raw `kill(-pgid, 0)` would count it.
fn process_group_has_non_zombie_member(pgid: i32) -> bool {
    let output = std::process::Command::new("ps")
        .arg("-axo")
        .arg("pgid=,stat=")
        .output()
        .expect("ps should inspect process groups");
    assert!(
        output.status.success(),
        "ps failed while inspecting process groups: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let current_pgid = fields.next()?.parse::<i32>().ok()?;
            let stat = fields.next().unwrap_or("");
            Some((current_pgid, stat.to_string()))
        })
        .any(|(current_pgid, stat)| current_pgid == pgid && !stat.starts_with('Z'))
}

struct UpgradeFixture {
    tmp: TempDir,
    manifest: Manifest,
}

impl UpgradeFixture {
    fn new() -> Self {
        let tmp = TempDir::new();
        let manifest: Manifest =
            serde_json::from_value(fixture_manifest()).expect("fixture manifest should parse");
        Self { tmp, manifest }
    }

    fn identity(&self, pretty: bool) -> StateIdentity {
        let admission = admission(&self.manifest, &self.tmp.path, pretty);
        StateIdentity::from_admission(admission.common())
    }

    fn placement(&self, run_id: &str) -> HostPlacement {
        derive_host_placement(&self.manifest, run_id, &self.tmp.path).expect("layout should derive")
    }

    fn prepare(
        &self,
        run_id: &str,
        identity: &StateIdentity,
    ) -> Result<nixfied_runtime::state::UpgradeReport, nixfied_runtime::RuntimeError> {
        let placement = self.placement(run_id);
        materialize_registry_root(&placement)?;
        let mut registry = open_registry(&placement, &self.manifest);
        nixfied_runtime::control::down_owned_process_groups(&mut registry, 1000)?;
        prepare_slot_state(&placement, identity, &mut registry)
    }

    fn registry(&self) -> Registry {
        let placement = self.placement("control");
        materialize_registry_root(&placement).expect("registry root should materialize");
        open_registry(&placement, &self.manifest)
    }

    fn state_root(&self) -> PathBuf {
        self.placement("control").state_root
    }

    fn plant_sentinel(&self) -> PathBuf {
        let sentinel = self.state_root().join("sentinel");
        fs::write(&sentinel, b"keep").expect("sentinel should be written");
        sentinel
    }

    fn marker(&self) -> StateMarker {
        serde_json::from_slice(
            &fs::read(self.state_root().join(MARKER_FILE_NAME)).expect("marker should be readable"),
        )
        .expect("marker should parse")
    }

    fn rewrite_marker(&self, marker: &StateMarker) {
        fs::write(
            self.state_root().join(MARKER_FILE_NAME),
            serde_json::to_vec_pretty(marker).expect("marker should serialize"),
        )
        .expect("marker should be rewritten");
    }

    fn upgrade_event_count(&self) -> i64 {
        self.registry()
            .connection()
            .query_row(
                "SELECT count(*) FROM events WHERE event_type = 'state.upgraded'",
                [],
                |row| row.get(0),
            )
            .expect("events should query")
    }

    fn last_upgrade_event_payload(&self) -> Value {
        let payload: String = self
            .registry()
            .connection()
            .query_row(
                "SELECT payload_json FROM events
                 WHERE event_type = 'state.upgraded' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("upgrade event should exist");
        serde_json::from_str(&payload).expect("payload should parse")
    }
}

fn open_registry(placement: &HostPlacement, manifest: &Manifest) -> Registry {
    Registry::open_or_create(
        registry_guard(placement),
        &RegistryIdentity::default_slot(
            &manifest.project.project_id,
            &manifest.runtime_abi,
            &manifest.toolchain_id,
        ),
    )
    .expect("registry should open")
}

fn manifest_bytes(manifest: &Manifest, pretty: bool) -> Vec<u8> {
    if pretty {
        serde_json::to_vec_pretty(manifest)
    } else {
        serde_json::to_vec(manifest)
    }
    .expect("fixture should serialize")
}

fn expected_hash(manifest: &Manifest, pretty: bool) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(manifest_bytes(manifest, pretty)))
}

fn admission(manifest: &Manifest, source_root: &Path, pretty: bool) -> RunAdmission {
    common::admit_fixture_bytes(
        &manifest_bytes(manifest, pretty),
        source_root,
        Path::new("/nix/store"),
    )
}

fn fixture_manifest() -> Value {
    common::test_child_manifest(23880, 23890)
}
