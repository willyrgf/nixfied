use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use nixfied_manifest::{Manifest, PersistencePolicy};
use nixfied_runtime::control::clean_reconciled_state;
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    CleanupMode, CleanupOutcome, MARKER_FILE_NAME, MARKER_VERSION, MarkerComparison, StateIdentity,
    StateMarker, clean_marked_state, commit_slot_marker, derive_host_placement,
    derive_host_placement_for_slot, evaluate_slot_marker, materialize_run_roots,
    prepare_slot_state,
};
use nixfied_runtime::{ErrorCode, RuntimeResult};
use serde_json::Value;

mod common;
use common::*;

#[test]
fn materializes_m0_roots_and_slot_marker() {
    let fixture = StateFixture::new();

    assert_eq!(
        fixture.layout.state_root,
        fixture.tmp.path.join("data/runtime-test/dev/0")
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
        fixture.layout.registry_dir.join("runs/run-1")
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
    assert_eq!(marker.compare(&fixture.identity), MarkerComparison::Match);
    assert_eq!(marker.marker_version, MARKER_VERSION);
    assert!(marker.data_generation.starts_with("gen-"));
    assert_eq!(marker.project_id, "runtime-test");
    assert_eq!(marker.environment, "dev");
    assert_eq!(marker.slot, 0);
    assert_eq!(marker.persistence, PersistencePolicy::RunScoped);
}

#[test]
fn selects_explicit_slot_placement() {
    let tmp = TempDir::new();
    let mut value = fixture_manifest();
    add_slot_one(&mut value, 23180, 23190);
    let manifest: Manifest = serde_json::from_value(value).expect("manifest should parse");
    let selected = select_slot(&manifest, Some(1)).expect("slot 1 should select");

    let layout = derive_host_placement_for_slot(&manifest, &selected, "run-2", &tmp.path)
        .expect("slot placement should derive");

    assert_eq!(selected.slot, 1);
    assert_eq!(layout.state_root, tmp.path.join("data/runtime-test/dev/1"));
    assert_eq!(
        layout.run_dir,
        tmp.path.join("registry/runtime-test/dev/1/runs/run-2")
    );
    assert_eq!(
        layout.logs_dir,
        tmp.path.join("registry/runtime-test/dev/1/runs/run-2/logs")
    );
    assert_eq!(
        layout.artifacts_dir,
        tmp.path
            .join("registry/runtime-test/dev/1/runs/run-2/artifacts")
    );
    assert_eq!(
        layout.summary_path,
        tmp.path
            .join("registry/runtime-test/dev/1/runs/run-2/summary.json")
    );
    assert_eq!(
        layout.registry_path(),
        tmp.path
            .join("registry/runtime-test/dev/1/registry.sqlite3")
    );
}

#[test]
fn slot_one_marker_records_selected_identity() {
    let tmp = TempDir::new();
    let mut value = fixture_manifest();
    add_slot_one(&mut value, 23180, 23190);
    let manifest: Manifest = serde_json::from_value(value).expect("manifest should parse");
    let admission = fixture_admission(&manifest, &tmp.path);
    let selected = select_slot(&manifest, Some(1)).expect("slot 1 should select");
    let layout = derive_host_placement_for_slot(&manifest, &selected, "run-2", &tmp.path)
        .expect("slot placement should derive");
    materialize_run_roots(&layout).expect("roots should materialize");
    let identity = StateIdentity::from_selected_slot(admission.common(), &selected);

    let marker = commit_slot_marker(&layout, &identity).expect("marker should be written");

    assert_eq!(marker.environment, "dev");
    assert_eq!(marker.slot, 1);
    assert_eq!(marker.compare(&identity), MarkerComparison::Match);
}

#[test]
fn slot_out_of_range_is_refused() {
    let manifest = manifest();
    let error = select_slot(&manifest, Some(1)).expect_err("slot 1 is outside default M1 fixture");

    assert_eq!(error.code, ErrorCode::ManifestAdmission);
}

#[test]
fn placement_rejects_compound_or_template_identifiers_before_effects() {
    for invalid in [
        "",
        ".",
        "..",
        "../outside",
        "/absolute",
        "nested/project",
        "trailing/",
        "x//y",
        "x/.",
        "project-${environment}",
        "${unknown}",
        "nul\0byte",
    ] {
        for (project, run) in [(invalid, "run"), ("project", invalid)] {
            let tmp = TempDir::new();
            let mut manifest = manifest();
            manifest.project.project_id = project.into();
            let error = derive_host_placement(&manifest, run, &tmp.path).unwrap_err();
            assert_eq!(error.code, ErrorCode::StateUnwritable);
            assert_eq!(fs::read_dir(&tmp.path).unwrap().count(), 0);
        }
    }
}

#[test]
fn placement_preserves_unix_backslashes_as_component_bytes() {
    let tmp = TempDir::new();
    let mut manifest = manifest();
    manifest.project.project_id = r"project\name".into();
    let layout = derive_host_placement(&manifest, r"run\name", &tmp.path).unwrap();
    assert_eq!(layout.state_root, tmp.path.join(r"data/project\name/dev/0"));
    assert_eq!(
        layout.run_dir,
        tmp.path.join(r"registry/project\name/dev/0/runs/run\name")
    );
}

#[cfg(unix)]
#[test]
fn materialization_refuses_symlinked_roots_and_nested_run_paths() {
    for target in ["registry", "state", "run", "logs", "artifacts"] {
        let tmp = TempDir::new();
        let layout = derive_host_placement(&manifest(), "run-1", &tmp.path).unwrap();
        let path = match target {
            "registry" => &layout.registry_dir,
            "state" => &layout.state_root,
            "run" => &layout.run_dir,
            "logs" => &layout.logs_dir,
            "artifacts" => &layout.artifacts_dir,
            _ => unreachable!(),
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let outside = tmp.path.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"untouched").unwrap();
        std::os::unix::fs::symlink(&outside, path).unwrap();

        let error = materialize_run_roots(&layout).unwrap_err();
        assert_eq!(error.code, ErrorCode::StateUnwritable, "{target}");
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"untouched");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    }
}

#[test]
fn marker_evaluate_refuses_foreign_ownership() {
    let fixture = StateFixture::new();
    let mut other = fixture.identity.clone();
    other.project_id = "other-project".to_string();

    let error = evaluate_slot_marker(&fixture.layout, &other)
        .expect_err("ownership mismatch must be refused");

    assert_eq!(error.code, ErrorCode::StateUnowned);
    let marker: StateMarker = serde_json::from_slice(
        &fs::read(fixture.layout.state_root.join(MARKER_FILE_NAME))
            .expect("marker should still be readable"),
    )
    .expect("marker should parse");
    assert_eq!(marker.project_id, "runtime-test");
}

#[test]
fn clean_accepts_old_provenance_marker() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    // The marker on disk was written by an older build of the same manifest:
    // cleanup is gated on ownership, so the current build may still clean it.
    let mut old = fixture.identity.clone();
    old.computed_manifest_hash = "older-manifest-hash".to_string();
    old.manifest_path = PathBuf::from("/nix/store/older-manifest/manifest.json");
    commit_slot_marker(&fixture.layout, &old).expect("old-provenance marker should be written");

    let outcome = fixture
        .clean(&mut registry, CleanupMode::Standard)
        .expect("old-provenance marker should be cleanable by the slot owner");

    assert!(deleted_id(&outcome).starts_with("cleanup-"));
    assert!(!fixture.layout.state_root.exists());
}

#[test]
fn cleanup_refuses_unmarked_roots() {
    for mode in [CleanupMode::Standard, CleanupMode::Purge] {
        let fixture = StateFixture::new();
        let mut registry = fixture.registry();
        let target = &fixture.layout.state_root;
        fs::remove_file(target.join(MARKER_FILE_NAME)).expect("remove selected root marker");
        fs::write(target.join("data"), b"unowned").unwrap();

        let error = fixture
            .clean(&mut registry, mode)
            .expect_err("unmarked target should be refused");

        assert_eq!(error.code, ErrorCode::StateUnowned);
        assert_eq!(fs::read(target.join("data")).unwrap(), b"unowned");
        assert_eq!(cleanup_rows(&registry), 0);
    }
}

#[test]
fn cleanup_refuses_marker_mismatch() {
    for mode in [CleanupMode::Standard, CleanupMode::Purge] {
        let fixture = StateFixture::new();
        let mut registry = fixture.registry();
        let mut marker = StateMarker::slot(&fixture.identity).unwrap();
        marker.project_id = "other-project".to_string();
        write_marker(&fixture, &marker);

        let error = fixture
            .clean(&mut registry, mode)
            .expect_err("marker mismatch should be refused");

        assert_eq!(error.code, ErrorCode::StateUnowned);
        assert!(fixture.layout.state_root.exists());
        assert_eq!(cleanup_rows(&registry), 0);
    }
}

#[test]
fn persistence_alone_authorizes_deletion_and_purge_overrides_only_retention() {
    for persistence in [PersistencePolicy::RunScoped, PersistencePolicy::Persistent] {
        let fixture = StateFixture::new();
        let mut registry = fixture.registry();
        let mut identity = fixture.identity.clone();
        identity.persistence = persistence.clone();
        let marker = StateMarker::slot(&identity).unwrap();
        write_marker(&fixture, &marker);

        let standard = clean_marked_state(
            &fixture.layout.state_base,
            &identity,
            &mut registry,
            CleanupMode::Standard,
        );
        if persistence == PersistencePolicy::Persistent {
            assert_eq!(standard.unwrap_err().code, ErrorCode::CleanupRefused);
            assert!(fixture.layout.state_root.exists());
            assert_eq!(cleanup_rows(&registry), 0);
            let outcome = clean_marked_state(
                &fixture.layout.state_base,
                &identity,
                &mut registry,
                CleanupMode::Purge,
            )
            .expect("persistent state should purge");
            let id = deleted_id(&outcome);
            let row: (String, i64, String) = registry
                .connection()
                .query_row(
                    "SELECT status, purge, data_generation FROM cleanups WHERE cleanup_id = ?1",
                    [&id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(
                row,
                ("completed".to_string(), 1, marker.data_generation.clone())
            );
            let payloads = cleanup_event_payloads(&registry, &id);
            assert_eq!(payloads.len(), 2);
            assert!(payloads.iter().all(|payload| payload["purge"] == true));
        } else {
            let outcome = standard.expect("run-scoped state should clean");
            deleted_id(&outcome);
        }
        assert!(!fixture.layout.state_root.exists());
    }
}

#[test]
fn purge_still_refuses_active_registry_refs() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let mut persistent = fixture.identity.clone();
    persistent.persistence = PersistencePolicy::Persistent;
    write_marker(&fixture, &StateMarker::slot(&persistent).unwrap());
    registry
        .connection_mut()
        .execute_batch(
            "INSERT INTO processes (
               process_key, environment, slot, pid, pgid, start_identity, command_json,
               run_id, status, role
             , source_label, presentation, stdout_path, stderr_path) VALUES ('process-1', 'dev', 0, 1, 1, 'start', '{}', 'run-1', 'running', 'task', 'fixture', 'hidden', 'logs/' || hex(randomblob(8)), 'logs/' || hex(randomblob(8)))",
        )
        .expect("active process should be inserted");

    let error = clean_marked_state(
        &fixture.layout.state_base,
        &persistent,
        &mut registry,
        CleanupMode::Purge,
    )
    .expect_err("purge must still refuse active refs");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

#[cfg(unix)]
#[test]
fn cleanup_refuses_a_symlinked_root_and_unlinks_tree_symlinks_without_following() {
    for mode in [CleanupMode::Standard, CleanupMode::Purge] {
        let fixture = StateFixture::new();
        let mut registry = fixture.registry();
        let real = fixture.tmp.path.join("real-root");
        fs::rename(&fixture.layout.state_root, &real).unwrap();
        std::os::unix::fs::symlink(&real, &fixture.layout.state_root).unwrap();
        let error = fixture.clean(&mut registry, mode).unwrap_err();
        assert_eq!(error.code, ErrorCode::StateUnowned);
        assert!(real.join(MARKER_FILE_NAME).is_file());
        fs::remove_file(&fixture.layout.state_root).unwrap();
        fs::rename(&real, &fixture.layout.state_root).unwrap();

        let outside = fixture.tmp.path.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("file"), b"outside").unwrap();
        let nested = fixture.layout.state_root.join("nested");
        fs::create_dir(&nested).unwrap();
        std::os::unix::fs::symlink(&outside, nested.join("dir-link")).unwrap();
        std::os::unix::fs::symlink(outside.join("file"), nested.join("file-link")).unwrap();
        fs::hard_link(outside.join("file"), nested.join("hard-link")).unwrap();
        fixture
            .clean(&mut registry, mode)
            .expect("cleanup should unlink tree symlinks and hard links");
        assert!(!fixture.layout.state_root.exists());
        assert_eq!(fs::read(outside.join("file")).unwrap(), b"outside");
    }
}

#[test]
fn cleanup_refuses_active_registry_refs() {
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO processes (
           process_key, environment, slot, pid, pgid, start_identity, command_json,
           run_id, status, role
         , source_label, presentation, stdout_path, stderr_path) VALUES ('process-1', 'dev', 0, 1, 1, 'start', '{}', 'run-1', 'running', 'task', 'fixture', 'hidden', 'logs/' || hex(randomblob(8)), 'logs/' || hex(randomblob(8)))",
    );
    assert_cleanup_refused_with_active_ref(
        "INSERT INTO ports (
           endpoint_key, environment, slot, service_instance_id, address, port,
           status, owner_process_key
         ) VALUES ('endpoint-1', 'dev', 0, 'service-1', '127.0.0.1', 23080, 'reserved', 'process-1')",
    );
}

#[test]
fn cleanup_deletes_matching_inactive_state_and_reports_later_absence() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let child_written = fixture
        .layout
        .state_root
        .join("child-owned/tool-artifacts/result.bin");
    fs::create_dir_all(child_written.parent().expect("child path has a parent"))
        .expect("child-owned directory should be created");
    fs::write(&child_written, b"opaque child data")
        .expect("child-owned artifact should be written");

    let outcome = fixture
        .clean(&mut registry, CleanupMode::Standard)
        .expect("inactive marked state should be deleted");
    let id = deleted_id(&outcome);
    assert!(
        !fixture.layout.state_root.exists(),
        "whole-slot clean removes opaque child-written contents without selectively interpreting them"
    );
    let row: (String, String) = registry
        .connection()
        .query_row(
            "SELECT status, target FROM cleanups WHERE cleanup_id = ?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cleanup status should be recorded");
    assert_eq!(
        row,
        (
            "completed".to_string(),
            "data/runtime-test/dev/0".to_string()
        )
    );
    assert_eq!(
        cleanup_event_types(&registry, &id),
        ["cleanup.intent", "cleanup.completed"]
    );

    // The registry lives outside the deleted tree. A later invocation observes
    // absence without attributing it to an old operation or inventing a new one.
    drop(registry);
    let mut reopened = fixture.registry();
    let repeated = fixture
        .clean(&mut reopened, CleanupMode::Standard)
        .expect("repeated cleanup should observe absence");
    assert_eq!(
        repeated,
        CleanupOutcome::Absent {
            target_path: fixture.layout.state_root.canonicalize_parent()
        }
    );
    assert_eq!(cleanup_rows(&reopened), 1);
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
              run_id, environment, slot, execution_outcome, manifest_path, computed_manifest_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            , owner_identity, diagnostic_path) VALUES (
              'run-stale', 'dev', 0, NULL, '/nix/store/test-manifest/manifest.json',
              'computed-hash', 'nixfied-runtime-abi:1',
              'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            , '{}', 'diagnostics.log');
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status, service_name, role
            , source_label, presentation, stdout_path, stderr_path) VALUES (
              'process-stale', 'dev', 0, 999999, 999999,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-stale', 'service-stale', 'running', 'synthetic', 'service'
            , 'fixture', 'hidden', 'logs/' || hex(randomblob(8)), 'logs/' || hex(randomblob(8)));
            INSERT INTO ports (
              endpoint_key, environment, slot, service_instance_id, address, port,
              status, owner_process_key
            ) VALUES (
              'service-stale:endpoint-stale', 'dev', 0, 'service-stale', '127.0.0.1', 23190,
              'active', 'process-stale'
            );
            ",
        )
        .expect("stale refs should be inserted");

    let outcome = clean_reconciled_state(
        &mut registry,
        &fixture.layout.state_base,
        &fixture.identity,
        CleanupMode::Standard,
    )
    .expect("stale refs should reconcile before cleanup");

    deleted_id(&outcome);
    assert!(!fixture.layout.state_root.exists());
    let statuses: (String, String) = registry
        .connection()
        .query_row(
            "SELECT p.status, e.status FROM processes p, ports e
             WHERE p.process_key = 'process-stale' AND e.endpoint_key = 'service-stale:endpoint-stale'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(statuses, ("stale".to_string(), "stale".to_string()));
}

#[test]
fn pending_cleanup_resumes_the_same_operation_across_every_marker_last_observation() {
    enum Interrupted {
        RootAbsent,
        PartialPayload,
        MarkerlessEmpty,
    }
    for case in [
        Interrupted::RootAbsent,
        Interrupted::PartialPayload,
        Interrupted::MarkerlessEmpty,
    ] {
        let fixture = StateFixture::new();
        let mut registry = fixture.registry();
        fs::create_dir_all(fixture.layout.state_root.join("payload/deep")).unwrap();
        fs::write(fixture.layout.state_root.join("payload/deep/file"), b"x").unwrap();
        let marker = read_marker(&fixture);
        insert_pending(&registry, &fixture, "cleanup-interrupted", &marker, false);
        match case {
            Interrupted::RootAbsent => fs::remove_dir_all(&fixture.layout.state_root).unwrap(),
            Interrupted::PartialPayload => {
                fs::remove_file(fixture.layout.state_root.join("payload/deep/file")).unwrap()
            }
            Interrupted::MarkerlessEmpty => {
                fs::remove_dir_all(fixture.layout.state_root.join("payload")).unwrap();
                fs::remove_file(fixture.layout.state_root.join(MARKER_FILE_NAME)).unwrap();
            }
        }
        // A requested purge neither extends nor replaces the committed operation.
        let outcome = fixture.clean(&mut registry, CleanupMode::Purge).unwrap();
        assert_eq!(deleted_id(&outcome), "cleanup-interrupted");
        assert!(!fixture.layout.state_root.exists());
        assert_eq!(cleanup_rows(&registry), 1);
        assert_eq!(
            cleanup_event_types(&registry, "cleanup-interrupted"),
            ["cleanup.completed"]
        );
    }
}

#[test]
fn pending_cleanup_refuses_inconsistent_or_replaced_trees_without_deleting() {
    // Markerless but nonempty contradicts ordered marker-last deletion.
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let marker = read_marker(&fixture);
    insert_pending(&registry, &fixture, "cleanup-pending", &marker, false);
    fs::remove_file(fixture.layout.state_root.join(MARKER_FILE_NAME)).unwrap();
    fs::write(fixture.layout.state_root.join("survivor"), b"kept").unwrap();
    let error = fixture
        .clean(&mut registry, CleanupMode::Standard)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert_eq!(
        fs::read(fixture.layout.state_root.join("survivor")).unwrap(),
        b"kept"
    );

    // A replacement tree with a new generation is never deleted by the old intent.
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let old = read_marker(&fixture);
    insert_pending(&registry, &fixture, "cleanup-pending", &old, false);
    fs::remove_dir_all(&fixture.layout.state_root).unwrap();
    fs::create_dir(&fixture.layout.state_root).unwrap();
    commit_slot_marker(&fixture.layout, &fixture.identity).unwrap();
    fs::write(fixture.layout.state_root.join("new"), b"new").unwrap();
    let error = fixture
        .clean(&mut registry, CleanupMode::Purge)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_eq!(
        fs::read(fixture.layout.state_root.join("new")).unwrap(),
        b"new"
    );
    let status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM cleanups WHERE cleanup_id = 'cleanup-pending'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "pending");
}

#[test]
fn pending_state_preparation_settles_deletion_before_a_new_generation() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let old = read_marker(&fixture);
    insert_pending(&registry, &fixture, "cleanup-before-run", &old, false);
    fs::write(fixture.layout.state_root.join("stale"), b"old").unwrap();
    prepare_slot_state(&fixture.layout, &fixture.identity, &mut registry)
        .expect("pending run-scoped deletion should settle before preparation");
    let fresh = read_marker(&fixture);
    assert_ne!(fresh.data_generation, old.data_generation);
    assert!(!fixture.layout.state_root.join("stale").exists());
    assert_eq!(
        cleanup_event_types(&registry, "cleanup-before-run"),
        ["cleanup.completed"]
    );
}

#[test]
fn deleted_generation_reappearing_is_contradictory_history() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let marker = read_marker(&fixture);
    let copy = fs::read(fixture.layout.state_root.join(MARKER_FILE_NAME)).unwrap();
    fixture.clean(&mut registry, CleanupMode::Standard).unwrap();
    fs::create_dir(&fixture.layout.state_root).unwrap();
    fs::write(fixture.layout.state_root.join(MARKER_FILE_NAME), &copy).unwrap();
    let error = fixture
        .clean(&mut registry, CleanupMode::Purge)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StateUnowned);
    assert_eq!(read_marker(&fixture), marker);
    assert_eq!(cleanup_rows(&registry), 1);
}

#[test]
fn incoherent_pending_authorization_rejects_before_deletion() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let mut persistent = fixture.identity.clone();
    persistent.persistence = PersistencePolicy::Persistent;
    let marker = StateMarker::slot(&persistent).unwrap();
    write_marker(&fixture, &marker);
    // A standard (non-purge) intent cannot authorize persistent data.
    insert_pending(&registry, &fixture, "cleanup-incoherent", &marker, false);
    let error = clean_marked_state(
        &fixture.layout.state_base,
        &persistent,
        &mut registry,
        CleanupMode::Purge,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert!(fixture.layout.state_root.join(MARKER_FILE_NAME).is_file());
}

#[test]
fn failed_deletion_step_keeps_the_intent_pending_and_retry_reuses_it() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let locked = fixture.layout.state_root.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("inner"), b"x").unwrap();
    let restore = PermissionRestore {
        path: locked.clone(),
        mode: 0o700,
    };
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();

    let error = fixture
        .clean(&mut registry, CleanupMode::Standard)
        .expect_err("delete failure should refuse cleanup");
    assert_eq!(error.code, ErrorCode::CleanupRefused);
    let id: String = registry
        .connection()
        .query_row(
            "SELECT cleanup_id FROM cleanups WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        cleanup_event_types(&registry, &id),
        ["cleanup.intent", "cleanup.attempt-failed"]
    );
    assert!(
        fixture.layout.state_root.join(MARKER_FILE_NAME).is_file(),
        "the marker is removed only after every payload entry"
    );

    drop(restore);
    let outcome = fixture.clean(&mut registry, CleanupMode::Standard).unwrap();
    assert_eq!(deleted_id(&outcome), id);
    assert_eq!(cleanup_rows(&registry), 1);
    assert!(!fixture.layout.state_root.exists());
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
              run_id, environment, slot, execution_outcome, manifest_path, computed_manifest_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json,
              summary_path
            , owner_identity, diagnostic_path) VALUES (
              'run-stale-port', 'dev', 0, NULL, '/nix/store/test-manifest/manifest.json',
              'computed-hash', 'nixfied-runtime-abi:1',
              'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            , '{}', 'diagnostics.log');
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status, service_name, role
            , source_label, presentation, stdout_path, stderr_path) VALUES (
              'process-stale-port', 'dev', 0, 999998, 999998,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-stale-port', 'service-stale-port', 'stopped', 'synthetic', 'service'
            , 'fixture', 'hidden', 'logs/' || hex(randomblob(8)), 'logs/' || hex(randomblob(8)));
            INSERT INTO ports (
              endpoint_key, environment, slot, service_instance_id, address, port,
              status, owner_process_key
            ) VALUES (
              'service-stale-port:endpoint', 'dev', 0, 'service-stale-port', '127.0.0.1', 23191,
              'active', 'process-stale-port'
            );
            ",
        )
        .expect("stale port refs should be inserted");

    let outcome = clean_reconciled_state(
        &mut registry,
        &fixture.layout.state_base,
        &fixture.identity,
        CleanupMode::Standard,
    )
    .expect("stale port should reconcile before cleanup");
    let port_status: String = registry
        .connection()
        .query_row(
            "SELECT status FROM ports WHERE endpoint_key = 'service-stale-port:endpoint'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");
    // A terminal row without proven containment is an explicit obligation;
    // recovery settles it (and its endpoint evidence) only after death proof.
    let (ownership, settled_events): (String, i64) = registry
        .connection()
        .query_row(
            "SELECT ownership, (SELECT count(*) FROM events WHERE event_type = 'process.ownership-settled')
             FROM processes WHERE process_key = 'process-stale-port'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("ownership should query");

    deleted_id(&outcome);
    assert_eq!(port_status, "stale");
    assert_eq!((ownership.as_str(), settled_events), ("settled", 1));
}

fn assert_cleanup_refused_with_active_ref(sql: &str) {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(sql)
        .expect("active ref should be inserted");

    let error = fixture
        .clean(&mut registry, CleanupMode::Standard)
        .expect_err("active registry refs should refuse cleanup");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.exists());
}

fn deleted_id(outcome: &CleanupOutcome) -> String {
    match outcome {
        CleanupOutcome::Deleted { cleanup_id, .. } => cleanup_id.clone(),
        CleanupOutcome::Absent { .. } => panic!("expected a deletion, got {outcome:?}"),
    }
}

fn cleanup_rows(registry: &Registry) -> i64 {
    registry
        .connection()
        .query_row("SELECT count(*) FROM cleanups", [], |row| row.get(0))
        .unwrap()
}

fn read_marker(fixture: &StateFixture) -> StateMarker {
    serde_json::from_slice(&fs::read(fixture.layout.state_root.join(MARKER_FILE_NAME)).unwrap())
        .unwrap()
}

fn write_marker(fixture: &StateFixture, marker: &StateMarker) {
    fs::write(
        fixture.layout.state_root.join(MARKER_FILE_NAME),
        serde_json::to_vec_pretty(marker).expect("marker JSON"),
    )
    .expect("marker should be replaced");
}

/// Simulate an owner that died after committing intent and before completion.
fn insert_pending(
    registry: &Registry,
    fixture: &StateFixture,
    cleanup_id: &str,
    marker: &StateMarker,
    purge: bool,
) {
    use std::os::unix::fs::MetadataExt;
    let root = fs::metadata(&fixture.layout.state_root).unwrap();
    registry
        .connection()
        .execute(
            "INSERT INTO cleanups (
               cleanup_id, target, data_generation, marker_json, purge, root_identity, status
             ) VALUES (?1, 'data/runtime-test/dev/0', ?2, ?3, ?4, ?5, 'pending')",
            rusqlite::params![
                cleanup_id,
                marker.data_generation,
                serde_json::to_string(marker).unwrap(),
                i64::from(purge),
                format!("{}:{}", root.dev(), root.ino()),
            ],
        )
        .unwrap();
}

fn cleanup_event_types(registry: &Registry, cleanup_id: &str) -> Vec<String> {
    let mut statement = registry
        .connection()
        .prepare(
            "SELECT event_type FROM events
             WHERE event_type LIKE 'cleanup.%' AND payload_json LIKE ?1 ORDER BY seq",
        )
        .unwrap();
    statement
        .query_map([format!("%\"{cleanup_id}\"%")], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn cleanup_event_payloads(registry: &Registry, cleanup_id: &str) -> Vec<Value> {
    let mut statement = registry
        .connection()
        .prepare(
            "
            SELECT payload_json FROM events
            WHERE event_type LIKE 'cleanup.%' AND payload_json LIKE ?1
            ORDER BY seq
            ",
        )
        .expect("cleanup payload statement should prepare");
    statement
        .query_map([format!("%{cleanup_id}%")], |row| {
            let payload_json: String = row.get(0)?;
            Ok(serde_json::from_str::<Value>(&payload_json).expect("payload should parse"))
        })
        .expect("cleanup payloads should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("cleanup payloads should collect")
}

trait CanonicalParent {
    fn canonicalize_parent(&self) -> PathBuf;
}

impl CanonicalParent for PathBuf {
    /// The absent target reported beneath the canonical state base.
    fn canonicalize_parent(&self) -> PathBuf {
        let mut path = self.clone();
        let mut missing = Vec::new();
        while !path.exists() {
            missing.push(path.file_name().unwrap().to_owned());
            path.pop();
        }
        let mut canonical = path.canonicalize().unwrap();
        canonical.extend(missing.into_iter().rev());
        canonical
    }
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
        let manifest = manifest();
        let admission = fixture_admission(&manifest, &tmp.path);
        let layout =
            derive_host_placement(&manifest, "run-1", &tmp.path).expect("layout should derive");
        materialize_run_roots(&layout).expect("roots should materialize");
        let identity = StateIdentity::from_admission(admission.common());
        commit_slot_marker(&layout, &identity).expect("marker should be written");
        Self {
            tmp,
            layout,
            identity,
        }
    }

    fn clean(&self, registry: &mut Registry, mode: CleanupMode) -> RuntimeResult<CleanupOutcome> {
        clean_marked_state(&self.layout.state_base, &self.identity, registry, mode)
    }

    fn registry(&self) -> Registry {
        Registry::open_or_create(
            registry_guard(&self.layout),
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

fn manifest() -> Manifest {
    serde_json::from_value(fixture_manifest()).expect("fixture manifest should parse")
}

fn fixture_manifest() -> Value {
    common::test_child_manifest(23080, 23090)
}

#[test]
fn namespace_names_are_ordinary_projects_with_disjoint_data_and_evidence() {
    let tmp = TempDir::new();
    let layouts: Vec<_> = ["registry", "data", "dev"]
        .into_iter()
        .map(|project| {
            let mut manifest = manifest();
            manifest.project.project_id = project.into();
            let layout = derive_host_placement(&manifest, "session", &tmp.path).unwrap();
            materialize_run_roots(&layout).unwrap();
            assert_eq!(
                layout.state_root,
                tmp.path.join(format!("data/{project}/dev/0"))
            );
            assert_eq!(
                layout.run_dir,
                tmp.path
                    .join(format!("registry/{project}/dev/0/runs/session"))
            );
            layout
        })
        .collect();
    for application in &layouts {
        for evidence in &layouts {
            assert!(!application.state_root.starts_with(&evidence.registry_dir));
            assert!(!evidence.registry_dir.starts_with(&application.state_root));
        }
    }
}

#[test]
fn application_cleanup_preserves_run_evidence_and_registry() {
    for mode in [CleanupMode::Standard, CleanupMode::Purge] {
        let fixture = StateFixture::new();
        let mut registry = fixture.registry();
        let log = fixture.layout.logs_dir.join("stdout.log");
        let artifact = fixture.layout.artifacts_dir.join("result.bin");
        for path in [&log, &artifact, &fixture.layout.summary_path] {
            fs::write(path, b"retained evidence").unwrap();
        }
        fs::write(fixture.layout.state_root.join("application.bin"), b"data").unwrap();
        fixture.clean(&mut registry, mode).unwrap();
        assert!(!fixture.layout.state_root.exists());
        for path in [&log, &artifact, &fixture.layout.summary_path] {
            assert_eq!(fs::read(path).unwrap(), b"retained evidence");
        }
        assert!(fixture.layout.registry_path().is_file());
        assert_eq!(
            registry
                .connection()
                .query_row(
                    "SELECT count(*) FROM cleanups WHERE status = 'completed'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }
}

#[test]
fn cleanup_rejects_application_ancestry_redirected_into_evidence() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    let marker = fs::read(fixture.layout.state_root.join(MARKER_FILE_NAME)).unwrap();
    fs::write(fixture.layout.registry_dir.join(MARKER_FILE_NAME), &marker).unwrap();
    fs::remove_dir_all(fixture.tmp.path.join("data")).unwrap();
    std::os::unix::fs::symlink(
        fixture.tmp.path.join("registry"),
        fixture.tmp.path.join("data"),
    )
    .unwrap();
    assert_eq!(
        fixture
            .clean(&mut registry, CleanupMode::Purge)
            .unwrap_err()
            .code,
        ErrorCode::StateUnowned
    );
    assert_eq!(
        fs::read(fixture.layout.registry_dir.join(MARKER_FILE_NAME)).unwrap(),
        marker
    );
    assert!(fixture.layout.registry_path().is_file());
    assert_eq!(cleanup_rows(&registry), 0);
}

#[test]
fn endpoint_less_unresolved_process_blocks_deletion_until_recovery_proves_death() {
    let fixture = StateFixture::new();
    let mut registry = fixture.registry();
    registry
        .connection_mut()
        .execute_batch(
            "
            INSERT INTO runs (
              run_id, environment, slot, execution_outcome, manifest_path, computed_manifest_hash,
              runtime_abi, toolchain_id, generator_json, target_json, source_json, summary_path
            , owner_identity, diagnostic_path) VALUES (
              'run-escaped', 'dev', 0, 'failed', '/nix/store/test-manifest/manifest.json',
              'computed-hash', 'nixfied-runtime-abi:1', 'nixfied-toolchain:1', '{}', '{}', '[]', NULL
            , '{}', 'diagnostics.log');
            INSERT INTO processes (
              process_key, environment, slot, pid, pgid, start_identity, command_json,
              run_id, service_instance_id, status, ownership, service_name, role
            , source_label, presentation, stdout_path, stderr_path) VALUES (
              'process-escaped', 'dev', 0, 999997, 999997,
              '{\"platformStart\":\"missing\"}', '{}',
              'run-escaped', 'service-escaped', 'escaped', 'unresolved', 'synthetic', 'service'
            , 'fixture', 'hidden', 'logs/' || hex(randomblob(8)), 'logs/' || hex(randomblob(8)));
            ",
        )
        .unwrap();

    let refused = fixture
        .clean(&mut registry, CleanupMode::Purge)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::CleanupRefused);
    assert!(fixture.layout.state_root.join(MARKER_FILE_NAME).is_file());

    let outcome = clean_reconciled_state(
        &mut registry,
        &fixture.layout.state_base,
        &fixture.identity,
        CleanupMode::Standard,
    )
    .expect("recovery settles an obligation once death is proven");
    deleted_id(&outcome);
    let ownership: String = registry
        .connection()
        .query_row(
            "SELECT ownership FROM processes WHERE process_key = 'process-escaped'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ownership, "settled");
}
