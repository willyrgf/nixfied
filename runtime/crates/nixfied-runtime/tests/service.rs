use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use nixfied_manifest::Manifest;
use nixfied_runtime::cancellation::CancellationToken;
use nixfied_runtime::output::SourcePresentation;
use nixfied_runtime::redaction::{REDACTION_TOKEN, Redactor};
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::service::{
    PrepareRunner, ReadinessFailure, ReadyService, RunContext, StartingService, TaskExecution,
    TaskExecutionError, run_dependent_task_cancellable, run_slot_clean,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{CleanupMode, StateIdentity, clean_marked_state, commit_slot_marker};
use nixfied_runtime::{ErrorCode, RunAdmission, RuntimeError};
use serde_json::{Value, json};

use nixfied_runtime::control::stop_recorded_processes;

mod common;
use common::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn service_registration_event_failure_prevents_workload_effects() {
    let root = TempDir::new();
    let marker = root.path.join("must-not-execute");
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(
        test_child().to_str().unwrap(),
        &["output", "occurrence", marker.to_str().unwrap(), "0"],
        port,
    );
    fixture.registry.connection().execute_batch(
        "CREATE TRIGGER reject_service_registration BEFORE INSERT ON events WHEN NEW.event_type='service.starting' BEGIN SELECT RAISE(ABORT, 'injected'); END"
    ).unwrap();
    let error = expect_service_start_failure(
        fixture.start("run-registration-denied", port),
        &mut fixture.registry,
        "registration failure must keep the gate closed",
    );
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert!(!marker.exists());
    assert_eq!(
        fixture.query::<i64>("SELECT count(*) FROM processes", []),
        0
    );
    assert_eq!(fixture.query::<i64>("SELECT count(*) FROM ports", []), 0);
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM events WHERE event_type='service.starting'",
            []
        ),
        0
    );
}

#[test]
fn probe_registration_event_failure_prevents_workload_effects() {
    let port = available_port_window(1);
    let value = exec_probe_fixture_value(
        test_child().to_str().unwrap(),
        LISTEN_HOLD,
        port,
        json!(["marker", "${stateDir}/must-not-execute"]),
        1,
    );
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture
        .start("run-probe-registration-denied", port)
        .unwrap();
    fixture.registry.connection().execute_batch(
        "CREATE TRIGGER reject_probe_registration BEFORE INSERT ON events WHEN NEW.event_type='probe.running' BEGIN SELECT RAISE(ABORT, 'injected'); END"
    ).unwrap();
    let (service, error) = fixture
        .ready(service)
        .expect_err("probe registration must reject before execution")
        .into_parts();
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert!(
        !fixture
            .placement
            .state_root()
            .join("must-not-execute")
            .exists()
    );
    assert_eq!(
        fixture.query::<i64>("SELECT count(*) FROM processes WHERE role='probe'", []),
        0
    );
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM events WHERE event_type='probe.running'",
            []
        ),
        0
    );
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM processes WHERE role='service' AND status='running'",
            []
        ),
        1
    );
    service.finalize_failed_start(&mut fixture.registry, 1000, error);
}

#[test]
fn starts_foreground_service_in_owned_process_group_and_records_before_ready() {
    let port = available_port_window(1);
    let mut fixture = test_child_listener_fixture(port);
    let service = fixture
        .start("run-service", port)
        .expect("foreground service should start");

    assert_eq!(service.info().pid as i32, service.info().pgid);
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    let service_name: String = fixture.query(
        "SELECT service_name FROM processes WHERE process_key = ?1 AND role = 'service'",
        [&service.info().process_key],
    );
    let probe_ready_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
        [],
    );

    assert_eq!(process_status, "running");
    assert_eq!(service_name, "synthetic");
    assert_eq!(probe_ready_events, 0);
    assert!(
        fixture
            .placement
            .logs_dir()
            .join("service.synthetic.stdout.log")
            .exists()
    );
    assert!(
        fixture
            .placement
            .logs_dir()
            .join("service.synthetic.stderr.log")
            .exists()
    );
    let command_json: Value = fixture
        .registry
        .connection()
        .query_row(
            "SELECT command_json FROM processes WHERE process_key = ?1",
            [&service.info().process_key],
            |row| {
                let payload: String = row.get(0)?;
                Ok(serde_json::from_str(&payload).expect("command JSON should parse"))
            },
        )
        .expect("process command should exist");
    assert_eq!(
        command_json["cwd"],
        json!(fixture.admission.source().observed_root.to_string_lossy())
    );

    fixture
        .ready(service)
        .expect("service should become ready")
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
    let stopped_process_status: String = fixture.query(
        "SELECT status FROM processes WHERE run_id = 'run-service' AND role='service'",
        [],
    );
    let stored_service_rows: i64 = fixture.query(
        "SELECT count(*) FROM processes WHERE service_name = 'synthetic' AND role='service'",
        [],
    );
    assert_eq!(stopped_process_status, "stopped");
    assert_eq!(stored_service_rows, 1);
    // Shutdown records the actual signal mechanism, not a fabricated exec terminal.
    let stop_signal: String = fixture.query(
        "SELECT json_extract(payload_json, '$.signal') FROM events
             WHERE event_type = 'service.stop.signaled'",
        [],
    );
    assert_eq!(stop_signal, "TERM");
}

#[test]
fn service_start_rechecks_cwd_symlink_confinement_after_admission() {
    let tmp = TempDir::new();
    let work = tmp.path.join("work");
    fs::create_dir(&work).unwrap();
    let port = available_port_window(1);
    let mut value = synthetic_manifest(&test_sleep(), &["30"], port, port);
    value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["cwd"] = json!("work");
    let mut fixture = ServiceFixture::from_manifest_in_store(
        serde_json::from_value(value).unwrap(),
        Path::new("/nix/store"),
        tmp,
    );
    fs::remove_dir(&work).unwrap();
    let outside = TempDir::new();
    std::os::unix::fs::symlink(&outside.path, &work).unwrap();

    let error = fixture.start_refused(
        "run-cwd-escape",
        port,
        "escaped exec cwd should fail before process start",
    );

    assert_eq!(error.code, ErrorCode::SourceMismatch);
}

#[test]
fn readiness_probe_marks_ready_only_after_complete_endpoint_round() {
    let mut attempts = 0;
    let (port, mut fixture, mut service) = loop {
        attempts += 1;
        assert!(
            attempts <= 8,
            "no uncontended fixture port after eight tries"
        );
        let port = available_port_window(1);
        let mut fixture = test_child_listener_fixture(port);
        match fixture.start("run-ready", port) {
            Ok(service) => {
                let ready = fixture.ready(service).expect("service should become ready");
                break (port, fixture, ready);
            }
            Err(error) if error.code == ErrorCode::PortConflict => {
                // Another process can bind after the candidate check. Retry
                // only when an independent bind proves
                // the address is genuinely occupied at the failure boundary.
                match std::net::TcpListener::bind(("127.0.0.1", port)) {
                    Err(bind) if bind.kind() == std::io::ErrorKind::AddrInUse => continue,
                    result => panic!("false port conflict: {error}; raw bind: {result:?}"),
                }
            }
            Err(error) => panic!("service should start: {error}"),
        }
    };
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    let ports: i64 = fixture.query(
        "SELECT count(*) FROM ports WHERE owner_process_key = ?1",
        [&service.info().process_key],
    );
    let verified_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'endpoint.check-succeeded' AND process_key = ?1",
        [&service.info().process_key],
    );

    assert_eq!(process_status, "ready");
    assert_eq!(ports, 1);
    assert_eq!(verified_events, 1);
    service
        .check_health(
            &mut fixture.registry,
            &CancellationToken::new(),
            &mut || Ok(()),
        )
        .unwrap();
    let events: Vec<Value> = fixture.registry.connection().prepare(
        "SELECT payload_json FROM events WHERE event_type='endpoint.check-succeeded' ORDER BY seq"
    ).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap()).collect();
    assert_eq!(events.len(), 2);
    for (event, phase) in events.iter().zip(["ready", "health"]) {
        let keys: Vec<_> = event
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "address",
                "endpointId",
                "listener",
                "phase",
                "port",
                "probeProcessKey"
            ]
        );
        assert_eq!(event["phase"], phase);
        assert_eq!(event["endpointId"], "synthetic-tcp");
        assert_eq!(event["address"], "127.0.0.1");
        assert_eq!(event["port"], port);
        assert_eq!(event["listener"]["holderPid"], service.info().pid);
        assert!(
            !event["listener"]["holderStartIdentity"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        let socket = &event["listener"]["socketIdentity"];
        #[cfg(target_os = "linux")]
        {
            assert_eq!(socket["platform"], "linux");
            assert!(socket["inode"].as_u64().unwrap() > 0);
            let cookie = socket["cookie"].as_array().unwrap();
            assert_eq!(cookie.len(), 2);
            assert!(
                cookie
                    .iter()
                    .all(|word| word.as_u64().unwrap() <= u32::MAX as u64)
            );
            assert!(
                cookie
                    .iter()
                    .any(|word| word.as_u64() != Some(u32::MAX as u64))
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(socket["platform"], "macos");
            for field in ["socketHandle", "inpcbGeneration"] {
                let hex = socket[field].as_str().unwrap();
                assert_eq!(hex.len(), 18);
                assert!(hex.starts_with("0x"));
                assert!(
                    hex[2..]
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                );
            }
            assert_ne!(socket["socketHandle"], "0x0000000000000000");
        }
        let probes: i64 = fixture.query(
            "SELECT count(*) FROM processes WHERE process_key=?1 AND role='probe' AND status='succeeded' AND exit_code=0 AND ownership='settled' AND capture='complete'",
            [event["probeProcessKey"].as_str().unwrap()],
        );
        assert_eq!(probes, 1);
    }
    let health_service: String = fixture.query(
        "SELECT json_extract(payload_json,'$.serviceId') FROM events WHERE event_type='service.lifecycle.terminal' AND json_extract(payload_json,'$.class')='health'",
        [],
    );
    assert_eq!(health_service, "synthetic");
    assert_eq!(events[0]["listener"], events[1]["listener"]);
    assert_ne!(events[0]["probeProcessKey"], events[1]["probeProcessKey"]);
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM events WHERE event_type='service.probe-ready'",
            []
        ),
        1
    );
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM events WHERE event_type='port.owner-verified'",
            []
        ),
        0
    );
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
    // Endpoint evidence is immutable and settles with its owner.
    let settled: (i64, String) = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*), p.ownership FROM ports ep JOIN processes p
             ON p.process_key = ep.owner_process_key",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(settled, (1, "settled".into()));
}

#[test]
fn check_success_transaction_rolls_back_every_coupled_write() {
    for phase in ["ready", "health"] {
        for rejected in [
            "endpoint.check-succeeded",
            "service.probe-ready",
            "service.lifecycle.terminal",
        ] {
            if phase == "health" && rejected == "service.probe-ready" {
                continue;
            }
            let port = available_port_window(1);
            let mut fixture = test_child_listener_fixture(port);
            let started = fixture.start("round-rollback", port).unwrap();
            let mut ready = None;
            let mut starting = None;
            if phase == "health" {
                ready = Some(fixture.ready(started).unwrap_or_else(|failure| {
                    let stderr = fs::read_to_string(
                        fixture
                            .placement
                            .logs_dir()
                            .join("service.synthetic.stderr.log"),
                    )
                    .unwrap_or_default();
                    panic!("health fixture did not become ready: {failure:?}; stderr: {stderr}");
                }));
            } else {
                starting = Some(started);
            }
            let baseline: i64 = fixture.query("SELECT count(*) FROM events WHERE event_type IN ('endpoint.check-succeeded','service.probe-ready')", []);
            fixture.registry.connection().execute_batch(&format!(
                "CREATE TRIGGER reject_check BEFORE INSERT ON events WHEN NEW.event_type='{rejected}' AND (NEW.event_type!='service.lifecycle.terminal' OR json_extract(NEW.payload_json,'$.errorCode') IS NULL) BEGIN SELECT RAISE(ABORT,'injected'); END"
            )).unwrap();
            if let Some(started) = starting {
                let (service, error) = fixture.ready(started).unwrap_err().into_parts();
                assert_eq!(error.code, ErrorCode::RegistryCorrupt);
                assert_eq!(
                    fixture
                        .query::<String>("SELECT status FROM processes WHERE role='service'", []),
                    "running"
                );
                service.finalize_failed_start(&mut fixture.registry, 1000, error);
            } else {
                let mut service = ready.unwrap();
                let error = service
                    .check_health(
                        &mut fixture.registry,
                        &CancellationToken::new(),
                        &mut || Ok(()),
                    )
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::RegistryCorrupt);
                assert_eq!(
                    fixture
                        .query::<String>("SELECT status FROM processes WHERE role='service'", []),
                    "ready"
                );
                service.finalize_failed_start(&mut fixture.registry, 1000, error);
            }
            assert_eq!(fixture.query::<i64>("SELECT count(*) FROM events WHERE event_type IN ('endpoint.check-succeeded','service.probe-ready')", []), baseline);
            let successes: i64 = fixture.query("SELECT count(*) FROM events WHERE event_type='service.lifecycle.terminal' AND json_extract(payload_json,'$.class')=?1 AND json_extract(payload_json,'$.errorCode') IS NULL", [phase]);
            assert_eq!(successes, 0, "{phase}: {rejected}");
        }
    }
}

#[test]
fn ready_activation_rejects_unexpected_open_endpoint_rows_atomically() {
    let port = available_port_window(2);
    let mut fixture = test_child_listener_fixture(port);
    let service = fixture
        .start("run-ready-unexpected-open-row", port)
        .expect("service should start before ready activation");
    fixture
        .registry
        .connection()
        .execute(
            "
            INSERT INTO ports (owner_process_key, endpoint_id, address, port)
            SELECT owner_process_key, 'unexpected', address, port + 1
            FROM ports
            WHERE owner_process_key = ?1
            ",
            [&service.info().process_key],
        )
        .expect("test should inject an unexpected endpoint row");

    let (service, error) = fixture
        .ready(service)
        .expect_err("ready activation must reject the complete mismatched open set")
        .into_parts();

    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    let state: (String, i64) = fixture
        .registry
        .connection()
        .query_row(
            "
            SELECT
              (SELECT status FROM processes WHERE process_key = ?1),
              (SELECT count(*) FROM events WHERE event_type = 'endpoint.check-succeeded')
            ",
            [&service.info().process_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("failed ready transaction should remain inspectable");
    assert_eq!(state, ("running".into(), 0));
    let mut contender = test_child_listener_fixture(port);
    let conflict = contender.start_refused(
        "run-contender-before-settlement",
        port,
        "failed readiness must retain the startup guard until settlement",
    );
    assert_eq!(conflict.code, ErrorCode::PortConflict);
    assert_eq!(
        conflict.details["portConflict"]["reason"],
        json!("startup-lock-contended")
    );
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    contender
        .start_ready("run-contender-after-settlement", port)
        .stop(&mut contender.registry, 1000)
        .unwrap();
}

#[test]
fn refused_slot_clean_records_a_failed_clean_terminal() {
    let port = available_port_window(1);
    let mut fixture = test_child_listener_fixture(port);
    let selected = select_slot(fixture.admission.common().manifest(), None)
        .expect("default slot should select");
    let mut identity = StateIdentity::from_selected_slot(fixture.admission.common(), &selected);
    identity.persistence = nixfied_manifest::PersistencePolicy::Persistent;
    commit_slot_marker(&fixture.registry, &identity).expect("slot marker should be written");

    let error = run_slot_clean(
        fixture.admission.common(),
        &mut fixture.registry,
        &selected,
        CleanupMode::Standard,
    )
    .expect_err("persistent state requires purge");

    assert_eq!(error.code, ErrorCode::CleanupRefused);
    assert!(fixture.placement.state_root().exists());
    let clean: Vec<_> = lifecycle_events(&fixture.registry)
        .into_iter()
        .filter(|event| event.class == "clean")
        .collect();
    assert_eq!(clean.len(), 2);
    assert_eq!(clean[0].event_type, "service.lifecycle.started");
    assert_eq!(clean[1].event_type, "service.lifecycle.terminal");
    assert_ne!(clean[1].terminal_result.as_deref(), Some("cleaned"));
    assert_eq!(clean[1].error_code.as_deref(), Some("CLEANUP_REFUSED"));
}

#[test]
fn lifecycle_events_follow_declared_class_order_and_clean_terminal() {
    let port = available_port_window(1);
    let mut fixture = test_child_listener_fixture(port);
    let mut service = fixture.start_ready("run-lifecycle-order", port);
    service
        .check_health(
            &mut fixture.registry,
            &CancellationToken::new(),
            &mut || Ok(()),
        )
        .expect("service health should pass");
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");

    let selected = select_slot(fixture.admission.common().manifest(), None)
        .expect("default slot should select");
    let identity = StateIdentity::from_selected_slot(fixture.admission.common(), &selected);
    commit_slot_marker(&fixture.registry, &identity).expect("slot marker should be written");
    let cleanup = run_slot_clean(
        fixture.admission.common(),
        &mut fixture.registry,
        &selected,
        CleanupMode::Standard,
    )
    .expect("clean lifecycle should succeed");

    assert!(matches!(
        &cleanup,
        nixfied_runtime::state::CleanupOutcome::Deleted { deleted_path, .. }
            if deleted_path.ends_with("runtime-test/dev/0")
    ));
    assert!(!fixture.placement.state_root().exists());
    assert_eq!(
        lifecycle_events(&fixture.registry),
        vec![
            LifecycleEvent {
                event_type: "service.lifecycle.started".to_string(),
                class: "start".to_string(),
                terminal_result: None,
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.terminal".to_string(),
                class: "start".to_string(),
                terminal_result: Some("spawned".to_string()),
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.started".to_string(),
                class: "ready".to_string(),
                terminal_result: None,
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.terminal".to_string(),
                class: "ready".to_string(),
                terminal_result: Some("ready".to_string()),
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.started".to_string(),
                class: "health".to_string(),
                terminal_result: None,
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.terminal".to_string(),
                class: "health".to_string(),
                terminal_result: Some("healthy".to_string()),
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.started".to_string(),
                class: "stop".to_string(),
                terminal_result: None,
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.terminal".to_string(),
                class: "stop".to_string(),
                terminal_result: Some("stopped".to_string()),
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.started".to_string(),
                class: "clean".to_string(),
                terminal_result: None,
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.terminal".to_string(),
                class: "clean".to_string(),
                terminal_result: Some("cleaned".to_string()),
                error_code: None,
            },
        ]
    );
    let cleanup_events: Vec<String> = fixture
        .registry
        .connection()
        .prepare("SELECT event_type FROM events WHERE event_type LIKE 'cleanup.%' ORDER BY rowid")
        .expect("cleanup statement should prepare")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("cleanup events should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("cleanup events should collect");
    assert_eq!(cleanup_events, vec!["cleanup.intent", "cleanup.completed"]);
    let declaration_only: i64 = fixture.query(
        "SELECT count(*) FROM events
         WHERE event_type IN ('service.lifecycle.started', 'service.lifecycle.terminal')
           AND json_extract(payload_json, '$.class') = 'clean'
           AND json_extract(payload_json, '$.serviceId') = 'synthetic'
           AND run_id IS NULL AND process_key IS NULL",
        [],
    );
    assert_eq!(
        declaration_only, 2,
        "slot cleanup must not invent a service instance"
    );
}

#[test]
fn wildcard_listener_does_not_satisfy_loopback_endpoint_ownership() {
    let port = available_port_window(1);
    let mut fixture = test_child_wildcard_listener_fixture(port);
    let service = fixture
        .start("run-wildcard-listener", port)
        .expect("foreground service should start");

    let (service, error) = fixture
        .ready(service)
        .expect_err("wildcard listener must not satisfy declared loopback endpoint")
        .into_parts();

    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    let process_key = service.info().process_key.clone();
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    let ready_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
        [],
    );
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&process_key],
    );
    assert_eq!(ready_events, 0);
    assert_eq!(process_status, "failed");
}

#[test]
fn slot_one_service_uses_slot_placement_port_window() {
    let tmp = TempDir::new();
    let port = available_port_window(2);
    let slot_port = port + 1;
    let mut value = test_child_manifest(port, port);
    add_slot_one(&mut value, slot_port, slot_port);
    let manifest: Manifest = serde_json::from_value(value).expect("fixture manifest should parse");
    let admission = fixture_admission(&manifest, &tmp.path);
    let selected_slot = select_slot(&manifest, Some(1)).expect("slot 1 should select");
    let placement = slot_placement(&manifest, &selected_slot, "run-slot-1", &tmp.path)
        .expect("slot 1 layout should derive");
    let mut registry = open_slot_registry(&placement, &manifest, &selected_slot);
    registry.authority().claim_run_dir(&placement).unwrap();

    let service = start_fixture_service(
        &admission,
        &placement,
        &mut registry,
        "run-slot-1",
        &synthetic_endpoint(slot_port),
        &CancellationToken::new(),
        None,
    )
    .expect("slot 1 service should accept slot placement port");

    assert_eq!(
        service.selected_endpoint().expect("endpoint").port,
        slot_port
    );
    assert_eq!(
        placement.state_root(),
        tmp.path.join("data/runtime-test/dev/1")
    );
    service
        .ready(&mut registry, &CancellationToken::new(), &mut || Ok(()))
        .expect("slot 1 service should become ready")
        .stop(&mut registry, 1000)
        .expect("service should stop");
    // The registry itself binds the slot; its rows do not repeat it.
    let bound: (String, i64) = registry
        .connection()
        .query_row(
            "SELECT environment, slot FROM registry_meta WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(bound, ("dev".into(), 1));
}

#[test]
fn two_slots_keep_services_state_and_controls_isolated() {
    let tmp = TempDir::new();
    let (port0, port1) = (available_port_window(1), available_port_window(1));
    let mut value = test_child_manifest(port0, port0);
    add_slot_one(&mut value, port1, port1);
    let manifest: Manifest = serde_json::from_value(value).expect("fixture manifest should parse");
    let admission = fixture_admission(&manifest, &tmp.path);

    let mut slot0 = StartedSlot::start(&manifest, &admission, &tmp.path, 0, "run-slot-0", port0);
    let mut slot1 = StartedSlot::start(&manifest, &admission, &tmp.path, 1, "run-slot-1", port1);

    assert_ne!(slot0.placement.state_root(), slot1.placement.state_root());
    assert_ne!(
        slot0.placement.registry_path(),
        slot1.placement.registry_path()
    );
    assert_ne!(slot0.placement.run_dir(), slot1.placement.run_dir());
    assert_ne!(slot0.placement.logs_dir(), slot1.placement.logs_dir());
    assert_ne!(
        slot0.placement.artifacts_dir(),
        slot1.placement.artifacts_dir()
    );
    assert_ne!(
        slot0.placement.summary_path(),
        slot1.placement.summary_path()
    );
    assert_ne!(
        slot0.service.selected_endpoint().expect("endpoint").port,
        slot1.service.selected_endpoint().expect("endpoint").port
    );
    assert_ne!(
        slot0.service.info().service_instance_id,
        slot1.service.info().service_instance_id
    );
    let slot0_ps = observe_registry(&slot0.registry).expect("slot 0 ps should observe");
    let slot1_ps = observe_registry(&slot1.registry).expect("slot 1 ps should observe");
    assert!(slot0_ps.processes.iter().any(|process| process.live));
    assert!(slot1_ps.processes.iter().any(|process| process.live));

    stop_recorded_processes(&mut slot0.registry, 1000).expect("slot 0 down should stop slot 0");
    drop(slot0.service);
    let slot0_after_down =
        observe_registry(&slot0.registry).expect("slot 0 ps should observe after down");
    let slot1_after_down = observe_registry(&slot1.registry).expect("slot 1 ps should remain live");
    assert!(
        slot0_after_down
            .processes
            .iter()
            .all(|process| !process.live)
    );
    assert!(
        slot1_after_down
            .processes
            .iter()
            .any(|process| process.live)
    );

    let slot0_identity = StateIdentity::from_selected_slot(admission.common(), &slot0.selected);
    clean_marked_state(&slot0_identity, &mut slot0.registry, CleanupMode::Standard)
        .expect("slot 0 cleanup should succeed after down");
    assert!(!slot0.placement.state_root().exists());
    assert!(slot1.placement.state_root().exists());

    let slot1_identity = StateIdentity::from_selected_slot(admission.common(), &slot1.selected);
    clean_marked_state(&slot0_identity, &mut slot1.registry, CleanupMode::Standard)
        .expect_err("slot 0 identity must not clean slot 1 state");
    assert!(slot1.placement.state_root().exists());

    slot1
        .service
        .stop(&mut slot1.registry, 1000)
        .expect("slot 1 service should stop");
    clean_marked_state(&slot1_identity, &mut slot1.registry, CleanupMode::Standard)
        .expect("slot 1 cleanup should succeed after stop");
}

#[test]
fn dependent_task_runs_after_owned_service_is_ready() {
    let port = available_port_window(1);
    let mut value = test_child_manifest(port, port);
    set_task_run_args(&mut value, &["output", "literal", "task-ok", ""]);
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture.start_ready("run-task", port);

    let task = fixture
        .run_smoke(&service, &CancellationToken::new())
        .expect("ready dependent task should run");
    let TaskExecution::Succeeded(evidence) = task else {
        panic!("ready dependent task should succeed");
    };
    let task = evidence;

    assert!(task.success);
    assert_eq!(task.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(&task.stdout_path).expect("stdout should read"),
        "task-ok"
    );
    assert!(task.stderr_path.exists());
    assert!(task.summary_path.exists());
    let summary: Value =
        serde_json::from_slice(&fs::read(&task.summary_path).expect("task summary should read"))
            .expect("task summary should parse");
    assert!(
        summary["durationMs"].as_u64().is_some(),
        "task summary should carry durationMs: {summary}"
    );
    let task_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type IN ('task.running','task.succeeded')",
        [],
    );
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&task.process_key],
    );
    let run_status: Option<String> = fixture.query(
        "SELECT execution_outcome FROM runs WHERE run_id = 'run-task'",
        [],
    );

    assert_eq!(task_events, 2);
    assert_eq!(process_status, "succeeded");
    assert_eq!(run_status, None, "a leaf cannot settle its session");
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn readiness_timeout_stops_started_service_and_records_failed() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["30"], port);
    let service = fixture
        .start("run-readiness-timeout", port)
        .expect("service should initially start");

    let (service, error) = fixture
        .ready(service)
        .expect_err("readiness should time out and clean up")
        .into_parts();

    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    let process_key = service.info().process_key.clone();
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&process_key],
    );
    let failure_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.failed'",
        [],
    );
    assert_eq!(process_status, "failed");
    assert_eq!(failure_events, 1);
}

/// The fixture manifest with an exec-based ready probe: a Nix-packaged fixture whose
/// args are supplied per test. The probe's operation is bound on the closure,
/// as admission requires.
fn exec_probe_fixture_value(
    executable: &str,
    start_args: &[&str],
    port: u16,
    probe_args: Value,
    probe_attempts: u32,
) -> Value {
    let mut value = synthetic_manifest(executable, start_args, port, port);
    add_probe_fixture_closure(&mut value);
    let mut run = vec![json!("nixfied-test-fixture")];
    run.extend(probe_args.as_array().expect("probe args").iter().cloned());
    value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["readyProbe"] =
        probe_fixture_invocation(Value::Array(run));
    value["services"]["synthetic"]["lifecycle"]["ready"]["policy"] =
        json!({"timeoutMs": 1000, "retryIntervalMs": 50, "maxAttempts": probe_attempts});
    value
}

/// A realised fixture closure selected by invocation probes.
fn add_probe_fixture_closure(value: &mut Value) {
    let target = value["target"]["closureSystem"].clone();
    let shell = test_fixture();
    let store_path = closure_root_for_store_executable(Path::new(&shell)).unwrap();
    value["closures"]["probe-shell"] = json!({
        "kind": "executable", "storePath": store_path, "executable": shell,
        "targetSystem": target,

        "requiresExecutable": true, "effects": ["process"]
    });
}

fn probe_fixture_invocation(run: Value) -> Value {
    json!({
        "tools": ["probe-shell"],
        "run": run,
        "executable": test_fixture(),
        "env": {},
        "codebaseId": "main",
        "cwd": ".",
        "stdin": "null"
    })
}

#[test]
fn exec_ready_probe_gates_on_flag_and_marks_ready() {
    let port = available_port_window(1);
    // The service binds its endpoint immediately but signals readiness only via
    // a marker acknowledgement — exactly what a tcp probe cannot see.
    let child = test_child();
    let value = exec_probe_fixture_value(
        child
            .to_str()
            .expect("test child store path should be valid UTF-8"),
        &[
            "listen",
            "127.0.0.1",
            "${port}",
            "ready-on-marker",
            "${stateDir}/listener-bound",
            "${stateDir}/ready-ack",
            "${stateDir}/ready-flag",
        ],
        port,
        json!(["exists", "${stateDir}/ready-flag"]),
        60,
    );
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture
        .start("run-exec-probe", port)
        .expect("service should start");

    let listener_bound = fixture.placement.state_root().join("listener-bound");
    let ready_ack = fixture.placement.state_root().join("ready-ack");
    let ready_flag = fixture.placement.state_root().join("ready-flag");
    assert!(
        wait_for_path(&listener_bound, Duration::from_secs(5)),
        "service child should announce the bound listener"
    );
    assert!(!ready_flag.exists(), "listener bind must precede readiness");
    let first_probe_log = fixture
        .placement
        .logs_dir()
        .join("lifecycle.synthetic.ready.probe.0.stdout.log");
    let acknowledge = thread::spawn(move || {
        assert!(
            wait_for_path(&first_probe_log, Duration::from_secs(5)),
            "the exec probe should fail at least once before acknowledgement"
        );
        fs::write(ready_ack, b"ready").expect("test should acknowledge readiness");
    });

    let service = fixture
        .ready(service)
        .expect("exec probe should succeed once the flag appears");
    acknowledge
        .join()
        .expect("readiness acknowledger should join");

    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    assert_eq!(process_status, "ready");
    assert!(
        fixture
            .placement
            .logs_dir()
            .join("lifecycle.synthetic.ready.probe.0.stdout.log")
            .exists(),
        "probe attempts should leave captured output"
    );
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn exec_ready_probe_failure_times_out_and_records_failed() {
    let port = available_port_window(1);
    let value = exec_probe_fixture_value(
        test_child().to_str().unwrap(),
        LISTEN_HOLD,
        port,
        json!(["exit", "7"]),
        3,
    );
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture
        .start("run-exec-probe-fail", port)
        .expect("service should initially start");

    let (service, error) = fixture
        .ready(service)
        .expect_err("a failing exec probe should time out and clean up")
        .into_parts();

    assert_eq!(fixture.query::<i64>("SELECT count(*) FROM processes WHERE role='probe' AND service_name='synthetic' AND execution_outcome='failed' AND exit_code=7 AND status='failed'", []), 3);
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM events WHERE event_type='probe.execution-observed'",
            []
        ),
        3
    );
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM runs WHERE execution_outcome IS NOT NULL",
            []
        ),
        0
    );
    for occurrence in 0..3 {
        assert!(
            fixture
                .placement
                .logs_dir()
                .join(format!(
                    "lifecycle.synthetic.ready.probe.{occurrence}.stdout.log"
                ))
                .is_file()
        );
    }
    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    assert_eq!(
        error.details["lastRound"],
        json!({"phase":"ready","reason":"probe-failed","endpointId":"synthetic-tcp"})
    );
    let process_key = service.info().process_key.clone();
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&process_key],
    );
    assert_eq!(process_status, "failed");
}

#[test]
fn exec_probe_obeys_its_attempt_deadline() {
    for acknowledge in [true, false] {
        let port = available_port_window(1);
        let child = test_child();
        let mut value = exec_probe_fixture_value(
            child.to_str().unwrap(),
            &["listen", "127.0.0.1", "${port}", "hold"],
            port,
            json!(["prepare", "${stateDir}/attempt", "${stateDir}/ack"]),
            1,
        );
        value["services"]["synthetic"]["lifecycle"]["ready"]["policy"]["timeoutMs"] =
            json!(if acknowledge { 2000 } else { 50 });
        let mut fixture = ServiceFixture::from_value(value);
        let service = fixture.start("probe-deadline", port).unwrap();
        let root = fixture.placement.state_root().clone();
        let acknowledger = acknowledge.then(|| {
            thread::spawn(move || {
                assert!(wait_for_path(&root.join("attempt"), Duration::from_secs(5)));
                thread::sleep(Duration::from_millis(100));
                fs::write(root.join("ack"), b"ready").unwrap();
            })
        });
        let result = fixture.ready(service);
        if let Some(acknowledger) = acknowledger {
            acknowledger.join().unwrap();
            let service = result.expect("the probe should complete within its attempt deadline");
            service.stop(&mut fixture.registry, 1000).unwrap();
        } else {
            let (service, error) = result
                .expect_err("the probe deadline must terminate a blocked attempt")
                .into_parts();
            assert_eq!(
                error.details["lastRound"]["reason"], "probe-timed-out",
                "{error:?}"
            );
            service.finalize_failed_start(&mut fixture.registry, 1000, error);
        }
    }
}

#[test]
fn exec_health_probe_failure_records_failed() {
    for reject_settlement in [false, true] {
        let port = available_port_window(1);
        // The service is ready (tcp) but never healthy: the failed run must leave
        // service.failed evidence, not a clean stopped/completed registry state.
        let mut value = test_child_manifest(port, port);
        add_probe_fixture_closure(&mut value);
        value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["healthProbe"] =
            probe_fixture_invocation(json!(["nixfied-test-fixture", "exit", "7"]));
        value["services"]["synthetic"]["lifecycle"]["health"]["policy"] =
            json!({"timeoutMs": 1000, "retryIntervalMs": 50, "maxAttempts": 2});
        let mut fixture = ServiceFixture::from_value(value);
        let mut service = fixture.start_ready("run-exec-health-fail", port);

        let error = service
            .check_health(
                &mut fixture.registry,
                &CancellationToken::new(),
                &mut || Ok(()),
            )
            .expect_err("a failing health probe should fail the service");

        assert_ne!(error.code, ErrorCode::Canceled);
        let process_key = service.info().process_key.clone();
        let pgid = service.info().pgid;
        let health_error_code = error.code;
        if reject_settlement {
            fixture
                .registry
                .connection()
                .execute_batch(
                    "CREATE TRIGGER reject_health_settlement BEFORE INSERT ON events
             WHEN NEW.event_type = 'service.failed'
             BEGIN SELECT RAISE(ABORT, 'health-settlement-denied'); END;",
                )
                .unwrap();
        }
        let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
        assert_ne!(error.code, ErrorCode::Canceled);
        let process_status: String = fixture.query(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&process_key],
        );
        assert_eq!(
            process_status,
            if reject_settlement { "ready" } else { "failed" }
        );
        assert!(!process_group_has_non_zombie_member(pgid));
        if reject_settlement {
            assert_eq!(error.code, ErrorCode::RegistryCorrupt);
            assert!(error.message.contains("health-settlement-denied"));
            assert_eq!(error.causes.len(), 1);
            assert_eq!(error.causes[0].code, health_error_code);
        } else {
            assert_eq!(error.code, health_error_code);
            assert!(error.causes.is_empty());
        }
        let run_status: Option<String> = fixture.query(
            "SELECT execution_outcome FROM runs WHERE run_id = 'run-exec-health-fail'",
            [],
        );
        assert_eq!(
            run_status, None,
            "service settlement cannot settle its session"
        );
    }
}

#[test]
fn cancellation_interrupts_readiness_and_terminates_service_group() {
    let marker = temp_marker("nixfied-cancel-survivor");
    let started = temp_marker("nixfied-cancel-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &["term-tree", &started_arg, &marker_arg],
        port,
        port,
    ));
    let service = fixture
        .start("run-readiness-canceled", port)
        .expect("service should initially start");
    let pgid = service.info().pgid;
    let cancellation = CancellationToken::new();
    let canceler = cancellation.clone();
    let handle = thread::spawn(move || {
        assert!(
            wait_for_path(&started, Duration::from_secs(3)),
            "TERM-ignoring descendant should start before cancellation"
        );
        canceler.cancel();
    });

    let (service, error) = service
        .ready(&mut fixture.registry, &cancellation, &mut || Ok(()))
        .expect_err("readiness should be canceled")
        .into_parts();
    let process_key = service.info().process_key.clone();
    handle.join().expect("canceler should join");
    let settled = service.finalize_failed_start(&mut fixture.registry, 200, error.clone());
    assert_eq!(settled.code, ErrorCode::Canceled, "{settled:?}");
    thread::sleep(Duration::from_millis(2300));
    let report = observe_registry(&fixture.registry).expect("ps should observe canceled service");
    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == process_key)
        .expect("service process should be reported");
    let cancel_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type IN ('service.canceling','service.canceled')",
        [],
    );

    assert_eq!(error.code, ErrorCode::Canceled);
    assert!(!observed.live);
    assert_eq!(observed.registry_status, "canceled");
    assert_eq!(
        cancel_events, 1,
        "a settled readiness cancellation records its terminal"
    );
    assert!(
        !process_group_has_non_zombie_member(pgid),
        "canceled service process group should be empty"
    );
    assert!(
        !marker.exists(),
        "child that ignored TERM should have been killed before touching marker"
    );
    let _ = fs::remove_file(marker);
    let _ = fs::remove_file(started_arg);
}

#[test]
fn cancellation_interrupts_task_and_terminates_task_group() {
    let marker = temp_marker("nixfied-cancel-task-survivor");
    let started = temp_marker("nixfied-cancel-task-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_manifest(port, port);
    set_task_run_args(&mut value, &["term-tree", &started_arg, &marker_arg]);
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture.start_ready("run-task-canceled", port);
    let cancellation = CancellationToken::new();
    let canceler = cancellation.clone();
    let handle = thread::spawn(move || {
        assert!(
            wait_for_path(&started, Duration::from_secs(3)),
            "TERM-ignoring task descendant should start before cancellation"
        );
        canceler.cancel();
    });

    let result = fixture
        .run_smoke(&service, &cancellation)
        .expect("task should complete with a canceled outcome");
    let TaskExecution::Failed { error, evidence } = result else {
        panic!("task should be canceled");
    };
    let task_run = evidence;
    handle.join().expect("canceler should join");
    thread::sleep(Duration::from_millis(2300));
    let report = observe_registry(&fixture.registry).expect("ps should observe canceled task");
    let task_observations = report
        .processes
        .iter()
        .filter(|process| process.service_instance_id.is_none())
        .collect::<Vec<_>>();
    let task_status: String = fixture.query("SELECT status FROM processes WHERE role = 'task'", []);
    let task_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type IN ('task.canceling','task.canceled')",
        [],
    );
    let summary: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .placement
                .summary_path()
                .with_file_name("summary.0.json"),
        )
        .expect("summary should read"),
    )
    .expect("summary should parse");

    assert_eq!(error.code, ErrorCode::Canceled);
    assert!(!task_observations.is_empty());
    assert!(task_observations.iter().all(|process| !process.live));
    for process in &task_observations {
        assert!(
            !process_group_has_non_zombie_member(process.pgid),
            "canceled task process group should be empty"
        );
    }
    assert!(
        !marker.exists(),
        "task child that ignored TERM should have been killed before touching marker"
    );
    assert_eq!(task_status, "canceled");
    assert_eq!(task_events, 2);
    assert_eq!(summary["canceled"], json!(true));
    assert!(task_run.canceled);
    let _ = fs::remove_file(marker);
    let _ = fs::remove_file(started_arg);
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn task_timeout_records_failed_summary_and_terminates_task_group() {
    let marker = temp_marker("nixfied-timeout-task-survivor");
    let started = temp_marker("nixfied-timeout-task-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_manifest(port, port);
    value["tasks"]["smoke"]["invocation"]["timeoutMs"] = json!(100);
    set_task_run_args(&mut value, &["term-tree", &started_arg, &marker_arg]);
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture.start_ready("run-task-timeout", port);

    let result = fixture
        .run_smoke(&service, &CancellationToken::new())
        .expect("task should complete with a timed-out failure outcome");
    let TaskExecution::Failed { error, evidence } = result else {
        panic!("task should time out as a task failure");
    };
    let task_run = evidence;
    thread::sleep(Duration::from_millis(2300));
    let report = observe_registry(&fixture.registry).expect("ps should observe timed-out task");
    let task_observations = report
        .processes
        .iter()
        .filter(|process| process.service_instance_id.is_none())
        .collect::<Vec<_>>();
    let task_status: String = fixture.query("SELECT status FROM processes WHERE role = 'task'", []);
    let task_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type IN ('task.canceling','task.timed-out')",
        [],
    );
    let summary: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .placement
                .summary_path()
                .with_file_name("summary.0.json"),
        )
        .expect("summary should read"),
    )
    .expect("summary should parse");

    assert_eq!(error.code, ErrorCode::TaskFailed);
    assert!(error.message.contains("timed out"));
    assert!(!task_observations.is_empty());
    assert!(task_observations.iter().all(|process| !process.live));
    for process in &task_observations {
        assert!(
            !process_group_has_non_zombie_member(process.pgid),
            "timed-out task process group should be empty"
        );
    }
    assert_eq!(task_status, "failed");
    assert_eq!(task_events, 2);
    assert_eq!(summary["timedOut"], json!(true));
    assert!(task_run.timed_out);
    assert_eq!(summary["canceled"], json!(false));
    assert!(
        started.exists(),
        "timeout proof should create the TERM-ignoring descendant"
    );
    assert!(
        !marker.exists(),
        "task timeout should kill TERM-ignoring descendants before marker"
    );
    let _ = fs::remove_file(marker);
    let _ = fs::remove_file(started);
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn cli_signal_cancels_run_and_empties_service_group() {
    let marker = temp_marker("nixfied-cli-cancel-survivor");
    let started = temp_marker("nixfied-cli-cancel-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let port = available_port_window(1);
    let value = test_child_service(&["term-tree", &started_arg, &marker_arg], port, port);
    let fixture = RuntimeFixture::new(value);

    let mut child = fixture
        .command(
            "run",
            &["--task", "smoke", "--timeout-ms", "200", "--output", "json"],
        )
        .spawn()
        .expect("runtime run should spawn");
    if !wait_for_path(&started, Duration::from_secs(3)) {
        let _ = child.kill();
        let output = wait_for_child_output(child, Duration::from_secs(1));
        panic!(
            "service did not start before signal\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "SIGTERM should be delivered to runtime");
    let output = wait_for_child_output(child, Duration::from_secs(6));
    assert_eq!(
        output.status.code(),
        Some(27),
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let error: Value = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("CANCELED"));

    let process = ps_service_row(&fixture);
    let pgid = process["pgid"]
        .as_i64()
        .expect("reported process should include pgid") as i32;
    assert_eq!(process["live"], json!(false));
    assert_eq!(process["registryStatus"], json!("canceled"));
    assert!(
        !process_group_has_non_zombie_member(pgid),
        "CLI-canceled service process group should be empty"
    );
    thread::sleep(Duration::from_millis(2300));
    assert!(
        !marker.exists(),
        "CLI signal cancellation should kill TERM-ignoring descendants before marker"
    );
    let _ = fs::remove_file(marker);
    let _ = fs::remove_file(started);
}

#[test]
fn cli_signal_during_shutdown_records_canceled_terminal_state() {
    let started = temp_marker("nixfied-cli-shutdown-started");
    let stopping = temp_marker("nixfied-cli-shutdown-stopping");
    let started_arg = started.to_string_lossy().to_string();
    let stopping_arg = stopping.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_service(
        &[
            "term-block",
            "127.0.0.1",
            "${port}",
            &started_arg,
            &stopping_arg,
        ],
        port,
        port,
    );
    set_task_run_args(&mut value, &["exit", "0"]);
    let fixture = RuntimeFixture::new(value);

    let child = fixture
        .command(
            "run",
            &[
                "--task",
                "smoke",
                "--timeout-ms",
                "1000",
                "--output",
                "json",
            ],
        )
        .spawn()
        .expect("runtime run should spawn");
    for (path, timeout, phase) in [
        (&started, Duration::from_secs(3), "service start"),
        (&stopping, Duration::from_secs(5), "service shutdown"),
    ] {
        if !wait_for_path(path, timeout) {
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
            let output = wait_for_child_output(child, Duration::from_secs(6));
            panic!(
                "{phase} marker missing before signal proof\nstatus: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "SIGTERM should be delivered to runtime");
    let output = wait_for_child_output(child, Duration::from_secs(6));
    assert_eq!(output.status.code(), Some(27));
    let error: Value = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("CANCELED"));

    let process = ps_service_row(&fixture);
    assert_eq!(process["live"], json!(false));
    assert_eq!(process["registryStatus"], json!("canceled"));
    let _ = fs::remove_file(started);
    let _ = fs::remove_file(stopping);
}

#[test]
fn readiness_timeout_prefers_escape_discovered_during_probe() {
    let request = temp_marker("nixfied-readiness-timeout-escape-request");
    let armed = temp_marker("nixfied-readiness-timeout-escape-armed");
    let detached = temp_marker("nixfied-readiness-timeout-escape-detached");
    let request_arg = request.to_string_lossy().to_string();
    let armed_arg = armed.to_string_lossy().to_string();
    let detached_arg = detached.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_service(
        &[
            "detached-sleeper",
            "after-marker",
            &request_arg,
            &armed_arg,
            &detached_arg,
        ],
        port,
        port,
    );
    // Only the running exec probe requests the escape. It then stays blocked,
    // so containment must be reconciled while the probe is in flight.
    let mut invocation = value["services"]["synthetic"]["lifecycle"]["start"]["invocation"].clone();
    let program = invocation["run"][0].clone();
    invocation["run"] = json!([program, "output", "hex-block", "", "", request_arg]);

    value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["readyProbe"] = invocation;
    value["services"]["synthetic"]["lifecycle"]["ready"]["policy"] =
        json!({"timeoutMs": 1000, "retryIntervalMs": 20, "maxAttempts": 1});
    // This fixture probes containment while no listener is declared.
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"] =
        value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["readyProbe"].clone();
    value["services"]["synthetic"]["lifecycle"]["health"]["probe"] =
        value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["healthProbe"].clone();
    value["services"]["synthetic"]
        .as_object_mut()
        .unwrap()
        .remove("endpoints");
    value["services"]["synthetic"]
        .as_object_mut()
        .unwrap()
        .remove("primaryEndpoint");
    value["closures"]["synthetic-helper"]["effects"] = json!(["process"]);
    set_task_run_args(&mut value, &["exit", "0"]);
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture
        .start_endpoint_less("run-readiness-timeout-escape")
        .expect("service should initially start");
    assert!(
        wait_for_path(&armed, Duration::from_secs(3)),
        "service child should arm the escape request"
    );
    assert!(!request.exists(), "no escape request before the exec probe");
    assert!(
        !detached.exists(),
        "no detached child before the exec probe"
    );

    let (service, error) = fixture
        .ready(service)
        .expect_err("readiness should report the monitored escape")
        .into_parts();

    assert!(
        request.exists(),
        "the exec probe must have requested the escape"
    );
    assert!(
        detached.exists(),
        "the service must have escaped during the probe"
    );
    assert_eq!(error.code, ErrorCode::ProcEscape);
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ProcEscape);
    let failed_processes: i64 =
        fixture.query("SELECT count(*) FROM processes WHERE status = 'failed'", []);
    let failure_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.failed'",
        [],
    );
    assert_eq!(failed_processes, 1);
    assert_eq!(failure_events, 1);
    for marker in [request, armed, detached] {
        let _ = fs::remove_file(marker);
    }
}

#[test]
fn daemonizing_service_is_terminated_and_recorded_failed() {
    let child_ready = temp_marker("nixfied-daemon-child-ready");
    let child_ready_arg = child_ready.to_string_lossy().to_string();
    let mut attempts = 0;
    let (fixture, error) = loop {
        attempts += 1;
        assert!(
            attempts <= 8,
            "no uncontended fixture port after eight tries"
        );
        let port = available_port_window(1);
        let mut fixture = ServiceFixture::from_value(test_child_service(
            &["detached-sleeper", "parent-exit", &child_ready_arg],
            port,
            port,
        ));
        let error =
            fixture.start_refused("run-escape", port, "daemonizing service should be refused");
        if error.code == ErrorCode::PortConflict {
            // This test exercises containment, so retry a genuine host bind
            // collision before the workload starts.
            assert!(!child_ready.exists(), "port conflict executed the workload");
            match std::net::TcpListener::bind(("127.0.0.1", port)) {
                Err(bind) if bind.kind() == std::io::ErrorKind::AddrInUse => continue,
                result => panic!("false port conflict: {error}; raw bind: {result:?}"),
            }
        }
        break (fixture, error);
    };
    assert_eq!(error.code, ErrorCode::ProcEscape, "{error:?}");
    let failed_processes: i64 =
        fixture.query("SELECT count(*) FROM processes WHERE status = 'failed'", []);
    let open_ports: i64 = fixture.query(
        "SELECT count(*) FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key WHERE p.ownership = 'unresolved'",
        [],
    );

    assert_eq!(failed_processes, 1);
    assert_eq!(open_ports, 0);
    assert!(child_ready.exists(), "daemon child should have started");
    let _ = fs::remove_file(child_ready);
}

#[test]
fn setsid_descendant_is_identity_killed_before_failed_settlement() {
    let detached = temp_marker("nixfied-start-escape-detached");
    let detached_arg = detached.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &["detached-sleeper", "immediate", &detached_arg],
        port,
        port,
    ));

    let error = fixture.start_refused(
        "run-setsid-escape",
        port,
        "setsid descendant should be refused",
    );

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let failed_processes: i64 =
        fixture.query("SELECT count(*) FROM processes WHERE status = 'failed'", []);
    let escaped_processes: i64 = fixture.query(
        "SELECT count(*) FROM processes WHERE status = 'escaped'",
        [],
    );
    assert_eq!(failed_processes, 1);
    assert_eq!(escaped_processes, 0);
    assert!(detached.exists(), "setsid descendant should have started");
    let _ = fs::remove_file(detached);
}

#[test]
fn ready_service_checkpoint_reaps_exit_and_teardown_retains_failure() {
    let mut fixture = endpoint_less_fixture();
    let service = fixture
        .start_endpoint_less("run-reap-at-checkpoint")
        .unwrap();
    let service = fixture.ready(service).unwrap();
    let pid = service.info().pid as i32;
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    // Observe exit without reaping: the runtime checkpoint must consume it.
    let mut exited: libc::siginfo_t = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe {
            libc::waitid(
                libc::P_PID,
                pid as _,
                &mut exited,
                libc::WEXITED | libc::WNOWAIT,
            )
        },
        0
    );
    let error = poll_until(Duration::from_secs(3), "the service to exit", || {
        service.check_liveness().err()
    });
    assert_eq!(error.code, ErrorCode::DependencyUnavailable);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert_eq!(
        service.check_liveness().unwrap_err().code,
        ErrorCode::DependencyUnavailable
    );
    let error = service.stop(&mut fixture.registry, 1000).unwrap_err();
    assert_eq!(error.code, ErrorCode::ProcEscape);
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM processes WHERE role='service' AND status='failed'",
            []
        ),
        1
    );
}

#[test]
fn ready_service_checkpoint_detects_new_escape_before_leader_exit() {
    let root = TempDir::new();
    let request = root.path.join("request");
    let armed = root.path.join("armed");
    let detached = root.path.join("detached");
    let mut fixture = endpoint_less_fixture_from(test_child_service(
        &[
            "detached-sleeper",
            "after-marker",
            request.to_str().unwrap(),
            armed.to_str().unwrap(),
            detached.to_str().unwrap(),
        ],
        23180,
        23180,
    ));
    let service = fixture
        .start_endpoint_less("run-checkpoint-escape")
        .unwrap();
    let service = fixture.ready(service).unwrap();
    service.check_liveness().unwrap();
    assert!(wait_for_path(&armed, Duration::from_secs(3)));
    fs::write(&request, []).unwrap();
    assert!(wait_for_path(&detached, Duration::from_secs(3)));
    assert_eq!(unsafe { libc::kill(service.info().pid as i32, 0) }, 0);
    assert_eq!(
        service.check_liveness().unwrap_err().code,
        ErrorCode::ProcEscape
    );
    assert_eq!(
        service.stop(&mut fixture.registry, 1000).unwrap_err().code,
        ErrorCode::ProcEscape
    );
    assert_eq!(
        fixture.query::<i64>(
            "SELECT count(*) FROM processes WHERE role='service' AND status='failed'",
            []
        ),
        1
    );
}

/// Cancellation contains every tracked descendant, not only the group: a
/// setsid escapee that holds no group membership is still killed before the
/// row records a settled cancellation.
#[test]
fn cancel_kills_a_tracked_setsid_descendant_before_settlement() {
    let root = TempDir::new();
    let [request, armed, detached, pid_file] =
        ["request", "armed", "detached", "escaped.pid"].map(|name| root.path.join(name));
    let mut fixture = endpoint_less_fixture_from(test_child_service(
        &[
            "detached-sleeper",
            "after-marker",
            request.to_str().unwrap(),
            armed.to_str().unwrap(),
            detached.to_str().unwrap(),
            pid_file.to_str().unwrap(),
        ],
        23180,
        23180,
    ));
    let service = fixture.start_endpoint_less("run-cancel-escape").unwrap();
    let service = fixture.ready(service).unwrap();
    assert!(wait_for_path(&armed, Duration::from_secs(3)));
    fs::write(&request, []).unwrap();
    let escaped = wait_for_pid_file(&pid_file);
    assert!(wait_for_path(&detached, Duration::from_secs(3)));

    service
        .cancel(&mut fixture.registry, 1000, "test cancellation")
        .expect("cancellation should contain the escapee");

    wait_for_process_exit(escaped);
    let (status, ownership): (String, String) = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status, ownership FROM processes WHERE role = 'service'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (status.as_str(), ownership.as_str()),
        ("canceled", "settled")
    );
}

#[test]
fn stop_terminates_delayed_setsid_escape_and_records_failure() {
    let request = temp_marker("nixfied-stop-escape-request");
    let armed = temp_marker("nixfied-stop-escape-armed");
    let detached = temp_marker("nixfied-stop-escape-detached");
    let request_arg = request.to_string_lossy().to_string();
    let armed_arg = armed.to_string_lossy().to_string();
    let detached_arg = detached.to_string_lossy().to_string();
    let mut fixture = endpoint_less_fixture_from(test_child_service(
        &[
            "detached-sleeper",
            "after-marker",
            &request_arg,
            &armed_arg,
            &detached_arg,
        ],
        23185,
        23185,
    ));
    let service = fixture
        .start_endpoint_less("run-delayed-escape")
        .expect("service should initially pass handoff");
    let service = fixture.ready(service).expect("service should become ready");
    assert!(wait_for_path(&armed, Duration::from_secs(3)));
    fs::write(&request, []).expect("escape request should be written");
    assert!(wait_for_path(&detached, Duration::from_secs(3)));

    let error = service
        .stop(&mut fixture.registry, 1000)
        .expect_err("stop should refuse escaped descendants");

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let failed_processes: i64 =
        fixture.query("SELECT count(*) FROM processes WHERE status = 'failed'", []);
    let escaped_processes: i64 = fixture.query(
        "SELECT count(*) FROM processes WHERE status = 'escaped'",
        [],
    );
    assert_eq!(failed_processes, 1);
    assert_eq!(escaped_processes, 0);
    for marker in [request, armed, detached] {
        let _ = fs::remove_file(marker);
    }
}

#[test]
fn readiness_refuses_monitored_setsid_escape() {
    let request = temp_marker("nixfied-readiness-escape-request");
    let armed = temp_marker("nixfied-readiness-escape-armed");
    let detached = temp_marker("nixfied-readiness-escape-detached");
    let request_arg = request.to_string_lossy().to_string();
    let armed_arg = armed.to_string_lossy().to_string();
    let detached_arg = detached.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &[
            "detached-sleeper",
            "after-marker",
            &request_arg,
            &armed_arg,
            &detached_arg,
        ],
        port,
        port,
    ));
    let service = fixture
        .start("run-readiness-escape", port)
        .expect("service should initially pass handoff");
    assert!(wait_for_path(&armed, Duration::from_secs(3)));
    fs::write(&request, []).expect("escape request should be written");
    assert!(wait_for_path(&detached, Duration::from_secs(3)));

    let (service, error) = fixture
        .ready(service)
        .expect_err("readiness should refuse monitored escape")
        .into_parts();

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ProcEscape);
    let probe_ready_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
        [],
    );
    let failed_processes: i64 =
        fixture.query("SELECT count(*) FROM processes WHERE status = 'failed'", []);
    assert_eq!(probe_ready_events, 0);
    assert_eq!(failed_processes, 1);
    for marker in [request, armed, detached] {
        let _ = fs::remove_file(marker);
    }
}

#[test]
fn readiness_records_foreground_exit_as_escape() {
    let started = temp_marker("nixfied-readiness-exit-started");
    let exit_request = temp_marker("nixfied-readiness-exit-request");
    let started_arg = started.to_string_lossy().to_string();
    let exit_request_arg = exit_request.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &["prepare", &started_arg, &exit_request_arg],
        port,
        port,
    ));
    let service = fixture
        .start("run-readiness-exit", port)
        .expect("service should initially pass handoff");
    assert!(wait_for_path(&started, Duration::from_secs(3)));
    fs::write(&exit_request, []).expect("foreground exit should be requested");
    wait_for_process_exit(service.info().pid);

    let (service, error) = fixture
        .ready(service)
        .expect_err("readiness should record foreground exit as escape")
        .into_parts();

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ProcEscape);
    let failed_processes: i64 =
        fixture.query("SELECT count(*) FROM processes WHERE status = 'failed'", []);
    let running_processes: i64 = fixture.query(
        "SELECT count(*) FROM processes WHERE status = 'running'",
        [],
    );
    assert_eq!(failed_processes, 1);
    assert_eq!(running_processes, 0);
    let _ = fs::remove_file(started);
    let _ = fs::remove_file(exit_request);
}

#[test]
fn process_tree_listener_retained_after_primary_exit_remains_proc_escape() {
    let bound = temp_marker("nixfied-detached-listener-bound");
    let exit_request = temp_marker("nixfied-detached-listener-exit-request");
    let exiting = temp_marker("nixfied-detached-listener-exiting");
    let bound_arg = bound.to_string_lossy().to_string();
    let exit_request_arg = exit_request.to_string_lossy().to_string();
    let exiting_arg = exiting.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_service(
        &[
            "detached-listener",
            "127.0.0.1",
            "${port}",
            &bound_arg,
            &exit_request_arg,
            &exiting_arg,
        ],
        port,
        port,
    );
    value["services"]["synthetic"]["containment"] = json!("process-tree");
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture
        .start("run-process-tree-primary-exit", port)
        .expect("parent should survive the foreground handoff");
    assert!(wait_for_path(&bound, Duration::from_secs(3)));
    fs::write(&exit_request, []).expect("primary exit should be requested");
    assert!(wait_for_path(&exiting, Duration::from_secs(3)));
    wait_for_process_exit(service.info().pid);

    let (service, error) = fixture
        .ready(service)
        .expect_err("a listener retained by the tracked child is still a process escape")
        .into_parts();

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let error = service.finalize_failed_start(&mut fixture.registry, 1000, error);
    assert_eq!(error.code, ErrorCode::ProcEscape);
    TcpListener::bind(("127.0.0.1", port))
        .expect("failure cleanup should terminate the identity-tracked child listener");
    for marker in [bound, exit_request, exiting] {
        let _ = fs::remove_file(marker);
    }
}

/// A second start while an owner is live is refused before any effect: the
/// owner is not signaled and its evidence is unchanged.
#[test]
fn second_start_against_a_live_owner_is_refused_without_mutation() {
    for (ready, other_port, expected, reason) in [
        (
            false,
            false,
            ErrorCode::PortConflict,
            Some("startup-lock-contended"),
        ),
        (true, false, ErrorCode::PortConflict, None),
        // An unsettled predecessor refuses a start on another port too.
        (true, true, ErrorCode::RegistryCorrupt, None),
    ] {
        let port = available_port_window(2);
        let mut fixture = test_child_listener_fixture(port);
        let owner = fixture
            .start("run-owner", port)
            .expect("owner should start");
        let (pgid, process_key) = (owner.info().pgid, owner.info().process_key.clone());
        let owner: Result<ReadyService, StartingService> = if ready {
            Ok(fixture.ready(owner).expect("owner should become ready"))
        } else {
            Err(owner)
        };
        let evidence = |fixture: &ServiceFixture| -> (i64, i64, String, String) {
            fixture
                .registry
                .connection()
                .query_row(
                    "SELECT (SELECT count(*) FROM processes),
                       (SELECT count(*) FROM events WHERE run_id = 'run-owner'),
                       (SELECT status FROM processes WHERE process_key = ?1),
                       (SELECT group_concat(endpoint_id) FROM ports)",
                    [&process_key],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap()
        };
        let before = evidence(&fixture);
        let error = fixture.start_refused(
            "run-other",
            port + u16::from(other_port),
            "a live owner must refuse a second start",
        );
        assert_eq!(error.code, expected);
        if let Some(reason) = reason {
            assert_eq!(error.details["portConflict"]["reason"], json!(reason));
        }
        assert!(
            process_group_has_non_zombie_member(pgid),
            "a refusal must not signal the owner"
        );
        assert_eq!(
            evidence(&fixture),
            before,
            "the refused start registered nothing"
        );
        let owner = match owner {
            Ok(owner) => owner,
            Err(owner) => fixture.ready(owner).expect("owner should become ready"),
        };
        owner
            .stop(&mut fixture.registry, 1000)
            .expect("owner should stop");
    }
}

#[test]
fn ps_observes_dead_process_without_mutating_evidence() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["1"], port);
    let service = fixture
        .start("run-ps-stale", port)
        .expect("foreground service should start");
    thread::sleep(Duration::from_millis(1300));

    let report = observe_registry(&fixture.registry).expect("ps should observe");

    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == service.info().process_key)
        .expect("process should be reported");
    assert!(!observed.live);
    assert_eq!(observed.observed_status, "stale");
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    let stale_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'process.stale'",
        [],
    );
    assert_eq!(process_status, "running");
    assert_eq!(stale_events, 0);
    stop_recorded_processes(&mut fixture.registry, 1000).unwrap();
    let status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    assert_eq!(status, "stale");
}

#[test]
fn ps_rejects_live_process_with_mismatched_start_identity_as_stale() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["30"], port);
    let service = fixture
        .start("run-ps-pid-reuse", port)
        .expect("foreground service should start");
    assert!(
        process_group_has_non_zombie_member(service.info().pgid),
        "test service process group should be live before identity mutation"
    );
    let mismatched_identity = json!({
        "pid": service.info().pid,
        "pgid": service.info().pgid,
        "platformStart": "not-the-recorded-process-start",
        "observedAtNanos": unique_suffix(),
    })
    .to_string();
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE processes SET start_identity = ?2 WHERE process_key = ?1",
            (&service.info().process_key, &mismatched_identity),
        )
        .expect("test should corrupt start identity");

    let report =
        observe_registry(&fixture.registry).expect("ps should observe mismatched identity");

    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == service.info().process_key)
        .expect("process should be reported");
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );

    assert!(!observed.live);
    assert_eq!(observed.observed_status, "stale");
    assert_eq!(process_status, "running");
    assert!(
        process_group_has_non_zombie_member(service.info().pgid),
        "OS process group should still be live; stale status must come from identity mismatch"
    );
}

#[test]
fn ps_keeps_live_process_ready_when_its_listener_disappears() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &[
            "listen",
            "127.0.0.1",
            "${port}",
            "close-on-marker",
            "${stateDir}/listener-close",
            "${stateDir}/listener-closed",
        ],
        port,
        port,
    ));
    let service = fixture.start_ready("run-listener-loss-ps", port);
    fs::write(
        fixture.placement.state_root().join("listener-close"),
        b"close",
    )
    .expect("test should request listener loss");
    assert!(
        wait_for_path(
            &fixture.placement.state_root().join("listener-closed"),
            Duration::from_secs(5)
        ),
        "service child should acknowledge listener loss"
    );

    let report = observe_registry(&fixture.registry).expect("ps should remain process-only");
    let process = report
        .processes
        .iter()
        .find(|process| process.process_key == service.info().process_key)
        .expect("service process should be reported");
    assert!(process.live);
    assert_eq!(process.observed_status, "running");
    let registry_status: String = fixture.query(
        "
            SELECT p.status
            FROM processes p
            WHERE p.process_key = ?1
            ",
        [&service.info().process_key],
    );
    assert_eq!(registry_status, "ready");
    service
        .stop(&mut fixture.registry, 1000)
        .expect("process-owned down path should not depend on the listener");
}

#[test]
fn down_stops_verified_owned_process_group_only() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["30"], port);
    let service = fixture
        .start("run-down", port)
        .expect("foreground service should start");

    let report =
        stop_recorded_processes(&mut fixture.registry, 1000).expect("down should stop service");

    assert_eq!(report.stopped, vec![service.info().process_key.clone()]);
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    let released_ports: i64 =
        fixture.query("SELECT count(*) FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key WHERE p.ownership = 'settled'", []);
    assert_eq!(process_status, "stopped");
    assert_eq!(released_ports, 1);
}

#[test]
fn escaped_plus_open_port_remains_actionable_until_down_proves_death() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["30"], port);
    let service = fixture
        .start("run-unresolved-escape", port)
        .expect("service should start");
    let start_identity =
        stored_process_start_identity(&fixture.registry, &service.info().process_key);
    mark_started_service_escape(
        &mut fixture.registry,
        service.info(),
        service.info().pid,
        &start_identity,
        r#"{"reason":"test-unconfirmable-termination"}"#,
    );

    let open_ports: i64 = fixture.query(
        "SELECT count(*) FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key WHERE p.ownership = 'unresolved'",
        [],
    );
    assert_eq!(open_ports, 1);
    let report =
        observe_registry(&fixture.registry).expect("ps should observe escaped owner liveness");
    let escaped = report
        .processes
        .iter()
        .find(|process| process.process_key == service.info().process_key)
        .expect("escaped process should remain visible");
    assert!(escaped.live);
    assert_eq!(escaped.observed_status, "escaped");

    let selected = select_slot(fixture.admission.common().manifest(), None)
        .expect("default slot should select");
    let identity = StateIdentity::from_selected_slot(fixture.admission.common(), &selected);
    commit_slot_marker(&fixture.registry, &identity).expect("slot marker should be written");
    let cleanup_error = clean_marked_state(&identity, &mut fixture.registry, CleanupMode::Standard)
        .expect_err("open unresolved escape must block cleanup");
    assert_eq!(cleanup_error.code, ErrorCode::CleanupRefused);

    let down = stop_recorded_processes(&mut fixture.registry, 1000)
        .expect("down should remain available without endpoint observation");
    assert_eq!(down.stopped, vec![service.info().process_key.clone()]);
    let terminal: (String, String) = fixture
        .registry
        .connection()
        .query_row(
            "
            SELECT p.status, p.ownership
            FROM processes p
            JOIN ports o ON o.owner_process_key = p.process_key
            WHERE p.process_key = ?1
            ",
            [&service.info().process_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("terminal escaped evidence should remain");
    assert_eq!(terminal, ("escaped".into(), "settled".into()));
    drop(service);
}

#[test]
fn unresolved_escape_keeps_ports_while_primary_group_descendant_lives() {
    let pid_dir = TempDir::new();
    let child_pid_path = pid_dir.path.join("child.pid");
    let started = temp_marker("nixfied-unresolved-group-child-started");
    let survivor = temp_marker("nixfied-unresolved-group-child-survivor");
    let started_arg = started.to_string_lossy().to_string();
    let survivor_arg = survivor.to_string_lossy().to_string();
    let child_pid_arg = child_pid_path.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &["term-tree", &started_arg, &survivor_arg, &child_pid_arg],
        port,
        port,
    ));
    let service = fixture
        .start("run-unresolved-group-child", port)
        .expect("service with a group child should start");
    assert!(wait_for_path(&started, Duration::from_secs(3)));
    let child_pid = wait_for_pid_file(&child_pid_path);
    let start_identity =
        stored_process_start_identity(&fixture.registry, &service.info().process_key);
    mark_started_service_escape(
        &mut fixture.registry,
        service.info(),
        service.info().pid,
        &start_identity,
        r#"{"reason":"test-primary-exit-with-live-group-child"}"#,
    );
    assert_eq!(
        unsafe { libc::kill(service.info().pid as libc::pid_t, libc::SIGKILL) },
        0
    );
    wait_for_process_exit(service.info().pid);
    assert!(
        process_is_non_zombie(child_pid),
        "the descendant should still hold the recorded containment"
    );

    let report = observe_registry(&fixture.registry)
        .expect("reconciliation should inspect the complete recorded process group");
    let escaped = report
        .processes
        .iter()
        .find(|process| process.process_key == service.info().process_key)
        .expect("escaped process evidence should remain visible");
    assert!(escaped.live);
    let open_ports: i64 = fixture.query(
        "SELECT count(*) FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key WHERE p.ownership = 'unresolved'",
        [],
    );
    assert_eq!(open_ports, 1);

    let down = stop_recorded_processes(&mut fixture.registry, 1000)
        .expect("down should terminate the surviving group containment");
    assert_eq!(down.stopped, vec![service.info().process_key.clone()]);
    assert!(!process_is_non_zombie(child_pid));
    let _ = fs::remove_file(started);
    let _ = fs::remove_file(survivor);
    drop(service);
}

#[cfg(target_os = "linux")]
#[test]
fn unresolved_escape_keeps_ports_for_identity_tracked_reparented_child() {
    let pid_dir = TempDir::new();
    let child_pid_path = pid_dir.path.join("tree-child.pid");
    let detached = temp_marker("nixfied-unresolved-tree-child-detached");
    let detached_arg = detached.to_string_lossy().to_string();
    let child_pid_arg = child_pid_path.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_service(
        &[
            "detached-sleeper",
            "immediate",
            &detached_arg,
            &child_pid_arg,
        ],
        port,
        port,
    );
    value["services"]["synthetic"]["containment"] = json!("process-tree");
    let mut fixture = ServiceFixture::from_value(value);
    let service = fixture
        .start("run-unresolved-tree-child", port)
        .expect("process-tree service should start");
    let child_pid = wait_for_pid_file(&child_pid_path);
    let child_start = platform_start_for_test(child_pid).expect("child should have start identity");
    let mut start_identity: Value = serde_json::from_str(&stored_process_start_identity(
        &fixture.registry,
        &service.info().process_key,
    ))
    .expect("stored start identity should parse");
    start_identity["trackedProcesses"] = json!([{
        "pid": child_pid,
        "platformStart": child_start,
    }]);
    mark_started_service_escape(
        &mut fixture.registry,
        service.info(),
        service.info().pid,
        &start_identity.to_string(),
        r#"{"reason":"test-primary-exit-with-reparented-tree-child"}"#,
    );
    assert_eq!(
        unsafe { libc::kill(service.info().pid as libc::pid_t, libc::SIGKILL) },
        0
    );
    wait_for_process_exit(service.info().pid);
    assert!(process_is_non_zombie(child_pid));

    let report = observe_registry(&fixture.registry)
        .expect("reconciliation should retain an identity-tracked reparented child");
    let escaped = report
        .processes
        .iter()
        .find(|process| process.process_key == service.info().process_key)
        .expect("escaped tree evidence should remain visible");
    assert!(escaped.live);
    let owner: String = fixture.query(
        "SELECT p.ownership FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key
         WHERE ep.owner_process_key = ?1",
        [&service.info().process_key],
    );
    assert_eq!(owner, "unresolved");

    let down = stop_recorded_processes(&mut fixture.registry, 1000)
        .expect("down should terminate the identity-tracked reparented child");
    assert_eq!(down.stopped, vec![service.info().process_key.clone()]);
    assert!(!process_is_non_zombie(child_pid));
    let _ = fs::remove_file(detached);
    drop(service);
}

#[test]
fn escaped_service_with_exact_listener_is_preserved_until_explicit_down() {
    let port = available_port_window(1);
    let mut fixture = test_child_listener_fixture(port);
    let escaped = fixture.start_ready("run-escaped-exact-listener", port);
    let escaped_process_key = escaped.info().process_key.clone();
    let escaped_pgid = escaped.info().pgid;
    let start_identity =
        stored_process_start_identity(&fixture.registry, &escaped.info().process_key);
    mark_started_service_escape(
        &mut fixture.registry,
        escaped.info(),
        escaped.info().pid,
        &start_identity,
        r#"{"reason":"test-failed-termination"}"#,
    );

    let conflict = fixture.start_refused(
        "run-blocked-by-escaped-owner",
        port,
        "an escaped owner must not be replaced implicitly",
    );
    assert_eq!(conflict.code, ErrorCode::PortConflict);
    assert!(process_group_has_non_zombie_member(escaped_pgid));
    let state: (String, String) = fixture
        .registry
        .connection()
        .query_row(
            "
            SELECT p.status, p.ownership
            FROM processes p
            JOIN ports o ON o.owner_process_key = p.process_key
            WHERE p.process_key = ?1
            ",
            [&escaped_process_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("escaped evidence should remain durable");
    assert_eq!(state, ("escaped".into(), "unresolved".into()));
    let down = stop_recorded_processes(&mut fixture.registry, 1000)
        .expect("explicit down should terminate the preserved escape");
    assert_eq!(down.stopped, vec![escaped_process_key.clone()]);
    assert!(
        !process_group_has_non_zombie_member(escaped_pgid),
        "explicit down must prove the escaped containment dead"
    );
    fixture.begin_run("run-after-escaped-exact-listener");
    let replacement = fixture.start_ready("run-after-escaped-exact-listener", port);

    assert_ne!(replacement.info().process_key, escaped_process_key);
    replacement
        .stop(&mut fixture.registry, 1000)
        .expect("replacement should stop");
    drop(escaped);
}

#[test]
fn owned_process_cleanup_does_not_settle_the_session() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["30"], port);
    let service = fixture
        .start("run-down-canceling-process", port)
        .expect("foreground service should start");
    commit_slot_marker(
        &fixture.registry,
        &default_state_identity(fixture.admission.common()),
    )
    .expect("slot marker should be written for cleanup proof");

    let report =
        stop_recorded_processes(&mut fixture.registry, 1000).expect("down should stop service");
    let run_status: Option<String> = fixture.query(
        "SELECT execution_outcome FROM runs WHERE run_id = ?1",
        [&service.info().run_id],
    );
    let process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    let canceled_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.stopped'",
        [],
    );
    let cleanup = clean_marked_state(
        &default_state_identity(fixture.admission.common()),
        &mut fixture.registry,
        CleanupMode::Standard,
    )
    .expect("settled process should not block cleanup");

    assert_eq!(report.stopped, vec![service.info().process_key.clone()]);
    assert_eq!(
        run_status, None,
        "process cleanup must preserve unknown session outcome"
    );
    assert_eq!(process_status, "stopped");
    assert_eq!(canceled_events, 1);
    assert!(matches!(
        &cleanup,
        nixfied_runtime::state::CleanupOutcome::Deleted { deleted_path, .. }
            if deleted_path.ends_with("runtime-test/dev/0")
    ));
    assert!(!fixture.placement.state_root().exists());
}

#[test]
fn down_rejects_corrupt_process_rows_before_reconciliation_or_signaling() {
    // A process of a missing run is unrepresentable: it references its run.
    for (status, run_id) in [("unknown", "run-corrupt-control")] {
        let port = available_port_window(1);
        let mut fixture = test_child_listener_fixture(port);
        let service = fixture.start_ready("run-corrupt-control", port);
        for (key, status, run_id) in [
            ("a-dead", "running", "run-corrupt-control"),
            ("z-corrupt", status, run_id),
        ] {
            fixture
                .registry
                .connection()
                .execute(
                    "INSERT INTO processes (
                   process_key, pid, pgid, start_identity,
                   command_json, run_id, status, role,
                   source_label, presentation, stdout_path, stderr_path, stop_signal, stop_timeout_ms, containment
                 ) SELECT ?1, 2147483647, 2147483647, start_identity,
                          command_json, ?2, ?3, 'task',
                          'fixture', 'hidden', 'logs/' || ?1 || '.out', 'logs/' || ?1 || '.err', 15, 1000, 'process-group'
                   FROM processes WHERE process_key = ?4",
                    rusqlite::params![key, run_id, status, service.info().process_key],
                )
                .unwrap();
        }
        let events_before: i64 = fixture
            .registry
            .connection()
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        let error = stop_recorded_processes(&mut fixture.registry, 1000)
            .expect_err("all process rows must decode before reconciliation or teardown");
        assert_eq!(error.code, ErrorCode::RegistryCorrupt);
        assert!(
            process_is_non_zombie(service.info().pid),
            "valid service must not be signaled"
        );
        let (dead_status, events_after): (String, i64) = fixture
            .registry
            .connection()
            .query_row(
                "SELECT status, (SELECT count(*) FROM events)
                 FROM processes WHERE process_key = 'a-dead'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            dead_status, "running",
            "earlier dead row must not be reconciled"
        );
        assert_eq!(events_after, events_before);
        fixture
            .registry
            .connection()
            .execute(
                "DELETE FROM processes WHERE process_key IN ('a-dead', 'z-corrupt')",
                [],
            )
            .unwrap();
        service.stop(&mut fixture.registry, 1000).unwrap();
    }
}

#[test]
fn down_cancels_live_task_process_group_and_unblocks_cleanup() {
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::new(&test_sleep(), &["30"], port);
    let service = fixture
        .start("run-down-task-canceling", port)
        .expect("foreground service should start");
    let mut command = Command::new(test_sleep());
    command
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.process_group(0);
    let mut task_child = command.spawn().expect("task process should spawn");
    let task_pid = task_child.id();
    let task_pgid = unsafe { libc::getpgid(task_pid as libc::pid_t) };
    assert!(task_pgid > 0, "task process group should exist");
    let task_process_key = format!(
        "process-{}-task-smoke-{}-{}",
        service.info().run_id,
        task_pid,
        task_pgid
    );
    let task_start_identity = json!({
        "pid": task_pid,
        "pgid": task_pgid,
        "platformStart": null,
        "observedAtNanos": unique_suffix(),
    })
    .to_string();
    seed_process(
        fixture.registry.connection(),
        SeedProcess {
            key: &task_process_key,
            run_id: &service.info().run_id,
            pid: task_pid.into(),
            start_identity: &task_start_identity,
            ..SeedProcess::default()
        },
    );
    commit_slot_marker(
        &fixture.registry,
        &default_state_identity(fixture.admission.common()),
    )
    .expect("slot marker should be written for cleanup proof");

    let report = stop_recorded_processes(&mut fixture.registry, 1000)
        .expect("down should stop service and task");
    let _ = task_child.wait();
    let run_status: Option<String> = fixture.query(
        "SELECT execution_outcome FROM runs WHERE run_id = ?1",
        [&service.info().run_id],
    );
    let service_process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&service.info().process_key],
    );
    let task_process_status: String = fixture.query(
        "SELECT status FROM processes WHERE process_key = ?1",
        [&task_process_key],
    );
    let service_canceled_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.stopped'",
        [],
    );
    let task_canceled_events: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'task.canceled'",
        [],
    );
    let cleanup = clean_marked_state(
        &default_state_identity(fixture.admission.common()),
        &mut fixture.registry,
        CleanupMode::Standard,
    )
    .expect("settled task should not block cleanup");

    assert_eq!(
        report.stopped,
        vec![service.info().process_key.clone(), task_process_key.clone()]
    );
    assert_eq!(
        run_status, None,
        "process cleanup must preserve unknown session outcome"
    );
    assert_eq!(service_process_status, "stopped");
    assert_eq!(task_process_status, "canceled");
    assert_eq!(service_canceled_events, 1);
    assert_eq!(task_canceled_events, 1);
    assert!(
        !process_group_has_non_zombie_member(service.info().pgid),
        "down should empty service process group"
    );
    assert!(
        !process_group_has_non_zombie_member(task_pgid),
        "down should empty task process group"
    );
    assert!(matches!(
        &cleanup,
        nixfied_runtime::state::CleanupOutcome::Deleted { deleted_path, .. }
            if deleted_path.ends_with("runtime-test/dev/0")
    ));
    assert!(!fixture.placement.state_root().exists());
}

#[test]
fn down_escalates_until_owned_process_group_is_empty() {
    let marker = {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "nixfied-survivor-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        path
    };
    let started = temp_marker("nixfied-down-escalate-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut fixture = ServiceFixture::from_value(test_child_service(
        &["term-tree", &started_arg, &marker_arg],
        port,
        port,
    ));
    let service = fixture
        .start("run-down-escalate", port)
        .expect("foreground service should start");
    assert!(
        wait_for_path(&started, Duration::from_secs(3)),
        "TERM-ignoring descendant should start before down"
    );

    let report = stop_recorded_processes(&mut fixture.registry, 200)
        .expect("down should escalate and stop the process group");
    thread::sleep(Duration::from_millis(2300));

    assert_eq!(report.stopped, vec![service.info().process_key.clone()]);
    assert!(
        !marker.exists(),
        "child that ignored TERM should have been killed before touching marker"
    );
    let _ = fs::remove_file(marker);
    let _ = fs::remove_file(started);
}

#[test]
fn task_child_environment_is_hermetic() {
    // This leaf declares no acquired service and needs no listening port.
    let mut value = test_child_manifest(23180, 23180);
    value["tasks"]["smoke"]["requires"] = json!([]);

    value["tasks"]["smoke"]["invocation"]["env"] = json!({
        "DECLARED": "yes",
        "CARGO_TARGET_DIR": "target/verification"
    });
    set_task_run_args(&mut value, &["output", "environment"]);
    let fixture = RuntimeFixture::new(value);
    let output = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .env("NIXFIED_HERMETIC_CANARY", "leaked")
        .output()
        .expect("runtime should execute");
    assert_success(&output);
    let run: Value = serde_json::from_slice(&output.stdout).expect("runtime summary");
    let stdout =
        fs::read_to_string(run["task"]["stdoutPath"].as_str().unwrap()).expect("task stdout log");
    let vars: Vec<&str> = stdout.split(';').filter(|v| !v.is_empty()).collect();
    assert!(
        vars.contains(&"DECLARED=yes"),
        "declared env must be present: {stdout}"
    );
    assert!(
        vars.contains(&"CARGO_TARGET_DIR=target/verification"),
        "ordinary declared tool output path must be present: {stdout}"
    );
    assert!(
        !stdout.contains("NIXFIED_HERMETIC_CANARY"),
        "runtime env must not leak: {stdout}"
    );
    let expected_path = format!("PATH={}", test_child().parent().unwrap().display());
    assert!(
        vars.contains(&expected_path.as_str()),
        "exact tool-root PATH: {stdout}"
    );
    // Exactly the declared env + the runtime-owned variables (PATH, and
    // anything the platform libc injects for every process, e.g. LC_CTYPE on
    // some systems). Assert the strong property directly: no inherited vars.
    for var in &vars {
        let name = var.split('=').next().unwrap_or("");
        assert!(
            matches!(name, "DECLARED" | "CARGO_TARGET_DIR" | "PATH" | "LC_CTYPE"),
            "unexpected child env var {name}: {stdout}"
        );
    }
}

#[test]
fn task_secret_output_is_redacted_from_runtime_owned_sinks() {
    let mut value = test_child_manifest(23180, 23180);
    value["tasks"]["smoke"]["requires"] = json!([]);

    value["secrets"]["api-token"] = json!({
        "secretId": "api-token",
        "source": {
            "kind": "env-var",
            "envVar": "NIXFIED_TEST_TASK_SECRET"
        }
    });
    value["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:api-token}");
    set_task_run_args(&mut value, &["output", "env", "TOKEN"]);
    let fixture = RuntimeFixture::new(value);
    let run = fixture
        .command("run", &["--task", "smoke", "--output", "json"])
        .env("NIXFIED_TEST_TASK_SECRET", "child-visible-secret")
        .output()
        .expect("runtime should run");
    assert_success(&run);
    let runtime_stdout = String::from_utf8_lossy(&run.stdout);
    let runtime_stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!runtime_stdout.contains("child-visible-secret"));
    assert!(!runtime_stderr.contains("child-visible-secret"));
    let output: Value = serde_json::from_slice(&run.stdout).expect("run output should be JSON");
    let stdout_path = output["task"]["stdoutPath"]
        .as_str()
        .expect("task output should link stdout");

    assert_eq!(
        fs::read_to_string(stdout_path).expect("task stdout should read"),
        REDACTION_TOKEN
    );
    assert_tree_excludes(&fixture.state_base, b"child-visible-secret");
}

fn assert_tree_excludes(root: &Path, needle: &[u8]) {
    for file in files_under(root) {
        let bytes = fs::read(&file).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", file.display());
        });
        assert!(
            !bytes.windows(needle.len()).any(|window| window == needle),
            "{} contains secret material",
            file.display()
        );
    }
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            for entry in fs::read_dir(&path).unwrap_or_else(|error| {
                panic!("failed to read dir {}: {error}", path.display());
            }) {
                stack.push(
                    entry
                        .unwrap_or_else(|error| {
                            panic!("failed to read dir entry under {}: {error}", path.display());
                        })
                        .path(),
                );
            }
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn endpoint_less_successor_requires_settlement_and_retains_distinct_history() {
    let mut fixture = endpoint_less_fixture();
    let first = fixture.start_endpoint_less("first-session").unwrap();
    let first_key = first.info().service_instance_id.clone();
    let error = fixture
        .start_endpoint_less("blocked-session")
        .err()
        .expect("an unresolved endpoint-less predecessor must block startup");
    assert_eq!(error.code, ErrorCode::RegistryCorrupt);
    assert_eq!(
        error.message,
        "service startup requires predecessor process settlement"
    );
    let processes: i64 = fixture.query("SELECT count(*) FROM processes", []);
    assert_eq!(processes, 1);
    let starts: i64 = fixture.query(
        "SELECT count(*) FROM events WHERE event_type = 'service.start-intent'",
        [],
    );
    assert_eq!(starts, 1, "rejection precedes prepare/start intent");
    fixture
        .ready(first)
        .unwrap()
        .stop(&mut fixture.registry, 1000)
        .unwrap();

    fixture.begin_run("second-session");
    let second = fixture.start_endpoint_less("second-session").unwrap();
    assert_ne!(first_key, second.info().service_instance_id);
    let predecessor: (String, String, String) = fixture
        .registry
        .connection()
        .query_row(
            "SELECT run_id, service_name, status FROM processes
             WHERE run_id = 'first-session' AND role = 'service'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        predecessor,
        ("first-session".into(), "synthetic".into(), "stopped".into())
    );
    fixture
        .ready(second)
        .unwrap()
        .stop(&mut fixture.registry, 1000)
        .unwrap();
}

#[test]
fn endpoint_less_service_reaches_ready_without_ownership_verification() {
    // An endpoint-less service binds nothing: readiness is its invocation
    // probe answering, and PORT-1 has no claim left to verify (scoped to
    // declared endpoints).
    let mut fixture = endpoint_less_fixture();
    let service = fixture
        .start_endpoint_less("run-endpoint-less")
        .expect("endpoint-less service should start");
    assert!(service.selected_endpoint().is_none());
    let service = fixture
        .ready(service)
        .expect("invocation probe readiness should succeed with no ownership claim");
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

fn endpoint_less_fixture() -> ServiceFixture {
    endpoint_less_fixture_from(synthetic_manifest(&test_sleep(), &["30"], 23180, 23180))
}

fn endpoint_less_fixture_from(mut value: Value) -> ServiceFixture {
    // Endpoint-less: no listener attestation either (effects coherence).
    value["closures"]["synthetic-helper"]["effects"] = json!(["process"]);
    value["services"]["synthetic"]["endpoints"] = json!(null);
    value["services"]["synthetic"]["primaryEndpoint"] = json!(null);
    add_probe_fixture_closure(&mut value);
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"] =
        probe_fixture_invocation(json!(["nixfied-test-fixture", "exit", "0"]));
    value["services"]["synthetic"]["lifecycle"]["ready"]["policy"] =
        json!({"timeoutMs": 1000, "retryIntervalMs": 50, "maxAttempts": 5});
    // Health stays tcp in the fixture; make it an invocation probe too.

    value["services"]["synthetic"]["lifecycle"]["health"]["probe"] =
        probe_fixture_invocation(json!(["nixfied-test-fixture", "exit", "0"]));
    value["services"]["synthetic"]["lifecycle"]["health"]["policy"] =
        json!({"timeoutMs": 1000, "retryIntervalMs": 50, "maxAttempts": 5});
    // The smoke task's bare placeholder has no endpoint to resolve against an
    // endpoint-less primary; the test exercises the service, not the task.
    set_task_run_args(&mut value, &["noop"]);
    // serde: null maps/strings are not "absent" — drop the keys entirely.
    value["services"]["synthetic"]
        .as_object_mut()
        .unwrap()
        .remove("endpoints");
    value["services"]["synthetic"]
        .as_object_mut()
        .unwrap()
        .remove("primaryEndpoint");
    ServiceFixture::from_value(value)
}

struct ServiceFixture {
    _tmp: TempDir,
    admission: RunAdmission,
    placement: nixfied_runtime::state::HostPlacement,
    registry: Registry,
}

impl ServiceFixture {
    /// A later session in the same slot owns its own run directory.
    fn begin_run(&mut self, run_id: &str) {
        self.placement = default_placement(
            self.admission.common().manifest(),
            run_id,
            self.placement.state_base(),
        )
        .expect("layout should derive");
        self.registry
            .authority()
            .claim_run_dir(&self.placement)
            .unwrap();
    }

    // Read durable evidence directly; callers own the SQL and expected result.
    #[track_caller]
    fn query<T: rusqlite::types::FromSql>(&self, sql: &str, params: impl rusqlite::Params) -> T {
        self.registry
            .connection()
            .query_row(sql, params, |row| row.get(0))
            .unwrap_or_else(|error| panic!("query {sql:?} failed: {error}"))
    }

    /// Record `run_id` and start `synthetic` on `endpoint_ports`.
    fn start_with(
        &mut self,
        run_id: &str,
        endpoint_ports: &BTreeMap<String, u16>,
        cancellation: &CancellationToken,
        prepare_runner: Option<PrepareRunner<'_>>,
    ) -> Result<StartingService, RuntimeError> {
        start_fixture_service(
            &self.admission,
            &self.placement,
            &mut self.registry,
            run_id,
            endpoint_ports,
            cancellation,
            prepare_runner,
        )
    }

    fn start(&mut self, run_id: &str, port: u16) -> Result<StartingService, RuntimeError> {
        self.start_with(
            run_id,
            &synthetic_endpoint(port),
            &CancellationToken::new(),
            None,
        )
    }

    fn start_endpoint_less(&mut self, run_id: &str) -> Result<StartingService, RuntimeError> {
        self.start_with(run_id, &BTreeMap::new(), &CancellationToken::new(), None)
    }

    /// A start that must be refused; an unexpected success is stopped first.
    fn start_refused(&mut self, run_id: &str, port: u16, message: &str) -> RuntimeError {
        let result = self.start(run_id, port);
        expect_service_start_failure(result, &mut self.registry, message)
    }

    fn ready(&mut self, service: StartingService) -> Result<ReadyService, ReadinessFailure> {
        service.ready(
            &mut self.registry,
            &CancellationToken::new(),
            &mut || Ok(()),
        )
    }

    fn start_ready(&mut self, run_id: &str, port: u16) -> ReadyService {
        let service = self.start(run_id, port).expect("service should start");
        self.ready(service).expect("service should become ready")
    }

    /// Run the smoke leaf against its ready dependency.
    fn run_smoke(
        &mut self,
        service: &ReadyService,
        cancellation: &CancellationToken,
    ) -> Result<TaskExecution, TaskExecutionError> {
        run_dependent_task_cancellable(
            &self.placement,
            &mut self.registry,
            RunContext::new(
                &runtime_binary(),
                &self.admission,
                &service.info().run_id,
                &self.placement.state_root(),
                &Redactor::from_secrets(self.admission.secrets()),
            ),
            &[service],
            "smoke",
            0,
            self.admission
                .common()
                .execution_manifest()
                .leaf("smoke")
                .expect("smoke task"),
            cancellation,
            SourcePresentation::Shown,
        )
    }

    fn new(executable: &str, start_args: &[&str], port: u16) -> Self {
        Self::from_manifest(manifest(executable, start_args, port))
    }

    fn from_value(value: Value) -> Self {
        Self::from_manifest(serde_json::from_value(value).expect("fixture manifest should parse"))
    }

    fn from_manifest(manifest: Manifest) -> Self {
        Self::from_manifest_in_store(manifest, Path::new("/nix/store"), TempDir::new())
    }

    fn from_manifest_in_store(manifest: Manifest, store_root: &Path, tmp: TempDir) -> Self {
        let admission = admit_fixture_bytes(
            &serde_json::to_vec(&manifest).unwrap(),
            &tmp.path,
            store_root,
        );
        let placement =
            default_placement(&manifest, "run-service", &tmp.path).expect("layout should derive");
        let registry = Registry::open_or_create(
            registry_guard(&placement),
            &RegistryIdentity::for_slot(
                &manifest.project.project_id,
                "dev",
                0,
                &manifest.runtime_abi,
                &manifest.toolchain_id,
            ),
        )
        .expect("registry should open");
        registry.authority().claim_run_dir(&placement).unwrap();
        // Workloads address an existing application root, as after preparation.
        fs::create_dir_all(placement.state_root()).unwrap();
        Self {
            _tmp: tmp,
            admission,
            placement,
            registry,
        }
    }
}

struct StartedSlot<'a> {
    selected: nixfied_runtime::slot::SelectedSlot<'a>,
    placement: nixfied_runtime::state::HostPlacement,
    registry: Registry,
    service: nixfied_runtime::service::ReadyService,
}

impl<'a> StartedSlot<'a> {
    fn start(
        manifest: &'a Manifest,
        admission: &RunAdmission,
        state_base: &Path,
        slot: u32,
        run_id: &str,
        selected_port: u16,
    ) -> Self {
        let selected = select_slot(manifest, Some(slot)).expect("slot should select");
        let placement = slot_placement(manifest, &selected, run_id, state_base)
            .expect("slot placement should derive");
        let identity = StateIdentity::from_selected_slot(admission.common(), &selected);
        let mut registry = open_slot_registry(&placement, manifest, &selected);
        registry.authority().claim_run_dir(&placement).unwrap();
        commit_slot_marker(&registry, &identity).expect("slot marker should be written");
        let service = start_fixture_service(
            admission,
            &placement,
            &mut registry,
            run_id,
            &synthetic_endpoint(selected_port),
            &CancellationToken::new(),
            None,
        )
        .expect("slot service should start");
        let service = service
            .ready(&mut registry, &CancellationToken::new(), &mut || Ok(()))
            .expect("slot service should become ready");
        Self {
            selected,
            placement,
            registry,
            service,
        }
    }
}

#[test]
fn cancellation_after_prepare_settles_startup_and_releases_startup_guard() {
    let port = available_port_window(1);
    let mut fixture = service_fixture_with_prepare(&test_sleep(), &["30"], port);
    let cancellation = CancellationToken::new();
    let prepare_cancellation = cancellation.clone();
    let result = fixture.start_with(
        "run-prepare-canceled",
        &synthetic_endpoint(port),
        &cancellation,
        Some(Box::new(move |_| {
            prepare_cancellation.cancel();
            Ok(())
        })),
    );
    let error = expect_service_start_failure(
        result,
        &mut fixture.registry,
        "cancellation after prepare must prevent spawn",
    );

    assert_eq!(error.code, ErrorCode::Canceled);
    assert_pre_child_settlement(&fixture.registry, "run-prepare-canceled");
    assert_prepared_retry(&mut fixture, "run-after-prepare-canceled", port);
}

#[test]
fn prepare_failure_settles_startup_and_allows_corrected_retry() {
    let port = available_port_window(1);
    let mut fixture = service_fixture_with_prepare(&test_sleep(), &["30"], port);
    let cancellation = CancellationToken::new();
    let result = fixture.start_with(
        "run-prepare-failed",
        &synthetic_endpoint(port),
        &cancellation,
        Some(Box::new(|_| {
            Err(RuntimeError::new(
                ErrorCode::TaskFailed,
                "deterministic prepare failure",
            ))
        })),
    );
    let error = expect_service_start_failure(
        result,
        &mut fixture.registry,
        "prepare failure must prevent spawn",
    );

    assert_eq!(error.code, ErrorCode::TaskFailed);
    assert_pre_child_settlement(&fixture.registry, "run-prepare-failed");
    assert_prepared_retry(&mut fixture, "run-after-prepare-failed", port);
}

#[test]
fn spawn_failure_after_prepare_settles_startup_and_allows_restored_retry() {
    let executable_dir = TempDir::new();
    let executable = executable_dir.path.join("bin/sleep");
    fs::create_dir(executable_dir.path.join("bin")).unwrap();
    restore_executable_fixture(&executable);
    let port = available_port_window(1);
    let mut value = synthetic_manifest(executable.to_str().unwrap(), &["30"], port, port);
    value["closures"]
        .as_object_mut()
        .unwrap()
        .remove("fixture-probe");
    let mut probe = value["services"]["synthetic"]["lifecycle"]["start"]["invocation"].clone();
    probe["run"] = json!(["sleep", "0"]);
    value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["readyProbe"] = probe.clone();
    value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["healthProbe"] = probe;
    add_endpoint_prepare(&mut value, &["0"]);
    let mut fixture = ServiceFixture::from_manifest_in_store(
        serde_json::from_value(value).unwrap(),
        &executable_dir.path,
        TempDir::new(),
    );
    let removed_executable = executable.clone();
    let cancellation = CancellationToken::new();
    let result = fixture.start_with(
        "run-spawn-failed",
        &synthetic_endpoint(port),
        &cancellation,
        Some(Box::new(move |_| {
            fs::remove_file(&removed_executable).map_err(|error| {
                RuntimeError::new(
                    ErrorCode::TaskFailed,
                    format!("failed to remove spawn fixture: {error}"),
                )
            })
        })),
    );
    let error = expect_service_start_failure(
        result,
        &mut fixture.registry,
        "removed executable must fail at the real spawn boundary",
    );

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let (count, status): (i64, String) = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*), min(status) FROM processes WHERE run_id='run-spawn-failed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((count, status.as_str()), (1, "failed"));
    assert_eq!(
        fixture.query::<i64>("SELECT count(*) FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key WHERE p.ownership = 'unresolved'", []),
        0
    );
    assert_eq!(
        fixture.query::<Option<String>>(
            "SELECT execution_outcome FROM runs WHERE run_id='run-spawn-failed'",
            []
        ),
        None
    );

    restore_executable_fixture(&executable);
    assert_prepared_retry(&mut fixture, "run-after-spawn-failed", port);
}

fn manifest(executable: &str, start_args: &[&str], port: u16) -> Manifest {
    serde_json::from_value(synthetic_manifest(executable, start_args, port, port))
        .expect("fixture manifest should parse")
}

/// SEAM-1: the runtime never invokes `nix`. Poison `PATH` with failing
/// `nix`/`nix-store`/`nix-build` shims that touch a sentinel, then drive the full
/// lifecycle (check -> run -> clean) through the binary and assert the sentinel is
/// never created. Service/task execs run by absolute store path, so poisoning
/// `PATH` cannot starve them - only a nix invocation would trip the sentinel.
#[test]
fn runtime_drives_full_lifecycle_without_invoking_nix() {
    let port = available_port_window(1);

    let mut value = test_child_manifest(port, port);
    set_task_run_args(&mut value, &["connect", "127.0.0.1", "${port}", "close"]);
    let fixture = RuntimeFixture::new(value);
    let tmp = &fixture.tmp;

    // A PATH whose nix tools fail loudly and record that they were called.
    let fake_bin = tmp.path.join("fake-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir should be created");
    let sentinel = tmp.path.join("nix-was-invoked");
    for tool in ["nix", "nix-store", "nix-build"] {
        let shim = fake_bin.join(tool);
        std::os::unix::fs::symlink(fixture_program("nixfied-test-poison"), &shim)
            .expect("poison shim should be linked");
    }
    let poisoned_path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut dirs = vec![fake_bin.clone()];
            dirs.extend(std::env::split_paths(&existing));
            std::env::join_paths(dirs).expect("PATH should join")
        }
        None => fake_bin.as_os_str().to_os_string(),
    };

    let run_binary = |command: &str, extra: &[&str]| -> Output {
        fixture
            .command(command, extra)
            .env("PATH", &poisoned_path)
            .env("NIXFIED_TEST_POISON_SENTINEL", &sentinel)
            .output()
            .expect("runtime should run")
    };

    let check = run_binary("check", &[]);
    assert_success(&check);

    let run = run_binary("run", &["--task", "smoke", "--timeout-ms", "5000"]);
    assert_success(&run);
    let run_stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run_stderr.contains("  ok smoke (smoke)"));
    assert!(run_stderr.contains("  result: ok 1 passed, 0 failed in "));
    assert!(run_stderr.contains("  run-summary: "));
    assert!(run_stderr.contains("  logs: "));
    assert!(
        run.stdout.is_empty(),
        "summary output mode should not write JSON stdout: {}",
        String::from_utf8_lossy(&run.stdout)
    );

    let run_json_output = run_binary(
        "run",
        &[
            "--task",
            "smoke",
            "--timeout-ms",
            "5000",
            "--output",
            "json",
        ],
    );
    assert_success(&run_json_output);
    let run_json_stderr = String::from_utf8_lossy(&run_json_output.stderr);
    assert!(
        !run_json_stderr.contains("  result: "),
        "json output mode should not write human summary stderr: {run_json_stderr}"
    );
    let run_json: Value =
        serde_json::from_slice(&run_json_output.stdout).expect("run output should be valid JSON");

    let run_both_output = run_binary(
        "run",
        &[
            "--task",
            "smoke",
            "--timeout-ms",
            "5000",
            "--output",
            "both",
        ],
    );
    assert_success(&run_both_output);
    assert!(
        String::from_utf8_lossy(&run_both_output.stderr)
            .contains("  result: ok 1 passed, 0 failed in "),
        "both output mode should include the human summary"
    );
    let _: Value =
        serde_json::from_slice(&run_both_output.stdout).expect("both output should include JSON");

    assert_eq!(
        run_json["task"]["success"],
        json!(true),
        "smoke task should succeed: {run_json}"
    );
    assert!(
        run_json["task"]["durationMs"].as_u64().is_some(),
        "task output should carry durationMs: {run_json}"
    );
    assert!(
        run_json["durationMs"].as_u64().is_some(),
        "run output should carry durationMs: {run_json}"
    );
    let run_summary_path = run_json["runSummaryPath"]
        .as_str()
        .expect("run output should link run summary");
    let run_summary: Value =
        serde_json::from_slice(&fs::read(run_summary_path).expect("run summary should read"))
            .expect("run summary should parse");
    assert!(
        run_summary["durationMs"].as_u64().is_some(),
        "run summary should carry durationMs: {run_summary}"
    );
    // Run-scoped data ends with its settled session; evidence and history stay.
    let state_root = fixture.state_base.join("data/runtime-test/dev/0");
    assert!(
        !state_root.exists(),
        "run-scoped state should be deleted by session finalization"
    );
    let settled: (String, String, String) = fixture
        .registry()
        .query_row(
            "SELECT r.execution_outcome, r.finalization, c.status FROM runs r, cleanups c",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        settled,
        (
            "succeeded".to_string(),
            "complete".to_string(),
            "completed".to_string()
        )
    );

    let clean = run_binary("clean", &[]);
    assert_success(&clean);
    let clean: Value = serde_json::from_slice(&clean.stdout).unwrap();
    assert_eq!(clean["result"], "absent");

    assert!(
        !sentinel.exists(),
        "runtime invoked a nix tool (sentinel was touched), violating SEAM-1"
    );
}

/// Bug #2: a selection that starts no services (an environment of only
/// service-less tasks) must still leave a durable `runs` row — the run path used
/// to create that row only inside the per-service loop, so a task-only run left
/// none and `mark_task_finished`'s `UPDATE runs` was a silent no-op.
#[test]
fn composite_run_keys_evidence_by_step_path() {
    // A composite referencing the same leaf twice runs it twice, with logs,
    // summaries, and registry rows keyed by the distinct step paths.
    let port = available_port_window(1);

    let mut value = test_child_manifest(port, port);
    value["tasks"]["smoke"]["requires"] = json!([]);

    set_task_run_args(&mut value, &["exit", "0"]);
    value["tasks"]["twice"] = json!({
        "kind": "composite",
        "steps": {
            "again": { "task": "smoke", "dependsOn": ["first"] },
            "first": { "task": "smoke" }
        }
    });
    let fixture = RuntimeFixture::new(value);
    let state_base = &fixture.state_base;

    let run = fixture
        .command(
            "run",
            &[
                "--task",
                "twice",
                "--timeout-ms",
                "5000",
                "--output",
                "json",
            ],
        )
        .output()
        .expect("runtime should run");
    assert_success(&run);
    let output: Value = serde_json::from_slice(&run.stdout).expect("run output should be JSON");
    let step_paths: Vec<&str> = output["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .map(|task| task["stepPath"].as_str().expect("stepPath"))
        .collect();
    assert_eq!(step_paths, vec!["twice.first", "twice.again"]);

    // Repeated nodes retain distinct occurrence files and their original step paths.
    for path in [0, 1] {
        assert!(
            find_named(state_base, &format!("task.{path}.stdout.log")).is_some(),
            "missing stdout log for {path}"
        );
        assert!(
            find_named(state_base, &format!("summary.{path}.json")).is_some(),
            "missing summary for {path}"
        );
    }

    // Two registry task rows, keyed by step-path process keys.
    let conn = fixture.registry();
    let keys: Vec<String> = conn
        .prepare("SELECT process_key FROM processes ORDER BY process_key")
        .expect("statement prepares")
        .query_map([], |row| row.get(0))
        .expect("query runs")
        .collect::<Result<_, _>>()
        .expect("rows collect");
    assert!(
        keys.iter().any(|key| key.contains("task-twice.first"))
            && keys.iter().any(|key| key.contains("task-twice.again")),
        "process keys must carry step paths: {keys:?}"
    );
}

#[test]
fn nested_composite_cancellation_terminates_leaf_process_group() {
    let marker = temp_marker("nixfied-nested-composite-cancel-survivor");
    let started = temp_marker("nixfied-nested-composite-cancel-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let port = available_port_window(1);
    let mut value = test_child_manifest(port, port);
    value["tasks"]["smoke"]["requires"] = json!([]);

    set_task_run_args(&mut value, &["term-tree", &started_arg, &marker_arg]);
    value["tasks"]["inner"] = json!({
        "kind": "composite",
        "steps": {
            "wait": { "task": "smoke" }
        }
    });
    value["tasks"]["outer"] = json!({
        "kind": "composite",
        "steps": {
            "inner": { "task": "inner" }
        }
    });
    let fixture = RuntimeFixture::new(value);

    let child = fixture
        .command("run", &["--task", "outer", "--output", "json"])
        .spawn()
        .expect("runtime should spawn");
    wait_for_task_process_row(&fixture.state_base, Duration::from_secs(5));
    assert!(
        wait_for_path(&started, Duration::from_secs(3)),
        "nested task descendant should start before cancellation"
    );
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "SIGTERM should reach runtime process");

    let output = wait_for_child_output(child, Duration::from_secs(10));

    assert_eq!(output.status.code(), Some(27), "CANCELED exit code");
    let error = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("CANCELED"));
    assert_eq!(error["details"]["failedNodeId"], json!("outer.inner.wait"));
    let conn = fixture.registry();
    let (task_status, pgid): (String, i32) = conn
        .query_row(
            "SELECT status, pgid FROM processes WHERE role = 'task'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("task process row should exist");
    assert_eq!(task_status, "canceled");
    assert!(
        !process_group_has_non_zombie_member(pgid),
        "canceled nested leaf process group should be empty"
    );
    thread::sleep(Duration::from_millis(2300));
    assert!(
        !marker.exists(),
        "descendant that ignored TERM should have been killed before touching marker"
    );
    let _ = fs::remove_file(marker);
    let _ = fs::remove_file(started);
}

#[test]
fn repeated_prepares_and_root_keep_distinct_occurrence_evidence() {
    for (fail_on, expected_count, expected_code) in [(0, 4, 0), (2, 2, 30), (4, 4, 30)] {
        let tmp = TempDir::new();
        let counter = tmp.path.join("child-counter");
        let port = available_port_window(2);
        let mut value = test_child_manifest(port, port);
        value["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
        add_service_clone(&mut value, "worker", LISTEN_HOLD, &[]);
        for service in ["synthetic", "worker"] {
            value["services"][service]["lifecycle"]["prepare"] = json!({"task": "prep"});
        }
        let fail_on = fail_on.to_string();
        let occurrence = ["output", "occurrence", counter.to_str().unwrap(), &fail_on];
        set_task_run_args(&mut value, &occurrence);
        value["tasks"]["smoke"]["requires"] = json!(["synthetic", "worker"]);
        add_task_clone(&mut value, "prep", &[], &occurrence);
        value["tasks"]["pipeline"] = json!({
            "kind": "composite", "steps": {"again": {"task": "prep"}, "final": {"task": "smoke", "dependsOn": ["again"]}}
        });

        let fixture = RuntimeFixture::new(&value);
        let state = &fixture.state_base;
        let output = fixture.output("run", &["--task", "pipeline", "--output", "json"]);
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Value = serde_json::from_slice(
            &fs::read(find_named(state, "run-summary.json").unwrap()).unwrap(),
        )
        .unwrap();
        let tasks = summary["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), expected_count);
        for (index, task) in tasks.iter().enumerate() {
            assert_eq!(task["taskId"], ["prep", "prep", "prep", "smoke"][index]);
            assert_eq!(
                task["stepPath"],
                ["prep", "prep", "pipeline.again", "pipeline.final"][index]
            );
            for (field, stream) in [("stdoutPath", "stdout"), ("stderrPath", "stderr")] {
                let path = Path::new(task[field].as_str().unwrap());
                assert_eq!(
                    path.file_name().unwrap(),
                    format!("task.{index}.{stream}.log").as_str()
                );
                assert_eq!(
                    fs::read_to_string(path).unwrap(),
                    format!("{stream} occurrence {}\n", index + 1)
                );
            }
            let path = Path::new(task["summaryPath"].as_str().unwrap());
            assert_eq!(
                path.file_name().unwrap(),
                format!("summary.{index}.json").as_str()
            );
            let recorded: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert_eq!(&recorded, task);
        }
        assert_eq!(
            summary["nodes"].as_array().unwrap().len(),
            expected_count.saturating_sub(2)
        );
    }
}

#[test]
fn completed_prepare_evidence_survives_success_and_later_failures() {
    for (prepare_exit, listener_host, expected_code) in [
        (0, "127.0.0.1", 0),
        (0, "0.0.0.0", 26),
        (7, "127.0.0.1", 30),
    ] {
        let port = available_port_window(1);
        let mut value =
            test_child_service(&["listen", listener_host, "${port}", "hold"], port, port);
        set_task_run_args(&mut value, &["exit", "0"]);
        for (name, code) in [("before", 0), ("after", prepare_exit)] {
            add_task_clone(&mut value, name, &[], &["exit", &code.to_string()]);
        }
        value["tasks"]["prep"] = json!({
            "kind": "composite", "steps": {
                "first": {"task": "before"},
                "second": {"task": "after", "dependsOn": ["first"]}
            }
        });
        value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({"task": "prep"});
        value["services"]["synthetic"]["lifecycle"]["ready"]["policy"]["maxAttempts"] = json!(2);

        let fixture = RuntimeFixture::new(&value);
        let state_base = &fixture.state_base;
        let output = fixture.output("run", &["--task", "smoke", "--output", "json"]);
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Value = serde_json::from_slice(
            &fs::read(find_named(state_base, "run-summary.json").expect("aggregate summary"))
                .unwrap(),
        )
        .unwrap();
        let tasks = summary["tasks"].as_array().unwrap();
        let ids: Vec<_> = tasks
            .iter()
            .map(|task| task["taskId"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            if expected_code == 0 {
                vec!["before", "after", "smoke"]
            } else {
                vec!["before", "after"]
            }
        );
        assert_eq!(tasks[0]["success"], true);
        assert_eq!(tasks[1]["exitCode"], prepare_exit);
        assert_eq!(summary["success"], expected_code == 0);
    }
}

#[test]
fn composite_starts_full_service_union_before_first_node() {
    let port = available_port_window(2);
    let worker_port = port.checked_add(1).expect("two-port window should fit");
    let worker_port_arg = worker_port.to_string();
    let mut value = test_child_manifest(port, port);
    value["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(worker_port);

    add_service_clone(&mut value, "worker", LISTEN_HOLD, &[]);
    set_task_run_args(
        &mut value,
        &["connect", "127.0.0.1", &worker_port_arg, "close"],
    );
    add_task_clone(
        &mut value,
        "needs-worker",
        &["worker"],
        &["connect", "127.0.0.1", "${port}", "close"],
    );
    value["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "steps": {
            "first": { "task": "smoke" },
            "second": { "task": "needs-worker", "dependsOn": ["first"] }
        }
    });
    let fixture = RuntimeFixture::new(value);

    let output = fixture
        .command("run", &["--task", "pipeline", "--output", "json"])
        .output()
        .expect("runtime should run");

    assert_success(&output);
    let run: Value = serde_json::from_slice(&output.stdout).expect("run output should be JSON");
    assert_eq!(run["services"].as_array().map(Vec::len), Some(2));
    assert_eq!(run["nodes"][0]["nodeId"], json!("pipeline.first"));
    assert_eq!(run["nodes"][1]["nodeId"], json!("pipeline.second"));
    assert!(run["nodes"][0]["durationMs"].as_u64().is_some());
    assert!(run["nodes"][1]["durationMs"].as_u64().is_some());
    assert_eq!(run["tasks"][0]["success"], json!(true));
    assert_eq!(run["tasks"][1]["success"], json!(true));
    assert!(run["tasks"][0]["durationMs"].as_u64().is_some());
    assert!(run["tasks"][1]["durationMs"].as_u64().is_some());
}

#[test]
fn task_only_run_records_a_durable_runs_row() {
    let port = available_port_window(1);

    let mut value = test_child_manifest(port, port);
    // The environment starts no services and runs only the service-less task.
    value["tasks"]["smoke"]["requires"] = json!([]);

    set_task_run_args(&mut value, &["exit", "0"]);
    let fixture = RuntimeFixture::new(value);

    let run = fixture
        .command("run", &["--task", "smoke", "--timeout-ms", "5000"])
        .output()
        .expect("runtime should run");
    assert_success(&run);

    let conn = fixture.registry();
    let (count, status): (i64, String) = conn
        .query_row(
            "SELECT count(*), coalesce(max(execution_outcome), '') FROM runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("runs row should be queryable");
    assert_eq!(
        count, 1,
        "a task-only run must leave exactly one durable runs row"
    );
    assert_eq!(
        status, "succeeded",
        "the session owner must record its execution outcome"
    );
}

/// Bug #1: a task whose exec declares `stdin: inherit` must receive the operator's
/// stdin, not a closed `/dev/null`. The runtime inherits its own stdin to the task
/// process, so a sentinel piped to `nixfied-runtime run` reaches the task.
#[test]
fn inherit_stdin_reaches_a_task_process() {
    use std::io::Write;

    let port = available_port_window(1);

    let mut value = test_child_manifest(port, port);
    value["tasks"]["smoke"]["requires"] = json!([]);

    set_task_run_args(&mut value, &["output", "stdin"]);
    value["tasks"]["smoke"]["invocation"]["stdin"] = json!("inherit");
    let fixture = RuntimeFixture::new(value);
    let state_base = &fixture.state_base;

    let mut child = fixture
        .command("run", &["--task", "smoke", "--timeout-ms", "5000"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("runtime should spawn");
    let sentinel = "nixfied-inherited-stdin-marker\n";
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(sentinel.as_bytes())
        .expect("sentinel should be written to runtime stdin");
    let out = child.wait_with_output().expect("runtime should complete");
    assert_success(&out);

    let log = find_named(state_base, "task.0.stdout.log").expect("task stdout log should exist");
    let captured = fs::read_to_string(&log).expect("task stdout log should be readable");
    assert!(
        captured.contains("nixfied-inherited-stdin-marker"),
        "task with stdin=inherit did not receive the operator's stdin: {captured:?}"
    );
}

/// Wait until a task process row is committed.
fn wait_for_task_process_row(state_base: &Path, timeout: Duration) {
    poll_until(timeout, "a task process row", || {
        try_registry_ro(state_base)?
            .query_row(
                "SELECT count(*) FROM processes WHERE role = 'task'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .filter(|count| *count > 0)
    });
}

#[test]
fn failed_composite_run_writes_failure_summary() {
    let port = available_port_window(1);
    let mut value = test_child_manifest(port, port);
    // A 0-service composite whose single node fails: the run must leave the
    // same aggregate evidence a success does, linked from the error.
    value["tasks"]["smoke"]["requires"] = json!([]);

    set_task_run_args(&mut value, &["exit", "3"]);
    value["tasks"]["wf"] = json!({
        "kind": "composite",
        "steps": {
            "fail-node": { "task": "smoke" }
        }
    });
    let fixture = RuntimeFixture::new(value);

    let output = fixture
        .command("run", &["--task", "wf"])
        .output()
        .expect("runtime run should execute");

    assert_eq!(output.status.code(), Some(30), "TaskFailed exit code");
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_text.contains("  fail wf.fail-node (smoke)"));
    assert!(stderr_text.contains("    stderr: "));
    assert!(stderr_text.contains("  result: fail 0 passed, 1 failed in "));
    assert!(stderr_text.contains("  run-summary: "));
    assert!(stderr_text.contains("  logs: "));
    assert!(
        output.stdout.is_empty(),
        "summary failure output should not write stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !stderr_text.contains(r#""code":"TASK_FAILED""#),
        "summary failure output should not append JSON error payload: {stderr_text}"
    );
    let summary_path =
        find_named(&fixture.state_base, "run-summary.json").expect("run summary should exist");
    let summary: Value =
        serde_json::from_slice(&fs::read(&summary_path).expect("run summary should exist"))
            .expect("run summary should parse");
    assert_eq!(summary["success"], json!(false));
    let nodes = summary["nodes"].as_array().expect("nodes should be array");
    assert!(summary["durationMs"].as_u64().is_some());
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["nodeId"], json!("wf.fail-node"));
    assert_eq!(nodes[0]["success"], json!(false));
    assert_eq!(nodes[0]["exitCode"], json!(3));
    assert!(nodes[0]["durationMs"].as_u64().is_some());
    let stdout_path = summary["tasks"][0]["stdoutPath"]
        .as_str()
        .expect("summary must link the failed task stdout");
    assert!(PathBuf::from(stdout_path).exists());

    let json_output = fixture
        .command_at(
            &fixture.tmp.path.join("state-json"),
            "run",
            &["--task", "wf", "--output", "json"],
        )
        .output()
        .expect("runtime run should execute");

    assert_eq!(json_output.status.code(), Some(30), "TaskFailed exit code");
    assert!(
        json_output.stdout.is_empty(),
        "json failure output should not write stdout: {}",
        String::from_utf8_lossy(&json_output.stdout)
    );
    let json_stderr_text = String::from_utf8_lossy(&json_output.stderr);
    assert!(
        !json_stderr_text.contains("  result: "),
        "json failure output should not include human summary: {json_stderr_text}"
    );
    let error: Value = stderr_json(&json_output.stderr);
    assert_eq!(error["code"], json!("TASK_FAILED"));
    let details = &error["details"];
    assert!(details["runId"].is_string(), "error must carry the run id");
    assert!(details["stateBase"].is_string());
    assert!(details["stateRoot"].is_string());
    assert!(details["registryDir"].is_string());
    assert!(details["registryPath"].is_string());
    assert!(details["logsDir"].is_string());
    assert_eq!(details["failedNodeId"], json!("wf.fail-node"));
    let json_summary_path = details["runSummaryPath"]
        .as_str()
        .expect("error must link the run summary");
    assert!(PathBuf::from(json_summary_path).exists());

    let both_output = fixture
        .command_at(
            &fixture.tmp.path.join("state-both"),
            "run",
            &["--task", "wf", "--output", "both"],
        )
        .output()
        .expect("runtime run should execute");

    assert_eq!(both_output.status.code(), Some(30), "TaskFailed exit code");
    assert!(
        both_output.stdout.is_empty(),
        "both failure output should not write stdout: {}",
        String::from_utf8_lossy(&both_output.stdout)
    );
    let both_stderr_text = String::from_utf8_lossy(&both_output.stderr);
    assert!(both_stderr_text.contains("error: TASK_FAILED:"));
    assert!(both_stderr_text.contains("state-root: "));
    assert!(both_stderr_text.contains("registry-dir: "));
    assert!(both_stderr_text.contains("registry: "));
    assert!(both_stderr_text.contains("logs: "));
    assert!(both_stderr_text.contains("run-summary: "));
    let both_json: Value = serde_json::from_str(
        both_stderr_text
            .lines()
            .last()
            .expect("both stderr should end with JSON"),
    )
    .expect("both stderr final line should be JSON");
    assert_eq!(both_json["code"], json!("TASK_FAILED"));
    assert!(both_json["details"]["registryPath"].is_string());
}

#[test]
fn service_failure_before_any_node_writes_failed_summary() {
    for fail_health in [false, true] {
        let port = available_port_window(1);
        // The service dies before ever listening, so the run fails on the ready
        // probe with zero node results — the summary must still record failure.
        let mut value = if fail_health {
            let mut value = test_child_manifest(port, port);
            add_probe_fixture_closure(&mut value);
            value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["healthProbe"] =
                probe_fixture_invocation(json!(["nixfied-test-fixture", "exit", "7"]));
            value["services"]["synthetic"]["lifecycle"]["health"]["policy"] =
                json!({"timeoutMs": 1000, "retryIntervalMs": 10, "maxAttempts": 1});
            value
        } else {
            test_child_service(&["exit", "1"], port, port)
        };
        value["tasks"]["wf"] = json!({
            "kind": "composite",
            "steps": {
                "never-runs": { "task": "smoke" }
            }
        });
        let fixture = RuntimeFixture::new(value);

        let output = fixture
            .command("run", &["--task", "wf", "--output", "json"])
            .output()
            .expect("runtime run should execute");

        assert!(!output.status.success(), "the run must fail");
        let error: Value = stderr_json(&output.stderr);
        let summary_path = error["details"]["runSummaryPath"]
            .as_str()
            .expect("error must link the run summary");
        let summary: Value =
            serde_json::from_slice(&fs::read(summary_path).expect("run summary should exist"))
                .expect("run summary should parse");
        assert_eq!(
            summary["success"],
            json!(false),
            "a run that failed before any node must not summarize as success"
        );
        assert!(summary["durationMs"].as_u64().is_some());
        assert_eq!(summary["nodes"].as_array().map(Vec::len), Some(0));
        assert_eq!(error["details"]["failedService"], json!("synthetic"));
        if fail_health {
            assert_eq!(error["code"], json!("READINESS_TIMEOUT"));
            let services = summary["services"].as_array().expect("service evidence");
            assert_eq!(services.len(), 1);
            assert_eq!(services[0]["serviceId"], json!("synthetic"));
            assert_eq!(services[0]["selectedEndpoint"]["port"], json!(port));
            let connection =
                rusqlite::Connection::open(error["details"]["registryPath"].as_str().unwrap())
                    .unwrap();
            let (ready_events, failed_events, process_status): (i64, i64, String) = connection
                .query_row(
                    "SELECT (SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'),
                    (SELECT count(*) FROM events WHERE event_type = 'service.failed'),
                    (SELECT status FROM processes WHERE process_key = ?1)",
                    [services[0]["processKey"].as_str().unwrap()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(
                (ready_events, failed_events, process_status),
                (1, 1, "failed".into())
            );
            assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
        }
    }
}

#[test]
fn control_registry_identity_mismatch_reports_human_scoped_recovery() {
    let manifest: Manifest =
        serde_json::from_value(test_child_service(&["block"], 23180, 23180)).unwrap();
    let fixture = RuntimeFixture::new(&manifest);

    let selected_slot = select_slot(&manifest, None).expect("slot should select");
    let placement = slot_placement(&manifest, &selected_slot, "setup", &fixture.state_base)
        .expect("placement should derive");
    let registry = open_slot_registry(&placement, &manifest, &selected_slot);
    // Fault injection changes stored identity; the writer constructor rejects
    // an incoherent requested identity before creating the database.
    registry
        .connection()
        .execute("UPDATE registry_meta SET project_id = 'other-project'", [])
        .unwrap();
    registry.close().unwrap();

    let output = fixture.output("ps", &[]);

    assert_eq!(output.status.code(), Some(21), "StateUnowned exit code");
    assert!(
        output.stdout.is_empty(),
        "failed control command should not write stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_text.contains("error: STATE_UNOWNED:"));
    assert!(stderr_text.contains("state-root: "));
    assert!(stderr_text.contains("registry-dir: "));
    assert!(stderr_text.contains("registry: "));
    assert!(stderr_text.contains("expected: projectId="));
    assert!(stderr_text.contains("found: projectId=other-project"));
    assert!(stderr_text.contains("mismatch: projectId"));
    assert!(stderr_text.contains("hint: do not delete the whole Nixfied state base"));
    assert!(
        !stderr_text.contains('{'),
        "default control errors should be human text, not JSON: {stderr_text}"
    );
}

fn test_child_listener_fixture(port: u16) -> ServiceFixture {
    ServiceFixture::from_value(test_child_manifest(port, port))
}

fn test_child_wildcard_listener_fixture(port: u16) -> ServiceFixture {
    ServiceFixture::from_value(test_child_service(
        &["listen", "0.0.0.0", "${port}", "hold"],
        port,
        port,
    ))
}

fn service_fixture_with_prepare(
    executable: &str,
    start_args: &[&str],
    port: u16,
) -> ServiceFixture {
    ServiceFixture::from_manifest(service_manifest_with_prepare(executable, start_args, port))
}

fn service_manifest_with_prepare(executable: &str, start_args: &[&str], port: u16) -> Manifest {
    let mut value = synthetic_manifest(executable, start_args, port, port);
    add_endpoint_prepare(&mut value, &["0"]);
    serde_json::from_value(value).expect("prepare fixture should deserialize")
}

fn expect_service_start_failure(
    result: Result<StartingService, RuntimeError>,
    registry: &mut Registry,
    message: &str,
) -> RuntimeError {
    match result {
        Ok(service) => {
            let error = RuntimeError::new(ErrorCode::TaskFailed, message);
            service.finalize_failed_start(registry, 1000, error);
            panic!("{message}");
        }
        Err(error) => error,
    }
}

fn assert_prepared_retry(fixture: &mut ServiceFixture, run_id: &str, port: u16) {
    fixture.begin_run(run_id);
    let retry = fixture
        .start_with(
            run_id,
            &synthetic_endpoint(port),
            &CancellationToken::new(),
            Some(Box::new(|_| Ok(()))),
        )
        .expect("a corrected run should reacquire the endpoint");
    // The sleep fixture cannot answer readiness; settle it as a failed start.
    let error = RuntimeError::new(ErrorCode::ReadinessTimeout, "retry fixture is not probed");
    retry.finalize_failed_start(&mut fixture.registry, 1000, error);
}

fn assert_pre_child_settlement(registry: &Registry, run_id: &str) {
    let (run_status, process_count, service_count, port_count): (Option<String>, i64, i64, i64) =
        registry
            .connection()
            .query_row(
                "SELECT (SELECT execution_outcome FROM runs WHERE run_id = ?1),
                    (SELECT count(*) FROM processes WHERE run_id = ?1),
                    (SELECT count(*) FROM sqlite_master WHERE name = 'services'), (SELECT count(*) FROM ports)",
                [run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(
        run_status, None,
        "startup failure cannot settle the session"
    );
    assert_eq!((process_count, service_count, port_count), (0, 0, 0));
}

fn restore_executable_fixture(path: &Path) {
    fs::copy(test_sleep(), path).expect("spawn fixture should copy");
    let mut permissions = fs::metadata(path)
        .expect("spawn fixture metadata should read")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("spawn fixture should be executable");
}

#[derive(Debug, PartialEq, Eq)]
struct LifecycleEvent {
    event_type: String,
    class: String,
    terminal_result: Option<String>,
    error_code: Option<String>,
}

fn lifecycle_events(registry: &Registry) -> Vec<LifecycleEvent> {
    registry
        .connection()
        .prepare(
            "
            SELECT event_type, payload_json
            FROM events
            WHERE event_type LIKE 'service.lifecycle.%'
            ORDER BY rowid
            ",
        )
        .expect("lifecycle statement should prepare")
        .query_map([], |row| {
            let event_type: String = row.get(0)?;
            let payload_json: String = row.get(1)?;
            let payload: Value =
                serde_json::from_str(&payload_json).expect("lifecycle payload should parse");
            Ok(LifecycleEvent {
                event_type,
                class: payload["class"]
                    .as_str()
                    .expect("class should be present")
                    .to_string(),
                terminal_result: payload["terminalResult"]
                    .as_str()
                    .map(|value| value.to_string()),
                error_code: payload["errorCode"].as_str().map(|value| value.to_string()),
            })
        })
        .expect("lifecycle events should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("lifecycle events should collect")
}

fn stored_process_start_identity(registry: &Registry, process_key: &str) -> String {
    registry
        .connection()
        .query_row(
            "SELECT start_identity FROM processes WHERE process_key = ?1",
            [process_key],
            |row| row.get(0),
        )
        .expect("process start identity should query")
}

fn mark_started_service_escape(
    registry: &mut Registry,
    service: &nixfied_runtime::service::ServiceInfo,
    pid: u32,
    start_identity: &str,
    payload_json: &str,
) {
    let transaction = registry
        .connection()
        .unchecked_transaction()
        .expect("failed to begin escaped-process fixture transaction");
    let process_rows = transaction
        .execute(
            "
            UPDATE processes
            SET status = 'escaped', start_identity = ?6
            WHERE process_key = ?1
              AND pid = ?2
              AND pgid = ?3
              AND run_id = ?4
              AND role = 'service'
              AND status IN ('running', 'ready')
              AND EXISTS (
                SELECT 1 FROM runs
                WHERE run_id = ?4 AND computed_manifest_hash = ?5
              )
            ",
            rusqlite::params![
                service.process_key,
                pid,
                service.pgid,
                service.run_id,
                service.computed_manifest_hash,
                start_identity,
            ],
        )
        .expect("failed to inject escaped process evidence");
    assert_eq!(process_rows, 1);
    let event_rows = transaction
        .execute(
            "
            UPDATE events
            SET event_type = 'service.proc-escape', payload_json = ?2
            WHERE seq = (
              SELECT max(seq) FROM events
              WHERE event_type = 'service.starting' AND process_key = ?1
            )
            ",
            rusqlite::params![service.process_key, payload_json],
        )
        .expect("failed to inject escaped event evidence");
    assert_eq!(event_rows, 1);
    transaction.commit().expect("escaped fixture should commit");
}

fn wait_for_process_exit(pid: u32) {
    poll_until(Duration::from_secs(2), "the process to exit", || {
        (!process_is_non_zombie(pid)).then_some(())
    });
}

fn process_is_non_zombie(pid: u32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps should inspect process status");
    if !output.status.success() {
        return false;
    }
    let stat = String::from_utf8_lossy(&output.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

#[cfg(target_os = "linux")]
fn platform_start_for_test(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields_after_comm = stat.rsplit_once(") ")?.1;
    let start_time_ticks = fields_after_comm.split_whitespace().nth(19)?;
    Some(format!("linux-start-ticks:{start_time_ticks}"))
}

#[test]
fn diamond_service_dependency_starts_once_per_session() {
    let port = available_port_window(3);
    let mut value = test_child_manifest(port, port);
    value["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 2);
    let listen = ["listen", "127.0.0.1", "${port}", "hold"];
    add_service_clone(&mut value, "left", &listen, &["synthetic"]);
    add_service_clone(&mut value, "right", &listen, &["synthetic"]);
    add_task_clone(&mut value, "via-left", &["left"], &["exit", "0"]);
    add_task_clone(&mut value, "via-right", &["right"], &["exit", "0"]);
    value["tasks"]["diamond"] = json!({
        "kind": "composite",
        "steps": {
            "a": { "task": "via-left" },
            "b": { "task": "via-right" }
        }
    });
    let fixture = RuntimeFixture::new(value);

    let mut shared = Vec::new();
    for _ in 0..2 {
        let output = fixture.output("run", &["--task", "diamond", "--output", "json"]);
        assert_success(&output);
        let run: Value = serde_json::from_slice(&output.stdout).expect("run output is JSON");
        assert_eq!(run["services"].as_array().map(Vec::len), Some(3));
        let connection = registry_ro(&fixture.state_base);
        let run_id: String = connection
            .query_row(
                "SELECT run_id FROM runs ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let rows: Vec<(String, String, String, String)> = connection
            .prepare(
                "SELECT service_name, process_key, status, ownership FROM processes
                 WHERE role = 'service' AND run_id = ?1 ORDER BY service_name",
            )
            .unwrap()
            .query_map([&run_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let names: Vec<&str> = rows.iter().map(|row| row.0.as_str()).collect();
        assert_eq!(
            names,
            ["left", "right", "synthetic"],
            "the shared dependency starts exactly once in its session"
        );
        for (name, _, status, ownership) in &rows {
            assert_eq!(
                (status.as_str(), ownership.as_str()),
                ("stopped", "settled"),
                "{name}"
            );
        }
        shared.push(rows[2].1.clone());
    }
    assert_ne!(
        shared[0], shared[1],
        "each session starts its own shared dependency"
    );
    let total: i64 = registry_ro(&fixture.state_base)
        .query_row(
            "SELECT count(*) FROM processes WHERE role = 'service' AND service_name = 'synthetic'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn finished_composite_step_does_not_finalize_the_running_session() {
    let port = available_port_window(1);
    let barrier = TempDir::new();
    let reached = barrier.path.join("second-step-running");
    let release = barrier.path.join("second-step-release");
    let mut value = test_child_manifest(port, port);
    set_task_run_args(&mut value, &["exit", "0"]);
    add_task_clone(
        &mut value,
        "gate",
        &[],
        &[
            "prepare",
            reached.to_str().unwrap(),
            release.to_str().unwrap(),
        ],
    );
    value["tasks"]["pipeline"] = json!({
        "kind": "composite",
        "steps": {
            "first": { "task": "smoke" },
            "second": { "task": "gate", "dependsOn": ["first"] }
        }
    });
    let fixture = RuntimeFixture::new(value);
    let child = fixture
        .command("run", &["--task", "pipeline", "--output", "json"])
        .spawn()
        .expect("runtime should spawn");
    assert!(
        wait_for_path(&reached, Duration::from_secs(10)),
        "the second step should start"
    );

    let connection = registry_ro(&fixture.state_base);
    let (outcome, finalization, output): (Option<String>, String, String) = connection
        .query_row(
            "SELECT execution_outcome, finalization, output FROM runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (outcome, finalization.as_str(), output.as_str()),
        (None, "unfinished", "unsealed"),
        "a finished step never settles the session"
    );
    let steps: Vec<String> = connection
        .prepare("SELECT status FROM processes WHERE role = 'task' ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert_ne!(steps[0], "running", "the first step finished: {steps:?}");
    assert_eq!(steps[1], "running", "{steps:?}");
    drop(connection);

    let ps = fixture.output("ps", &[]);
    assert_success(&ps);
    let report: Value = serde_json::from_slice(&ps.stdout).expect("ps output is JSON");
    let live = report["processes"]
        .as_array()
        .expect("ps processes array")
        .iter()
        .filter(|process| process["live"] == json!(true))
        .count();
    assert_eq!(live, 2, "ps reports the running session: {report}");

    fs::write(&release, b"go").unwrap();
    let output = wait_for_child_output(child, Duration::from_secs(15));
    assert_success(&output);
    let (outcome, finalization, output): (Option<String>, String, String) =
        registry_ro(&fixture.state_base)
            .query_row(
                "SELECT execution_outcome, finalization, output FROM runs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(
        (outcome.as_deref(), finalization.as_str(), output.as_str()),
        (Some("succeeded"), "complete", "sealed")
    );
}

#[test]
fn owner_killed_during_readiness_leaves_an_obligation_the_successor_settles() {
    let port = available_port_window(1);
    let child_program = test_child();
    let mut value = exec_probe_fixture_value(
        child_program.to_str().unwrap(),
        &[
            "listen",
            "127.0.0.1",
            "${port}",
            "ready-on-marker",
            "${stateDir}/listener-bound",
            "${stateDir}/ready-ack",
            "${stateDir}/ready-flag",
        ],
        port,
        json!(["exists", "${stateDir}/ready-flag"]),
        400,
    );
    set_task_run_args(&mut value, &["exit", "0"]);
    let fixture = RuntimeFixture::new(value);
    let mut owner = fixture
        .command("run", &["--task", "smoke"])
        .spawn()
        .expect("runtime should spawn");
    let bound = wait_for_named(
        &fixture.state_base,
        "listener-bound",
        Duration::from_secs(10),
    )
    .expect("service should bind");
    // The service is registered and running; readiness waits on the ack.
    let (pgid, status): (i32, String) = registry_ro(&fixture.state_base)
        .query_row(
            "SELECT pgid, status FROM processes WHERE role = 'service'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        status, "running",
        "the service is between registration and readiness"
    );
    assert_eq!(
        unsafe { libc::kill(owner.id() as libc::pid_t, libc::SIGKILL) },
        0
    );
    owner.wait().unwrap();
    assert!(
        process_group_has_non_zombie_member(pgid),
        "the orphaned service outlives its killed owner"
    );
    assert!(!bound.with_file_name("ready-flag").exists());

    let down = fixture.output("down", &[]);
    assert_success(&down);
    assert!(
        !process_group_has_non_zombie_member(pgid),
        "the successor terminates the recorded service group"
    );
    let connection = registry_ro(&fixture.state_base);
    let (status, ownership): (String, String) = connection
        .query_row(
            "SELECT status, ownership FROM processes WHERE role = 'service'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(ownership, "settled", "status {status}");
    assert!(
        !matches!(status.as_str(), "starting" | "running" | "ready"),
        "{status}"
    );
    let open: i64 = connection
        .query_row(
            "SELECT count(*) FROM ports ep JOIN processes p ON p.process_key = ep.owner_process_key WHERE p.ownership = 'unresolved'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(open, 0, "the successor releases the service endpoints");
    let finalization: String = connection
        .query_row("SELECT finalization FROM runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(finalization, "complete");
}

/// `anchor` exits zero once `synthetic` (which starts after it and so stops
/// before it) receives its stop signal, which it ignores.
fn teardown_failure_manifest(stopping: &Path, task_args: &[&str]) -> Manifest {
    let port = available_port_window(2);
    let started = temp_marker("nixfied-teardown-started");
    let mut value = test_child_service(
        &[
            "term-block",
            "127.0.0.1",
            "${port}",
            started.to_str().unwrap(),
            stopping.to_str().unwrap(),
        ],
        port,
        port,
    );
    value["placement"]["slotPlacements"]["0"]["candidatePorts"]["end"] = json!(port + 1);
    value["services"]["synthetic"]["lifecycle"]["stop"]["timeoutMs"] = json!(3000);
    value["services"]["synthetic"]["connectsTo"] = json!(["anchor"]);
    add_service_clone(
        &mut value,
        "anchor",
        &[
            "listen",
            "127.0.0.1",
            "${port}",
            "exit-zero-on-marker",
            stopping.to_str().unwrap(),
        ],
        &[],
    );
    set_task_run_args(&mut value, task_args);
    serde_json::from_value(value).expect("teardown manifest should parse")
}

/// The service row a CLI `ps` reports.
fn ps_service_row(fixture: &RuntimeFixture) -> Value {
    let ps = fixture.output("ps", &[]);
    assert_success(&ps);
    let report: Value = serde_json::from_slice(&ps.stdout).expect("ps stdout should be JSON");
    report["processes"]
        .as_array()
        .expect("ps processes should be an array")
        .iter()
        .find(|process| process["serviceInstanceId"].is_string())
        .expect("service process should be reported")
        .clone()
}

fn service_statuses(state_base: &Path) -> Vec<(String, String, String)> {
    registry_ro(state_base)
        .prepare(
            "SELECT service_name, status, ownership FROM processes
             WHERE role = 'service' ORDER BY service_name",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn service_exit_during_ordered_teardown_fails_the_run() {
    let stopping = temp_marker("nixfied-teardown-stopping");
    let fixture = RuntimeFixture::new(teardown_failure_manifest(&stopping, &["exit", "0"]));
    let output = fixture
        .command(
            "run",
            &[
                "--task",
                "smoke",
                "--output",
                "json",
                "--timeout-ms",
                "5000",
            ],
        )
        .output()
        .expect("runtime should run");
    let _ = fs::remove_file(&stopping);
    assert!(
        !output.status.success(),
        "an unexpected exit during teardown fails the run"
    );
    let error = stderr_json(&output.stderr);
    assert!(error.to_string().contains("exited before stop"), "{error}");
    assert_eq!(
        service_statuses(&fixture.state_base),
        [
            ("anchor".into(), "failed".into(), "settled".into()),
            ("synthetic".into(), "stopped".into(), "settled".into()),
        ]
    );
}

#[test]
fn service_exit_during_canceled_teardown_is_a_failure_not_a_cancellation() {
    let stopping = temp_marker("nixfied-canceled-teardown-stopping");
    let fixture = RuntimeFixture::new(teardown_failure_manifest(&stopping, &["block"]));
    let child = fixture
        .command(
            "run",
            &[
                "--task",
                "smoke",
                "--output",
                "json",
                "--timeout-ms",
                "3000",
            ],
        )
        .spawn()
        .expect("runtime should spawn");
    wait_for_task_process_row(&fixture.state_base, Duration::from_secs(10));
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let output = wait_for_child_output(child, Duration::from_secs(15));
    let _ = fs::remove_file(&stopping);
    // The observed failure outranks the cancellation, which stays a cause.
    let error = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("PROC_ESCAPE"), "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("exited before stop")),
        "{error}"
    );
    assert_eq!(error["causes"][0]["code"], json!("CANCELED"), "{error}");
    assert_eq!(
        service_statuses(&fixture.state_base),
        [
            ("anchor".into(), "failed".into(), "settled".into()),
            ("synthetic".into(), "canceled".into(), "settled".into()),
        ],
        "the dependency exited on its own before its cancellation signal"
    );
}

#[test]
fn down_ends_when_a_successor_recovers_the_chosen_dead_session() {
    let port = available_port_window(1);
    let started = temp_marker("nixfied-down-successor-started");
    let stopping = temp_marker("nixfied-down-successor-stopping");
    let mut value = test_child_service(
        &[
            "term-block",
            "127.0.0.1",
            "${port}",
            started.to_str().unwrap(),
            stopping.to_str().unwrap(),
        ],
        port,
        port,
    );
    value["services"]["synthetic"]["lifecycle"]["stop"]["timeoutMs"] = json!(2000);
    clear_task_deadline(&mut value, "smoke");
    set_task_run_args(&mut value, &["block"]);
    let fixture = RuntimeFixture::new(value);
    let run = |timeout: &str| {
        fixture
            .command("run", &["--task", "smoke", "--output", "json"])
            .args(["--timeout-ms", timeout])
            .spawn()
            .unwrap()
    };
    let running_task = |state_base: &Path| -> String {
        poll_until(Duration::from_secs(5), "the task to start", || {
            try_registry_ro(state_base)?
                .query_row(
                    "SELECT run_id FROM processes WHERE role = 'task' AND status = 'running'",
                    [],
                    |row| row.get(0),
                )
                .ok()
        })
    };

    let owner = run("5000");
    let chosen = running_task(&fixture.state_base);
    assert_eq!(
        unsafe { libc::kill(owner.id() as libc::pid_t, libc::SIGKILL) },
        0
    );
    let _ = wait_for_child_output(owner, Duration::from_secs(5));

    // The successor holds the slot while its recovery waits on the orphaned
    // service, which ignores its stop signal.
    let successor = run("2000");
    assert!(wait_for_path(&stopping, Duration::from_secs(5)));
    let down = fixture
        .command("down", &["--timeout-ms", "10000"])
        .spawn()
        .unwrap();
    let down = wait_for_child_output(down, Duration::from_secs(12));
    let successor_run = running_task(&fixture.state_base);
    let _ = unsafe { libc::kill(successor.id() as libc::pid_t, libc::SIGTERM) };
    let successor = wait_for_child_output(successor, Duration::from_secs(10));
    let _ = fs::remove_file(&started);
    let _ = fs::remove_file(&stopping);

    assert_success(&down);
    let report: Value = serde_json::from_slice(&down.stdout).unwrap();
    assert_eq!(
        report,
        json!({"stopped": [], "stale": []}),
        "the successor, not down, recovered the chosen session"
    );
    assert_ne!(chosen, successor_run);
    let (outcome, finalization): (Option<String>, String) = registry_ro(&fixture.state_base)
        .query_row(
            "SELECT execution_outcome, finalization FROM runs WHERE run_id = ?1",
            [&chosen],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (outcome.as_deref(), finalization.as_str()),
        (Some("interrupted"), "complete")
    );
    assert_eq!(
        successor.status.code(),
        Some(27),
        "down never reached the successor"
    );
}
