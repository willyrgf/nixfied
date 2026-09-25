//! Marker preparation semantics: a slot is owned by project/environment/slot, not
//! by one manifest build. These tests drive `prepare_slot_state` through the
//! second-run / changed-manifest / retention / interrupted-run matrix that
//! first surfaced in MFM's v2 adoption.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nixfied_manifest::{Manifest, PersistencePolicy};
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::state::{
    HostPlacement, MARKER_FILE_NAME, StateIdentity, StateMarker, commit_slot_marker,
    derive_host_placement, prepare_slot_state,
};
use nixfied_runtime::{ErrorCode, RunAdmission};
use serde_json::Value;

mod common;
use common::*;

#[test]
fn second_run_same_manifest_adopts_marker() {
    let fixture = PreparationFixture::new();
    let identity = fixture.identity(false);
    let first = fixture
        .prepare("run-1", &identity)
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();

    let second = fixture
        .prepare("run-2", &identity)
        .expect("second run of the same manifest should adopt the slot");

    assert!(!first.provenance_refreshed);
    assert!(!second.provenance_refreshed);
    assert!(sentinel.exists(), "adopted state root must be preserved");
    assert_eq!(
        fixture.marker().computed_manifest_hash,
        expected_hash(&fixture.manifest, false)
    );
    assert_eq!(fixture.provenance_event_count(), 0);
}

#[test]
fn changed_manifest_hash_updates_provenance_and_preserves_state_root() {
    let fixture = PreparationFixture::new();
    fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();
    let generation = fixture.marker().data_generation;

    let report = fixture
        .prepare("run-2", &fixture.identity(true))
        .expect("a changed manifest hash should refresh provenance, not refuse");

    assert!(report.provenance_refreshed);
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
    assert_eq!(
        marker.data_generation, generation,
        "provenance refresh preserves the data generation"
    );
    assert_eq!(fixture.provenance_event_count(), 1);
    let payload = fixture.last_provenance_event_payload();
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
fn symlinked_ancestry_rejects_a_provenance_refresh_before_any_registry_event() {
    let fixture = PreparationFixture::new();
    fixture.prepare("run-1", &fixture.identity(false)).unwrap();
    let state_root = fixture.state_root();
    let project = state_root.parent().unwrap().parent().unwrap().to_path_buf();
    let moved = project.with_file_name("moved-project");
    fs::rename(&project, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &project).unwrap();
    let marker = fs::read(state_root.join(MARKER_FILE_NAME)).unwrap();

    let error = fixture
        .prepare("run-2", &fixture.identity(true))
        .expect_err("a symlinked state ancestry must reject");

    // The marker is inspected through held descriptors, so the redirected
    // ancestry is rejected before any registry event or marker write.
    assert_eq!(error.code, ErrorCode::StateUnowned, "{error:?}");
    assert_eq!(fixture.provenance_event_count(), 0);
    assert_eq!(fs::read(state_root.join(MARKER_FILE_NAME)).unwrap(), marker);
}

#[test]
fn old_marker_version_rejects_without_data_mutation() {
    let fixture = PreparationFixture::new();
    fixture.prepare("run-1", &fixture.identity(false)).unwrap();
    let sentinel = fixture.plant_sentinel();
    let path = fixture.state_root().join(MARKER_FILE_NAME);
    let mut value = serde_json::to_value(fixture.marker()).unwrap();
    value["markerVersion"] = serde_json::json!(2);
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &bytes).unwrap();
    let error = fixture
        .prepare("run-2", &fixture.identity(true))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
    assert_eq!(fixture.provenance_event_count(), 0);
}

#[test]
fn pre_existing_unmarked_state_root_refuses() {
    let fixture = PreparationFixture::new();
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
    let fixture = PreparationFixture::new();
    let state_root = fixture.state_root();
    fs::create_dir_all(&state_root).expect("state root should be creatable");

    let report = fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("an empty state root has no state to adopt and is a fresh slot");

    assert!(!report.provenance_refreshed);
    assert_eq!(
        fixture.marker().computed_manifest_hash,
        expected_hash(&fixture.manifest, false)
    );
}

#[test]
fn foreign_ownership_or_runtime_abi_refuses_state_unowned() {
    for foreign_abi in [false, true] {
        let fixture = PreparationFixture::new();
        fixture
            .prepare("run-1", &fixture.identity(false))
            .expect("first run should prepare a fresh slot");
        let mut identity = fixture.identity(foreign_abi);
        if foreign_abi {
            let mut marker = fixture.marker();
            marker.runtime_abi = "nixfied-runtime-abi:0-foreign".to_string();
            fixture.rewrite_marker(&marker);
        } else {
            identity.project_id = "other-project".to_string();
        }

        let error = fixture
            .prepare("run-2", &identity)
            .expect_err("a foreign owner or ABI must be refused, never refreshed");

        assert_eq!(error.code, ErrorCode::StateUnowned);
        if !foreign_abi {
            assert_eq!(fixture.marker().project_id, "runtime-test");
        }
    }
}

#[test]
fn changed_manifest_cannot_weaken_existing_retention() {
    {
        let fixture = PreparationFixture::new();
        let mut persistent = fixture.identity(false);
        persistent.persistence = PersistencePolicy::Persistent;
        fixture.prepare("run-1", &persistent).unwrap();
        let sentinel = fixture.plant_sentinel();
        let before = fixture.marker();
        let error = fixture
            .prepare("run-2", &fixture.identity(true))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::CleanupRefused);
        assert_eq!(fixture.marker(), before);
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
        assert_eq!(fixture.provenance_event_count(), 0);
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
        let identity_a = StateIdentity::from_admission(admission_a.common());
        let mut registry = open_registry(&placement, &manifest);
        registry.authority().claim_run_dir(&placement).unwrap();
        commit_slot_marker(&registry, &identity_a).expect("marker should be written");
        let service = start_fixture_service(
            &admission_a,
            &placement,
            &mut registry,
            "run-a",
            &synthetic_endpoint(23980),
            &nixfied_runtime::cancellation::CancellationToken::new(),
            None,
        )
        .expect("old-manifest service should start");
        let pgid = service.info().pgid;
        let process_key = service.info().process_key.clone();

        let admission_b = admission(&manifest, &tmp.path, changed);
        let identity_b = StateIdentity::from_admission(admission_b.common());
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
            let error = prepare_slot_state(&identity_b, &mut registry).unwrap_err();
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
        nixfied_runtime::control::stop_recorded_processes(&mut registry, 5000)
            .expect("exclusive recovery settles all manifest provenances");
        let report = prepare_slot_state(&identity_b, &mut registry)
            .expect("preparation follows successful recovery");

        assert_eq!(report.provenance_refreshed, changed);
        poll_until(Duration::from_secs(5), "an empty predecessor group", || {
            (!process_group_has_non_zombie_member(pgid)).then_some(())
        });
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
fn interrupted_run_recovers_then_provenance_refresh_proceeds() {
    let fixture = PreparationFixture::new();
    fixture
        .prepare("run-1", &fixture.identity(false))
        .expect("first run should prepare a fresh slot");
    // A crashed runtime's leftovers: rows still active, process long dead.
    let registry = fixture.registry();
    seed_run(registry.connection(), "run-interrupted", None);
    seed_process(
        registry.connection(),
        SeedProcess {
            key: "process-interrupted",
            run_id: "run-interrupted",
            service: Some(("service-interrupted", "synthetic")),
            ..SeedProcess::default()
        },
    );
    registry
        .close()
        .expect("interrupted predecessor releases slot authority");

    let report = fixture
        .prepare("run-2", &fixture.identity(true))
        .expect("the refresh must recover interrupted leftovers, not trip on them");

    assert!(report.provenance_refreshed);
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

struct PreparationFixture {
    tmp: TempDir,
    manifest: Manifest,
}

impl PreparationFixture {
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
    ) -> Result<nixfied_runtime::state::PreparationReport, nixfied_runtime::RuntimeError> {
        let placement = self.placement(run_id);
        let mut registry = open_registry(&placement, &self.manifest);
        nixfied_runtime::control::stop_recorded_processes(&mut registry, 1000)?;
        prepare_slot_state(identity, &mut registry)
    }

    fn registry(&self) -> Registry {
        let placement = self.placement("control");
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

    fn provenance_event_count(&self) -> i64 {
        self.registry()
            .connection()
            .query_row(
                "SELECT count(*) FROM events WHERE event_type = 'state.provenance-refreshed'",
                [],
                |row| row.get(0),
            )
            .expect("events should query")
    }

    fn last_provenance_event_payload(&self) -> Value {
        let payload: String = self
            .registry()
            .connection()
            .query_row(
                "SELECT payload_json FROM events
                 WHERE event_type = 'state.provenance-refreshed' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("provenance event should exist");
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
