//! Marker upgrade semantics: a slot is owned by project/environment/slot, not
//! by one model build. These tests drive `prepare_slot_state` through the
//! second-run / changed-model / changed-epoch / interrupted-run matrix that
//! first surfaced in MFM's v2 adoption.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use nixfied_model::{CleanupPolicy, DirtyPolicy, Model, SourceMode};
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::state::{
    HostPlacement, MARKER_FILE_NAME, StateIdentity, StateMarker, commit_slot_marker,
    derive_host_placement, materialize_registry_root, materialize_run_roots, prepare_slot_state,
};
use nixfied_runtime::{Admission, AdmittedSource, ErrorCode};
use serde_json::Value;

mod common;
use common::*;

#[test]
fn second_run_same_model_adopts_marker() {
    let fixture = UpgradeFixture::new();
    let identity = fixture.identity("hash-a");
    let first = fixture
        .prepare("run-1", &identity)
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();

    let second = fixture
        .prepare("run-2", &identity)
        .expect("second run of the same model should adopt the slot");

    assert!(!first.upgraded);
    assert!(!second.upgraded);
    assert!(sentinel.exists(), "adopted state root must be preserved");
    assert_eq!(fixture.marker().computed_model_hash, "hash-a");
    assert_eq!(fixture.upgrade_event_count(), 0);
}

#[test]
fn changed_model_hash_same_epoch_upgrades_and_preserves_state_root() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();

    let report = fixture
        .prepare("run-2", &fixture.identity("hash-b"))
        .expect("a changed model hash should upgrade, not refuse");

    assert!(report.upgraded);
    assert!(!report.cleaned);
    assert_eq!(report.from_model_hash.as_deref(), Some("hash-a"));
    assert!(
        sentinel.exists(),
        "same-epoch upgrade must preserve the state root"
    );
    let marker = fixture.marker();
    assert_eq!(marker.computed_model_hash, "hash-b");
    assert_eq!(marker.state_epoch, "1");
    assert_eq!(fixture.upgrade_event_count(), 1);
    let payload = fixture.last_upgrade_event_payload();
    assert_eq!(payload["fromModelHash"], "hash-a");
    assert_eq!(payload["toModelHash"], "hash-b");
    assert_eq!(payload["cleaned"], false);
}

#[test]
fn changed_state_epoch_upgrades_and_cleans_state_root() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();

    let mut epoch2_value = fixture_model();
    epoch2_value["state"]["stateEpoch"] = serde_json::json!("2");
    let epoch2_model: Model =
        serde_json::from_value(epoch2_value).expect("epoch-2 model should parse");
    let epoch2_admission = admission(&epoch2_model, &fixture.tmp.path, "hash-b");
    let epoch2_identity = StateIdentity::from_model(&epoch2_model, &epoch2_admission);

    let report = fixture
        .prepare("run-2", &epoch2_identity)
        .expect("a changed state epoch should upgrade with a clean");

    assert!(report.upgraded);
    assert!(report.cleaned);
    assert!(
        !sentinel.exists(),
        "epoch upgrade must clean the old state root"
    );
    assert!(
        fixture.placement("run-2").registry_path().exists(),
        "the registry must survive the upgrade clean"
    );
    let marker = fixture.marker();
    assert_eq!(marker.state_epoch, "2");
    assert_eq!(marker.computed_model_hash, "hash-b");
    let payload = fixture.last_upgrade_event_payload();
    assert_eq!(payload["fromEpoch"], "1");
    assert_eq!(payload["toEpoch"], "2");
    assert_eq!(payload["cleaned"], true);
}

#[test]
fn pre_existing_unmarked_state_root_refuses() {
    let fixture = UpgradeFixture::new();
    let state_root = fixture.state_root();
    fs::create_dir_all(&state_root).expect("state root should be creatable");
    fs::write(state_root.join("leftover"), b"data").expect("leftover should be written");

    let error = fixture
        .prepare("run-1", &fixture.identity("hash-a"))
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
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("an empty state root has no state to adopt and is a fresh slot");

    assert!(!report.upgraded);
    assert_eq!(fixture.marker().computed_model_hash, "hash-a");
}

#[test]
fn changed_ownership_refuses_state_unowned() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("first run should prepare a fresh slot");
    let mut foreign = fixture.identity("hash-a");
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
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("first run should prepare a fresh slot");
    let mut marker = fixture.marker();
    marker.runtime_abi = "nixfied-runtime-abi:0-foreign".to_string();
    fixture.rewrite_marker(&marker);

    let error = fixture
        .prepare("run-2", &fixture.identity("hash-b"))
        .expect_err("a foreign runtime ABI must be refused, never upgraded");

    assert_eq!(error.code, ErrorCode::StateUnowned);
}

#[test]
fn epoch_change_on_protected_state_refuses_upgrade_clean() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("first run should prepare a fresh slot");
    let sentinel = fixture.plant_sentinel();
    let mut marker = fixture.marker();
    marker.cleanup_policy = CleanupPolicy::Protected;
    fixture.rewrite_marker(&marker);

    let mut epoch2_value = fixture_model();
    epoch2_value["state"]["stateEpoch"] = serde_json::json!("2");
    let epoch2_model: Model =
        serde_json::from_value(epoch2_value).expect("epoch-2 model should parse");
    let epoch2_admission = admission(&epoch2_model, &fixture.tmp.path, "hash-b");
    let epoch2_identity = StateIdentity::from_model(&epoch2_model, &epoch2_admission);

    let error = fixture
        .prepare("run-2", &epoch2_identity)
        .expect_err("protected state must not be deleted by an epoch upgrade");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(
        sentinel.exists(),
        "protected state must survive the refusal"
    );
}

#[test]
fn live_old_model_service_is_torn_down_on_upgrade() {
    let tmp = TempDir::new();
    let model: Model = serde_json::from_value(synthetic_model("/bin/sleep", &["30"], 23980, 23990))
        .expect("model should parse");
    let admission_a = admission(&model, &tmp.path, "hash-a");
    let placement = derive_host_placement(&model, "run-a", &tmp.path).expect("layout derives");
    materialize_run_roots(&placement).expect("roots should materialize");
    let identity_a = StateIdentity::from_model(&model, &admission_a);
    commit_slot_marker(&placement, &identity_a).expect("marker should be written");
    let mut registry = open_registry(&placement, &model);
    let service = start_synthetic_service(
        &model,
        &admission_a,
        &placement,
        &mut registry,
        "run-a",
        23980,
    )
    .expect("old-model service should start");
    let pgid = service.pgid;
    let process_key = service.process_key.clone();

    let admission_b = admission(&model, &tmp.path, "hash-b");
    let identity_b = StateIdentity::from_model(&model, &admission_b);
    let placement_b = derive_host_placement(&model, "run-b", &tmp.path).expect("layout derives");
    let report = prepare_slot_state(&placement_b, &identity_b, &mut registry, 5000)
        .expect("upgrade should tear down the old model's live service");

    assert!(report.upgraded);
    assert!(
        wait_for_group_exit(pgid, 5000),
        "the old model's process group must be empty after the upgrade"
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

#[test]
fn interrupted_run_reconciles_then_upgrade_proceeds() {
    let fixture = UpgradeFixture::new();
    fixture
        .prepare("run-1", &fixture.identity("hash-a"))
        .expect("first run should prepare a fresh slot");
    // A crashed runtime's leftovers: rows still active, process long dead.
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
              'run-interrupted', 'dev', 0, 'service-starting',
              '/nix/store/model-a/model.json', 'hash-a', 'nixfied-runtime-abi:1',
              'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            );
            ",
        )
        .expect("interrupted run row should insert");
    insert_registry_service(
        &mut registry,
        &RegistryServiceRow::synthetic("service-interrupted", "probe-ready", "/tmp/interrupted"),
    );
    registry
        .connection_mut()
        .execute_batch(
            "
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status
            ) VALUES (
              'process-interrupted', 'dev', 0, 999999, 999999,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-interrupted', 'service-interrupted', 'running'
            );
            ",
        )
        .expect("interrupted process row should insert");

    let report = fixture
        .prepare("run-2", &fixture.identity("hash-b"))
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
    assert_eq!(fixture.marker().computed_model_hash, "hash-b");
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
    model: Model,
}

impl UpgradeFixture {
    fn new() -> Self {
        let tmp = TempDir::new();
        let model: Model =
            serde_json::from_value(fixture_model()).expect("fixture model should parse");
        Self { tmp, model }
    }

    fn identity(&self, hash: &str) -> StateIdentity {
        let admission = admission(&self.model, &self.tmp.path, hash);
        StateIdentity::from_model(&self.model, &admission)
    }

    fn placement(&self, run_id: &str) -> HostPlacement {
        derive_host_placement(&self.model, run_id, &self.tmp.path).expect("layout should derive")
    }

    fn prepare(
        &self,
        run_id: &str,
        identity: &StateIdentity,
    ) -> Result<nixfied_runtime::state::UpgradeReport, nixfied_runtime::RuntimeError> {
        let placement = self.placement(run_id);
        materialize_registry_root(&placement)?;
        let mut registry = open_registry(&placement, &self.model);
        prepare_slot_state(&placement, identity, &mut registry, 1000)
    }

    fn registry(&self) -> Registry {
        let placement = self.placement("control");
        materialize_registry_root(&placement).expect("registry root should materialize");
        open_registry(&placement, &self.model)
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

fn open_registry(placement: &HostPlacement, model: &Model) -> Registry {
    Registry::open_or_create(
        placement.registry_path(),
        &RegistryIdentity::default_slot(
            &model.project.project_id,
            &model.runtime_abi,
            &model.toolchain_id,
        ),
    )
    .expect("registry should open")
}

fn admission(model: &Model, source_root: &Path, hash: &str) -> Admission {
    Admission {
        model_path: PathBuf::from(format!("/nix/store/model-{hash}/model.json")),
        computed_model_hash: hash.to_string(),
        raw_len: 100,
        project_id: model.project.project_id.clone(),
        runtime_abi: model.runtime_abi.clone(),
        toolchain_id: model.toolchain_id.clone(),
        target_system: model.target.system.clone(),
        source: Some(admitted_source(source_root)),
        generator_json: serde_json::to_string(&model.generator).expect("generator serializes"),
        target_json: serde_json::to_string(&model.target).expect("target serializes"),
        execution_model: nixfied_runtime::execution::lower(model).expect("model should lower"),
        secrets: nixfied_runtime::admission::secrets::ResolvedSecrets::empty(),
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

fn fixture_model() -> Value {
    common::synthetic_model_default(23880, 23890)
}
