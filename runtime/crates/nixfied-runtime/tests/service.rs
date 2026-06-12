use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nixfied_model::{DirtyPolicy, Model, SourceMode};
use nixfied_runtime::cancellation::CancellationToken;
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::service::registry::{
    PortReservation, RunRecord, TaskProcessRecord, record_task_started, reserve_service_start,
};
use nixfied_runtime::service::{
    RunContext, compute_service_identity, run_dependent_task, run_dependent_task_cancellable,
    service_address_hash, service_instance_id, wait_for_tcp_probe,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, clean_marked_state, commit_slot_marker, derive_host_placement,
    derive_host_placement_for_slot, materialize_run_roots,
};
use nixfied_runtime::{Admission, AdmittedSource, ErrorCode};
use serde_json::{Value, json};

use nixfied_runtime::control::{down_owned_process_groups, ps};

mod common;
use common::*;

#[test]
fn starts_foreground_service_in_owned_process_group_and_records_before_ready() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23180);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-service",
        23180,
    )
    .expect("foreground service should start");

    assert_eq!(service.pid as i32, service.pgid);
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("process row should exist");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service row should exist");
    let probe_ready_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");

    assert_eq!(process_status, "running");
    assert_eq!(service_status, "starting");
    assert_eq!(probe_ready_events, 0);
    assert!(
        fixture
            .placement
            .logs_dir
            .join("service.synthetic.stdout.log")
            .exists()
    );
    assert!(
        fixture
            .placement
            .logs_dir
            .join("service.synthetic.stderr.log")
            .exists()
    );
    let command_json: Value = fixture
        .registry
        .connection()
        .query_row(
            "SELECT command_json FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| {
                let payload: String = row.get(0)?;
                Ok(serde_json::from_str(&payload).expect("command JSON should parse"))
            },
        )
        .expect("process command should exist");
    assert_eq!(
        command_json["cwd"],
        json!(
            fixture
                .admission
                .require_source()
                .unwrap()
                .observed_root
                .to_string_lossy()
        )
    );

    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
    let stopped_process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE run_id = 'run-service'",
            [],
            |row| row.get(0),
        )
        .expect("stopped process row should exist");
    let stopped_service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_name = 'synthetic'",
            [],
            |row| row.get(0),
        )
        .expect("stopped service row should exist");
    assert_eq!(stopped_process_status, "stopped");
    assert_eq!(stopped_service_status, "stopped");
    // Shutdown records the actual signal mechanism, not a fabricated exec terminal.
    let stop_signal: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT json_extract(payload_json, '$.signal') FROM events
             WHERE event_type = 'service.stop.signaled'",
            [],
            |row| row.get(0),
        )
        .expect("stop should record a signal event");
    assert_eq!(stop_signal, "TERM");
}

#[test]
fn service_start_rejects_exec_cwd_escape() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23180);
    fixture
        .model
        .services
        .get_mut("synthetic")
        .expect("fixture service should exist")
        .lifecycle
        .start
        .invocation
        .cwd = "..".to_string();
    fixture.relower();

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-cwd-escape",
        23180,
    ) {
        Ok(service) => {
            let _ = service.stop(&mut fixture.registry, 1000);
            panic!("escaped exec cwd should fail before process start");
        }
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::SourceMismatch);
}

#[test]
fn readiness_probe_marks_ready_only_after_endpoint_ownership() {
    let Some(python) = python3_path() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let script = python_listener_script();
    let mut fixture = ServiceFixture::new(python, &["-c", script, "${port}"], port);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-ready",
        port,
    )
    .expect("foreground service should start");

    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("owned listener should satisfy readiness");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let port_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM ports WHERE owner_process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("port status should query");
    let verified_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'port.owner-verified'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");

    assert_eq!(service_status, "probe-ready");
    assert_eq!(port_status, "active");
    assert_eq!(verified_events, 1);
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
    let released_ports: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM ports WHERE status = 'released'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");
    assert_eq!(released_ports, 1);
}

#[test]
fn lifecycle_events_follow_declared_class_order_and_clean_terminal() {
    let Some(python) = python3_path() else {
        return;
    };
    let port = 45000 + (unique_suffix() % 1000) as u16;
    let script = python_listener_script();
    let mut fixture = ServiceFixture::new(python, &["-c", script, "${port}"], port);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-lifecycle-order",
        port,
    )
    .expect("foreground service should start");

    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("service should become ready");
    service
        .check_health(&mut fixture.registry)
        .expect("service health should pass");
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");

    let selected = select_slot(&fixture.model, None).expect("default slot should select");
    let identity = StateIdentity::from_selected_slot(&fixture.model, &fixture.admission, &selected);
    commit_slot_marker(&fixture.placement, &identity).expect("slot marker should be written");
    let cleanup = run_synthetic_service_clean_for_slot(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        &selected,
    )
    .expect("clean lifecycle should succeed");

    assert!(cleanup.deleted_path.ends_with("runtime-test/dev/0"));
    assert!(!fixture.placement.state_root.exists());
    assert_eq!(
        lifecycle_events(&fixture.registry),
        vec![
            LifecycleEvent {
                event_type: "service.lifecycle.started".to_string(),
                class: "prepare".to_string(),
                terminal_result: None,
                error_code: None,
            },
            LifecycleEvent {
                event_type: "service.lifecycle.terminal".to_string(),
                class: "prepare".to_string(),
                terminal_result: Some("prepared".to_string()),
                error_code: None,
            },
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
    assert_eq!(cleanup_events, vec!["cleanup.intent", "cleanup.deleted"]);
}

#[test]
fn external_listener_does_not_satisfy_endpoint_ownership() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], port);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-external-listener",
        port,
    )
    .expect("foreground service should start");

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("external listener must not satisfy ownership");

    assert_eq!(error.code, ErrorCode::PortUnverifiable);
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let ready_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");

    assert_eq!(service_status, "failed");
    assert_eq!(ready_events, 0);
    drop(listener);
}

#[test]
fn wildcard_listener_does_not_satisfy_loopback_endpoint_ownership() {
    let Some(python) = python3_path() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let script = python_wildcard_listener_script();
    let mut fixture = ServiceFixture::new(python, &["-c", script, "${port}"], port);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-wildcard-listener",
        port,
    )
    .expect("foreground service should start");

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("wildcard listener must not satisfy declared loopback endpoint");

    assert_eq!(error.code, ErrorCode::PortUnverifiable);
    let ready_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    assert_eq!(ready_events, 0);
    assert_eq!(service_status, "failed");
}

#[test]
fn slot_one_service_uses_slot_placement_port_window() {
    let tmp = TempDir::new();
    let mut value = fixture_model("/bin/sleep", &["30"], 23180);
    add_slot_one(&mut value, 23280, 23280);
    let model: Model = serde_json::from_value(value).expect("fixture model should parse");
    let admission = admission(&model, &tmp.path);
    let selected_slot = select_slot(&model, Some(1)).expect("slot 1 should select");
    let placement = derive_host_placement_for_slot(&model, &selected_slot, "run-slot-1", &tmp.path)
        .expect("slot 1 layout should derive");
    materialize_run_roots(&placement).expect("roots should materialize");
    let mut registry = Registry::open_or_create(
        placement.registry_path(),
        &RegistryIdentity::for_slot(
            &model.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &model.runtime_abi,
            &model.toolchain_id,
        ),
    )
    .expect("registry should open");

    let service = start_synthetic_service_for_slot(
        &admission,
        &placement,
        &mut registry,
        "run-slot-1",
        &selected_slot,
        23280,
    )
    .expect("slot 1 service should accept slot placement port");

    assert_eq!(service.selected_endpoint.port, 23280);
    assert_eq!(placement.state_root, tmp.path.join("runtime-test/dev/1"));
    service
        .stop(&mut registry, 1000)
        .expect("service should stop");
    assert_registry_tables_scoped_to_slot(
        &registry,
        1,
        &["runs", "services", "processes", "ports", "events"],
    );
}

#[test]
fn two_slots_keep_services_state_and_controls_isolated() {
    let python = python3_path()
        .map(str::to_string)
        .or_else(python3_from_path)
        .expect("python3 is required for the M1 slot isolation proof");
    let tmp = TempDir::new();
    let mut value = fixture_model(&python, &["-c", python_listener_script(), "${port}"], 23210);
    add_slot_one(&mut value, 23310, 23320);
    let model: Model = serde_json::from_value(value).expect("fixture model should parse");
    let admission = admission(&model, &tmp.path);

    let mut slot0 = StartedSlot::start(&model, &admission, &tmp.path, 0, "run-slot-0", 23210);
    let mut slot1 = StartedSlot::start(&model, &admission, &tmp.path, 1, "run-slot-1", 23310);

    assert_ne!(slot0.placement.state_root, slot1.placement.state_root);
    assert_ne!(
        slot0.placement.registry_path(),
        slot1.placement.registry_path()
    );
    assert_ne!(slot0.placement.run_dir, slot1.placement.run_dir);
    assert_ne!(slot0.placement.logs_dir, slot1.placement.logs_dir);
    assert_ne!(slot0.placement.artifacts_dir, slot1.placement.artifacts_dir);
    assert_ne!(slot0.placement.summary_path, slot1.placement.summary_path);
    assert_ne!(
        slot0.service.selected_endpoint.port,
        slot1.service.selected_endpoint.port
    );
    assert_ne!(
        slot0.service.service_instance_id,
        slot1.service.service_instance_id
    );

    let slot0_ps = ps(&mut slot0.registry).expect("slot 0 ps should reconcile");
    let slot1_ps = ps(&mut slot1.registry).expect("slot 1 ps should reconcile");
    assert!(slot0_ps.processes.iter().any(|process| process.live));
    assert!(slot1_ps.processes.iter().any(|process| process.live));

    down_owned_process_groups(&mut slot0.registry, 1000).expect("slot 0 down should stop slot 0");
    drop(slot0.service);
    let slot0_after_down = ps(&mut slot0.registry).expect("slot 0 ps should reconcile after down");
    let slot1_after_down = ps(&mut slot1.registry).expect("slot 1 ps should remain live");
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

    let slot0_identity = StateIdentity::from_selected_slot(&model, &admission, &slot0.selected);
    clean_marked_state(
        &slot0.placement.state_base,
        &slot0.placement.state_root,
        &slot0_identity,
        &mut slot0.registry,
    )
    .expect("slot 0 cleanup should succeed after down");
    assert!(!slot0.placement.state_root.exists());
    assert!(slot1.placement.state_root.exists());

    let slot1_identity = StateIdentity::from_selected_slot(&model, &admission, &slot1.selected);
    clean_marked_state(
        &slot1.placement.state_base,
        &slot1.placement.state_root,
        &slot0_identity,
        &mut slot1.registry,
    )
    .expect_err("slot 0 identity must not clean slot 1 state");
    assert!(slot1.placement.state_root.exists());

    slot1
        .service
        .stop(&mut slot1.registry, 1000)
        .expect("slot 1 service should stop");
    clean_marked_state(
        &slot1.placement.state_base,
        &slot1.placement.state_root,
        &slot1_identity,
        &mut slot1.registry,
    )
    .expect("slot 1 cleanup should succeed after stop");
}

#[test]
fn service_instance_identity_includes_selected_slot() {
    let mut value = fixture_model("/bin/sleep", &["30"], 23180);
    add_slot_one(&mut value, 23280, 23280);
    let model: Model = serde_json::from_value(value).expect("fixture model should parse");
    let service = model
        .services
        .get("synthetic")
        .expect("fixture has synthetic service");

    let identity = compute_service_identity(service, &model.state, &model.target);
    let slot_0_address = service_address_hash(&model.project.project_id, "dev", 0, "synthetic");
    let slot_1_address = service_address_hash(&model.project.project_id, "dev", 1, "synthetic");
    let slot_0_instance = service_instance_id(&slot_0_address, &identity);
    let slot_1_instance = service_instance_id(&slot_1_address, &identity);

    assert_ne!(slot_0_address, slot_1_address);
    assert_ne!(slot_0_instance, slot_1_instance);
}

#[test]
fn dependent_task_runs_after_owned_service_is_ready() {
    let Some(python) = python3_path() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let script = python_listener_script();
    let mut fixture = ServiceFixture::new(python, &["-c", script, "${port}"], port);
    set_smoke_args(
        &mut fixture.model,
        &["-c", "import sys; sys.stdout.write('task-ok')"],
    );
    fixture.relower();
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-task",
        port,
    )
    .expect("foreground service should start");
    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("owned listener should become ready");

    let task = run_dependent_task(
        &fixture.placement,
        &mut fixture.registry,
        RunContext::from_service(&service),
        &[&service],
        fixture
            .admission
            .execution_model
            .tasks
            .get("smoke")
            .expect("smoke task"),
    )
    .expect("ready dependent task should run");

    assert!(task.success);
    assert_eq!(task.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(&task.stdout_path).expect("stdout should read"),
        "task-ok"
    );
    assert!(task.stderr_path.exists());
    assert!(task.summary_path.exists());
    let task_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type IN ('task.running','task.succeeded')",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&task.process_key],
            |row| row.get(0),
        )
        .expect("process status should query");
    let run_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM runs WHERE run_id = 'run-task'",
            [],
            |row| row.get(0),
        )
        .expect("run status should query");

    assert_eq!(task_events, 2);
    assert_eq!(process_status, "succeeded");
    assert_eq!(run_status, "task-succeeded");
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn dependent_task_refuses_to_run_before_service_ready() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23186);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-task-not-ready",
        23186,
    )
    .expect("foreground service should start");

    let error = run_dependent_task(
        &fixture.placement,
        &mut fixture.registry,
        RunContext::from_service(&service),
        &[&service],
        fixture
            .admission
            .execution_model
            .tasks
            .get("smoke")
            .expect("smoke task"),
    )
    .expect_err("task should wait for probe-ready service");

    assert_eq!(error.code, ErrorCode::DependencyUnavailable);
    let task_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type LIKE 'task.%'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    assert_eq!(task_events, 0);
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn readiness_probe_times_out_without_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let fixture = ServiceFixture::new("/bin/sleep", &["1"], port);
    let nixfied_runtime::execution::Probe::Tcp(probe) = &fixture
        .admission
        .execution_model
        .services
        .get("synthetic")
        .expect("fixture has service")
        .ready
        .probe
    else {
        panic!("fixture ready probe should be tcp");
    };

    let error = wait_for_tcp_probe(probe, "127.0.0.1", port, &CancellationToken::new())
        .expect_err("closed endpoint should time out");

    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
}

#[test]
fn tcp_probe_supports_ipv6_loopback_hosts() {
    // `LoopbackHost` admits `::1`; the probe must build a connectable address
    // from it rather than the unparseable concatenation `::1:<port>`.
    let listener = TcpListener::bind("[::1]:0").expect("ipv6 loopback listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    let fixture = ServiceFixture::new("/bin/sleep", &["1"], port);
    let nixfied_runtime::execution::Probe::Tcp(probe) = &fixture
        .admission
        .execution_model
        .services
        .get("synthetic")
        .expect("fixture has service")
        .ready
        .probe
    else {
        panic!("fixture ready probe should be tcp");
    };

    wait_for_tcp_probe(probe, "::1", port, &CancellationToken::new())
        .expect("probe should connect to the ipv6 loopback listener");
}

#[test]
fn readiness_timeout_stops_started_service_and_records_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], port);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-readiness-timeout",
        port,
    )
    .expect("service should initially start");

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("readiness should time out and clean up");

    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("process status should query");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let failure_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.failed'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    assert_eq!(process_status, "failed");
    assert_eq!(service_status, "failed");
    assert_eq!(failure_events, 1);
}

/// The fixture model with an exec-based ready probe: a /bin/sh exec whose
/// args are supplied per test. The probe's operation is bound on the closure,
/// as admission requires.
fn exec_probe_fixture_value(
    executable: &str,
    start_args: &[&str],
    port: u16,
    probe_args: Value,
    probe_attempts: u32,
) -> Value {
    let mut value = fixture_model(executable, start_args, port);
    add_probe_shell_closure(&mut value, "service.synthetic.ready");
    let mut run = vec![json!("sh")];
    run.extend(probe_args.as_array().expect("probe args").iter().cloned());
    value["services"]["synthetic"]["lifecycle"]["ready"]["probe"] = json!({
        "kind": "exec", "invocation": probe_shell_invocation(Value::Array(run)),
        "timeoutMs": 1000, "retryIntervalMs": 50, "maxAttempts": probe_attempts
    });
    value
}

/// A `/bin/sh` tool closure for invocation probes, bound to the given op.
fn add_probe_shell_closure(value: &mut Value, operation: &str) {
    let target = value["target"]["closureSystem"].clone();
    value["closures"]["probe-shell"] = json!({
        "kind": "executable", "storePath": "/bin", "executable": "/bin/sh",
        "targetSystem": target,
        "operationBindings": [operation],
        "requiresExecutable": true, "effects": ["process"]
    });
}

fn probe_shell_invocation(run: Value) -> Value {
    json!({
        "tools": ["probe-shell"],
        "run": run,
        "executable": "/bin/sh",
        "env": {},
        "codebaseId": "main",
        "cwd": ".",
        "stdin": "null",
        "timeoutMs": 30000
    })
}

#[test]
fn exec_ready_probe_gates_on_flag_and_marks_ready() {
    let Some(python) = python3_path() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    // The service binds its endpoint immediately but signals readiness only via
    // the flag file it touches afterwards — exactly what a tcp probe cannot see.
    let script = "import socket, sys, time, pathlib; s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('127.0.0.1', int(sys.argv[1]))); s.listen(16); time.sleep(0.3); pathlib.Path(sys.argv[2]).touch(); time.sleep(30)";
    let value = exec_probe_fixture_value(
        python,
        &["-c", script, "${port}", "${stateDir}/ready-flag"],
        port,
        json!([
            "-c",
            "exec test -e \"$1\"",
            "probe",
            "${stateDir}/ready-flag"
        ]),
        60,
    );
    let mut fixture = ServiceFixture::from_value(value);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-exec-probe",
        port,
    )
    .expect("service should start");

    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("exec probe should succeed once the flag appears");

    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    assert_eq!(service_status, "probe-ready");
    assert!(
        fixture
            .placement
            .logs_dir
            .join("lifecycle.ready.probe.stdout.log")
            .exists(),
        "probe attempts should leave captured output"
    );
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn exec_ready_probe_failure_times_out_and_records_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let value = exec_probe_fixture_value("/bin/sleep", &["30"], port, json!(["-c", "exit 7"]), 3);
    let mut fixture = ServiceFixture::from_value(value);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-exec-probe-fail",
        port,
    )
    .expect("service should initially start");

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("a failing exec probe should time out and clean up");

    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
    assert!(
        error.message.contains("exited with code 7"),
        "failure should carry the last attempt's exit code: {}",
        error.message
    );
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    assert_eq!(service_status, "failed");
}

#[test]
fn exec_health_probe_failure_records_failed() {
    let Some(python) = python3_path() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    // The service is ready (tcp) but never healthy: the failed run must leave
    // service.failed evidence, not a clean stopped/completed registry state.
    let mut value = fixture_model(python, &["-c", python_listener_script(), "${port}"], port);
    add_probe_shell_closure(&mut value, "service.synthetic.health");
    value["services"]["synthetic"]["lifecycle"]["health"]["probe"] = json!({
        "kind": "exec",
        "invocation": probe_shell_invocation(json!(["sh", "-c", "exit 7"])),
        "timeoutMs": 1000, "retryIntervalMs": 50, "maxAttempts": 2
    });
    let mut fixture = ServiceFixture::from_value(value);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-exec-health-fail",
        port,
    )
    .expect("service should start");
    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("service should become ready");

    let error = service
        .check_health(&mut fixture.registry)
        .expect_err("a failing health probe should fail the service");

    assert_ne!(error.code, ErrorCode::Canceled);
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    assert_eq!(service_status, "failed");
    let run_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM runs WHERE run_id = 'run-exec-health-fail'",
            [],
            |row| row.get(0),
        )
        .expect("run status should query");
    assert_eq!(run_status, "service-failed");
}

#[test]
fn cancellation_interrupts_readiness_and_terminates_service_group() {
    let marker = temp_marker("nixfied-cancel-survivor");
    let marker_arg = marker.to_string_lossy().to_string();
    let script = "trap 'exit 0' TERM; /bin/sh -c 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30' child \"$1\" & wait";
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut fixture = ServiceFixture::new(
        "/bin/sh",
        &["-c", script, "parent", marker_arg.as_str()],
        port,
    );
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-readiness-canceled",
        port,
    )
    .expect("service should initially start");
    let pgid = service.pgid;
    let cancellation = CancellationToken::new();
    let canceler = cancellation.clone();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        canceler.cancel();
    });

    let error = service
        .wait_for_probe_ready_cancellable(&mut fixture.registry, &cancellation)
        .expect_err("readiness should be canceled");
    handle.join().expect("canceler should join");
    service
        .cancel(&mut fixture.registry, 200, "test readiness cancellation")
        .expect("canceled service should be terminated");
    thread::sleep(Duration::from_millis(2300));
    let report = ps(&mut fixture.registry).expect("ps should reconcile canceled service");
    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == service.process_key)
        .expect("service process should be reported");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");
    let cancel_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type IN ('service.canceling','service.canceled')",
            [],
            |row| row.get(0),
        )
        .expect("events should query");

    assert_eq!(error.code, ErrorCode::Canceled);
    assert!(!observed.live);
    assert_eq!(observed.registry_status, "canceled");
    assert_eq!(service_status, "canceled");
    assert_eq!(lease_status, "canceled");
    assert_eq!(cancel_events, 2);
    assert!(
        !process_group_has_non_zombie_member(pgid),
        "canceled service process group should be empty"
    );
    assert!(
        !marker.exists(),
        "child that ignored TERM should have been killed before touching marker"
    );
    let _ = fs::remove_file(marker);
}

#[test]
fn cancellation_interrupts_task_and_terminates_task_group() {
    let python = python3_path()
        .map(str::to_string)
        .or_else(python3_from_path)
        .expect("python3 is required for task cancellation proof");
    let marker = temp_marker("nixfied-cancel-task-survivor");
    let marker_arg = marker.to_string_lossy().to_string();
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut fixture =
        ServiceFixture::new(&python, &["-c", python_listener_script(), "${port}"], port);
    set_smoke_args(
        &mut fixture.model,
        &[
            "-c",
            "import signal, subprocess, sys; signal.signal(signal.SIGTERM, lambda *_: sys.exit(0)); subprocess.Popen(['/bin/sh', '-c', 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30', 'child', sys.argv[1]]).wait()",
            &marker_arg,
        ],
    );
    fixture.relower();
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-task-canceled",
        port,
    )
    .expect("foreground service should start");
    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("owned listener should become ready");
    let cancellation = CancellationToken::new();
    let canceler = cancellation.clone();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        canceler.cancel();
    });

    let error = run_dependent_task_cancellable(
        &fixture.placement,
        &mut fixture.registry,
        RunContext::from_service(&service),
        &[&service],
        "smoke",
        fixture
            .admission
            .execution_model
            .tasks
            .get("smoke")
            .expect("smoke task"),
        &cancellation,
    )
    .expect_err("task should be canceled");
    handle.join().expect("canceler should join");
    thread::sleep(Duration::from_millis(2300));
    let report = ps(&mut fixture.registry).expect("ps should reconcile canceled task");
    let task_observations = report
        .processes
        .iter()
        .filter(|process| process.service_instance_id.is_none())
        .collect::<Vec<_>>();
    let task_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE service_instance_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("task status should query");
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");
    let task_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type IN ('task.canceling','task.canceled')",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let summary: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .placement
                .summary_path
                .with_file_name("summary.smoke.json"),
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
    assert_eq!(lease_status, "canceled");
    assert_eq!(task_events, 2);
    assert_eq!(summary["canceled"], json!(true));
    let _ = fs::remove_file(marker);
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn task_timeout_records_failed_summary_and_terminates_task_group() {
    let python = python3_path()
        .map(str::to_string)
        .or_else(python3_from_path)
        .expect("python3 is required for task timeout proof");
    let marker = temp_marker("nixfied-timeout-task-survivor");
    let marker_arg = marker.to_string_lossy().to_string();
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut fixture =
        ServiceFixture::new(&python, &["-c", python_listener_script(), "${port}"], port);
    fixture
        .model
        .tasks
        .get_mut("smoke")
        .expect("fixture has task")
        .invocation
        .timeout_ms = 100u64.try_into().unwrap();
    set_smoke_args(
        &mut fixture.model,
        &[
            "-c",
            "import subprocess, sys; subprocess.Popen(['/bin/sh', '-c', 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30', 'child', sys.argv[1]]).wait()",
            &marker_arg,
        ],
    );
    fixture.relower();
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-task-timeout",
        port,
    )
    .expect("foreground service should start");
    service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect("owned listener should become ready");

    let error = run_dependent_task(
        &fixture.placement,
        &mut fixture.registry,
        RunContext::from_service(&service),
        &[&service],
        fixture
            .admission
            .execution_model
            .tasks
            .get("smoke")
            .expect("smoke task"),
    )
    .expect_err("task should time out as a task failure");
    thread::sleep(Duration::from_millis(2300));
    let report = ps(&mut fixture.registry).expect("ps should reconcile timed-out task");
    let task_observations = report
        .processes
        .iter()
        .filter(|process| process.service_instance_id.is_none())
        .collect::<Vec<_>>();
    let task_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE service_instance_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("task status should query");
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");
    let task_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type IN ('task.canceling','task.timed-out')",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let summary: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .placement
                .summary_path
                .with_file_name("summary.smoke.json"),
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
    assert_eq!(lease_status, "failed");
    assert_eq!(task_events, 2);
    assert_eq!(summary["timedOut"], json!(true));
    assert_eq!(summary["canceled"], json!(false));
    assert!(
        !marker.exists(),
        "task timeout should kill TERM-ignoring descendants before marker"
    );
    let _ = fs::remove_file(marker);
    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn cli_signal_cancels_run_and_empties_service_group() {
    let Some(shell) = nix_store_executable(&["sh", "bash"]) else {
        return;
    };
    let closure_root = closure_root_for_store_executable(&shell)
        .expect("store executable should have a closure root");
    let marker = temp_marker("nixfied-cli-cancel-survivor");
    let started = temp_marker("nixfied-cli-cancel-started");
    let marker_arg = marker.to_string_lossy().to_string();
    let started_arg = started.to_string_lossy().to_string();
    let script = "touch \"$2\"; trap 'exit 0' TERM; /bin/sh -c 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30' child \"$1\" & wait";
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut value = fixture_model(
        &shell.to_string_lossy(),
        &["service", "--host", "127.0.0.1", "--port", "${port}"],
        port,
    );
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["executable"] = json!(shell.to_string_lossy());
    prepend_invocation_args(
        &mut value,
        &["-c", script, "parent", &marker_arg, &started_arg],
    );
    // The script needs touch/sleep on its hermetic PATH: declare coreutils as a
    // tool like any adopter would.
    let Some(touch) = nix_store_executable(&["touch"]) else {
        return;
    };
    add_tool_closure(&mut value, "coreutils-tools", &touch);
    let model: Model = serde_json::from_value(value).expect("CLI fixture model should parse");
    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    let mut child = Command::new(runtime_binary())
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--state-base")
        .arg(&state_base)
        .arg("--timeout-ms")
        .arg("200")
        .current_dir(&tmp.path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    assert_eq!(output.status.code(), Some(27));
    let error: Value = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("CANCELED"));

    let ps_output = Command::new(runtime_binary())
        .arg("ps")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--state-base")
        .arg(&state_base)
        .current_dir(&tmp.path)
        .output()
        .expect("runtime ps should run");
    assert!(
        ps_output.status.success(),
        "ps failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&ps_output.stdout),
        String::from_utf8_lossy(&ps_output.stderr)
    );
    let ps_report: Value =
        serde_json::from_slice(&ps_output.stdout).expect("ps stdout should be JSON");
    let process = ps_report["processes"]
        .as_array()
        .expect("ps processes should be an array")
        .iter()
        .find(|process| process["serviceInstanceId"].is_string())
        .expect("service process should be reported");
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
    let Some(shell) = nix_store_executable(&["sh", "bash"]) else {
        return;
    };
    let python = python3_path()
        .map(str::to_string)
        .or_else(python3_from_path)
        .expect("python3 is required for shutdown cancellation proof");
    let closure_root = closure_root_for_store_executable(&shell)
        .expect("store executable should have a closure root");
    let started = temp_marker("nixfied-cli-shutdown-started");
    let stopping = temp_marker("nixfied-cli-shutdown-stopping");
    let started_arg = started.to_string_lossy().to_string();
    let stopping_arg = stopping.to_string_lossy().to_string();
    let script = "started=\"$1\"; stopping=\"$2\"; python=\"$3\"; cmd=\"$4\"; if [ \"$cmd\" = service ]; then port=\"$8\"; touch \"$started\"; trap 'touch \"$2\"; sleep 30' TERM; \"$python\" -c 'import socket, sys, time; s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((\"127.0.0.1\", int(sys.argv[1]))); s.listen(16); time.sleep(30)' \"$port\" & wait; elif [ \"$cmd\" = task ]; then exit 0; elif [ \"$cmd\" = stop ]; then exit 0; else exit 1; fi";
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut value = fixture_model(
        &shell.to_string_lossy(),
        &["service", "--host", "127.0.0.1", "--port", "${port}"],
        port,
    );
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["executable"] = json!(shell.to_string_lossy());
    prepend_invocation_args(
        &mut value,
        &[
            "-c",
            script,
            "wrapper",
            &started_arg,
            &stopping_arg,
            &python,
        ],
    );
    // The script needs touch/sleep on its hermetic PATH: declare coreutils as a
    // tool like any adopter would.
    let Some(touch) = nix_store_executable(&["touch"]) else {
        return;
    };
    add_tool_closure(&mut value, "coreutils-tools", &touch);
    let model: Model = serde_json::from_value(value).expect("CLI fixture model should parse");
    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    let child = Command::new(runtime_binary())
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--state-base")
        .arg(&state_base)
        .arg("--timeout-ms")
        .arg("1000")
        .current_dir(&tmp.path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("runtime run should spawn");
    assert!(
        wait_for_path(&started, Duration::from_secs(3)),
        "service should start before shutdown proof"
    );
    assert!(
        wait_for_path(&stopping, Duration::from_secs(5)),
        "service should enter stop handling before signal"
    );
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "SIGTERM should be delivered to runtime");
    let output = wait_for_child_output(child, Duration::from_secs(6));
    assert_eq!(output.status.code(), Some(27));
    let error: Value = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("CANCELED"));

    let ps_output = Command::new(runtime_binary())
        .arg("ps")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--state-base")
        .arg(&state_base)
        .current_dir(&tmp.path)
        .output()
        .expect("runtime ps should run");
    assert!(
        ps_output.status.success(),
        "ps failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&ps_output.stdout),
        String::from_utf8_lossy(&ps_output.stderr)
    );
    let ps_report: Value =
        serde_json::from_slice(&ps_output.stdout).expect("ps stdout should be JSON");
    let process = ps_report["processes"]
        .as_array()
        .expect("ps processes should be an array")
        .iter()
        .find(|process| process["serviceInstanceId"].is_string())
        .expect("service process should be reported");
    assert_eq!(process["live"], json!(false));
    assert_eq!(process["registryStatus"], json!("canceled"));
    let _ = fs::remove_file(started);
    let _ = fs::remove_file(stopping);
}

#[test]
fn readiness_timeout_prefers_escape_discovered_during_probe() {
    if !Path::new("/usr/bin/perl").exists() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut fixture = ServiceFixture::new(
        "/usr/bin/perl",
        &[
            "-MPOSIX=setsid",
            "-e",
            "select(undef, undef, undef, 0.2); if (fork() == 0) { setsid(); sleep 30; exit 0; } sleep 30;",
        ],
        port,
    );
    let probe = &mut fixture
        .model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .lifecycle
        .ready
        .probe;
    probe.max_attempts = 30u32.try_into().unwrap();
    probe.retry_interval_ms = 20u64.try_into().unwrap();
    fixture.relower();
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-readiness-timeout-escape",
        port,
    )
    .expect("service should initially start");

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("readiness should report the monitored escape");

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let escaped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    let escaped_services: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM services WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("service status should query");
    let failure_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.failed'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    assert_eq!(escaped_processes, 1);
    assert_eq!(escaped_services, 1);
    assert_eq!(failure_events, 0);
}

#[test]
fn daemonizing_service_escape_is_recorded_and_refused() {
    let mut fixture = ServiceFixture::new("/bin/sh", &["-c", "sleep 30 & exit 0"], 23182);

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-escape",
        23182,
    ) {
        Ok(_) => panic!("daemonizing service should be refused"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let escaped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    let escaped_services: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM services WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("service status should query");

    assert_eq!(escaped_processes, 1);
    assert_eq!(escaped_services, 1);
}

#[test]
fn setsid_descendant_escape_is_recorded_and_refused() {
    if !Path::new("/usr/bin/perl").exists() {
        return;
    }
    let mut fixture = ServiceFixture::new(
        "/usr/bin/perl",
        &[
            "-MPOSIX=setsid",
            "-e",
            "if (fork() == 0) { setsid(); sleep 30; exit 0; } sleep 30;",
        ],
        23183,
    );

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-setsid-escape",
        23183,
    ) {
        Ok(_) => panic!("setsid descendant should be refused"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let escaped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    assert_eq!(escaped_processes, 1);
}

#[test]
fn stop_refuses_delayed_setsid_escape() {
    if !Path::new("/usr/bin/perl").exists() {
        return;
    }
    let mut fixture = ServiceFixture::new(
        "/usr/bin/perl",
        &[
            "-MPOSIX=setsid",
            "-e",
            "sleep 1; if (fork() == 0) { setsid(); sleep 30; exit 0; } sleep 1; exit 0;",
        ],
        23185,
    );
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-delayed-escape",
        23185,
    )
    .expect("service should initially pass handoff");
    thread::sleep(Duration::from_millis(1300));

    let error = service
        .stop(&mut fixture.registry, 1000)
        .expect_err("stop should refuse escaped descendants");

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let escaped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    let stopped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'stopped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    assert_eq!(escaped_processes, 1);
    assert_eq!(stopped_processes, 0);
}

#[test]
fn readiness_refuses_monitored_setsid_escape() {
    if !Path::new("/usr/bin/perl").exists() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    let mut fixture = ServiceFixture::new(
        "/usr/bin/perl",
        &[
            "-MPOSIX=setsid",
            "-e",
            "sleep 1; if (fork() == 0) { setsid(); sleep 30; exit 0; } sleep 1;",
        ],
        port,
    );
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-readiness-escape",
        port,
    )
    .expect("service should initially pass handoff");
    thread::sleep(Duration::from_millis(1300));

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("readiness should refuse monitored escape");

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let probe_ready_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.probe-ready'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let escaped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    assert_eq!(probe_ready_events, 0);
    assert_eq!(escaped_processes, 1);
    drop(listener);
}

#[test]
fn readiness_records_foreground_exit_as_escape() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    let mut fixture = ServiceFixture::new("/bin/sh", &["-c", "sleep 1"], port);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-readiness-exit",
        port,
    )
    .expect("service should initially pass handoff");
    thread::sleep(Duration::from_millis(1300));

    let error = service
        .wait_for_probe_ready(&mut fixture.registry)
        .expect_err("readiness should record foreground exit as escape");

    assert_eq!(error.code, ErrorCode::ProcEscape);
    let escaped_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'escaped'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    let running_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'running'",
            [],
            |row| row.get(0),
        )
        .expect("process status should query");
    assert_eq!(escaped_processes, 1);
    assert_eq!(running_processes, 0);
    drop(listener);
}

#[test]
fn duplicate_active_service_start_is_refused() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23184);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-first",
        23184,
    )
    .expect("first foreground service should start");

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-second",
        23184,
    ) {
        Ok(_) => panic!("duplicate active service should be refused"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::LeaseConflict);
    let running_processes: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM processes WHERE status = 'running'",
            [],
            |row| row.get(0),
        )
        .expect("process count should query");
    assert_eq!(running_processes, 1);

    service
        .stop(&mut fixture.registry, 1000)
        .expect("service should stop");
}

#[test]
fn ps_reconciles_dead_owned_process_and_port_as_stale() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["1"], 23187);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-ps-stale",
        23187,
    )
    .expect("foreground service should start");
    thread::sleep(Duration::from_millis(1300));

    let report = ps(&mut fixture.registry).expect("ps should reconcile");

    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == service.process_key)
        .expect("process should be reported");
    assert!(!observed.live);
    assert_eq!(observed.reconciled_status, "stale");
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("process status should query");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let stale_ports: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM ports WHERE status = 'stale'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");
    let stale_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'process.stale'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    assert_eq!(process_status, "stale");
    assert_eq!(service_status, "stale");
    assert_eq!(stale_ports, 1);
    assert_eq!(stale_events, 1);
}

#[test]
fn ps_rejects_live_process_with_mismatched_start_identity_as_stale() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23233);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-ps-pid-reuse",
        23233,
    )
    .expect("foreground service should start");
    assert!(
        process_group_has_non_zombie_member(service.pgid),
        "test service process group should be live before identity mutation"
    );
    let mismatched_identity = json!({
        "pid": service.pid,
        "pgid": service.pgid,
        "platformStart": "not-the-recorded-process-start",
        "observedAtNanos": unique_suffix(),
    })
    .to_string();
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE processes SET start_identity = ?2 WHERE process_key = ?1",
            (&service.process_key, &mismatched_identity),
        )
        .expect("test should corrupt start identity");

    let report = ps(&mut fixture.registry).expect("ps should reconcile mismatched identity");

    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == service.process_key)
        .expect("process should be reported");
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("process status should query");
    let stale_ports: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM ports WHERE status = 'stale'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");

    assert!(!observed.live);
    assert_eq!(observed.reconciled_status, "stale");
    assert_eq!(process_status, "stale");
    assert_eq!(stale_ports, 1);
    assert!(
        process_group_has_non_zombie_member(service.pgid),
        "OS process group should still be live; stale status must come from identity mismatch"
    );
}

#[test]
fn ps_marks_expired_dead_run_lease_as_stale() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23228);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-lease-stale",
        23228,
    )
    .expect("foreground service should start");
    unsafe {
        libc::kill(-service.pgid, libc::SIGKILL);
    }
    thread::sleep(Duration::from_millis(100));
    fixture
        .registry
        .connection()
        .execute(
            "
            UPDATE run_leases
            SET expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 seconds')
            WHERE run_id = ?1
            ",
            [&service.run_id],
        )
        .expect("test should expire lease");

    let report = ps(&mut fixture.registry).expect("ps should reconcile expired dead lease");
    let observed = report
        .processes
        .iter()
        .find(|process| process.process_key == service.process_key)
        .expect("service process should be reported");
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");
    let run_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("run status should query");
    let lease_stale_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'run.lease-stale'",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let lease_stale_hash: Option<String> = fixture
        .registry
        .connection()
        .query_row(
            "SELECT computed_model_hash FROM events WHERE event_type = 'run.lease-stale'",
            [],
            |row| row.get(0),
        )
        .expect("lease stale event hash should query");

    assert!(!observed.live);
    assert_eq!(observed.reconciled_status, "stale");
    assert_eq!(lease_status, "stale");
    assert_eq!(run_status, "stale");
    assert_eq!(lease_stale_events, 1);
    assert_eq!(lease_stale_hash.as_deref(), Some("computed-hash"));

    let restarted = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-after-stale-lease",
        23228,
    )
    .expect("expired dead lease should reconcile before new service start");
    restarted
        .stop(&mut fixture.registry, 1000)
        .expect("restarted service should stop");
}

#[test]
fn active_run_lease_refuses_new_service_start_even_after_terminal_service_row() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23229);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-active-lease",
        23229,
    )
    .expect("foreground service should start");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE services SET status = 'stopped' WHERE service_instance_id = ?1",
            [&service.service_instance_id],
        )
        .expect("test should terminalize service row");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE processes SET status = 'stopped' WHERE process_key = ?1",
            [&service.process_key],
        )
        .expect("test should terminalize process row");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE ports SET status = 'released' WHERE service_instance_id = ?1",
            [&service.service_instance_id],
        )
        .expect("test should release port row");

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-lease-conflict",
        23229,
    ) {
        Ok(_) => panic!("active lease should refuse new owner"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::LeaseConflict);
    assert!(error.message.contains("active run lease"));
    service
        .cancel(&mut fixture.registry, 1000, "test lease conflict cleanup")
        .expect("original service should cancel");
}

#[test]
fn expired_live_run_lease_still_refuses_new_service_start() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23230);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-expired-live-lease",
        23230,
    )
    .expect("foreground service should start");
    fixture
        .registry
        .connection()
        .execute(
            "
            UPDATE run_leases
            SET expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 seconds')
            WHERE run_id = ?1
            ",
            [&service.run_id],
        )
        .expect("test should expire lease");

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-expired-live-conflict",
        23230,
    ) {
        Ok(_) => panic!("expired but live lease should refuse new owner"),
        Err(error) => error,
    };
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");

    assert_eq!(error.code, ErrorCode::LeaseConflict);
    assert_eq!(lease_status, "active");
    service
        .cancel(
            &mut fixture.registry,
            1000,
            "test expired live lease cleanup",
        )
        .expect("original service should cancel");
}

#[test]
fn down_stops_verified_owned_process_group_only() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23188);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down",
        23188,
    )
    .expect("foreground service should start");

    let report =
        down_owned_process_groups(&mut fixture.registry, 1000).expect("down should stop service");

    assert_eq!(report.stopped, vec![service.process_key.clone()]);
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("process status should query");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let released_ports: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM ports WHERE status = 'released'",
            [],
            |row| row.get(0),
        )
        .expect("port status should query");
    assert_eq!(process_status, "stopped");
    assert_eq!(service_status, "stopped");
    assert_eq!(released_ports, 1);
}

#[test]
fn down_completes_canceling_lease_and_unblocks_cleanup() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23231);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down-canceling-lease",
        23231,
    )
    .expect("foreground service should start");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE runs SET status = 'canceling' WHERE run_id = ?1",
            [&service.run_id],
        )
        .expect("test should mark run canceling");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE run_leases SET status = 'canceling' WHERE run_id = ?1",
            [&service.run_id],
        )
        .expect("test should mark lease canceling");
    commit_slot_marker(
        &fixture.placement,
        &StateIdentity::from_model(&fixture.model, &fixture.admission),
    )
    .expect("slot marker should be written for cleanup proof");

    let report =
        down_owned_process_groups(&mut fixture.registry, 1000).expect("down should stop service");
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");
    let run_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("run status should query");
    let process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("process status should query");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let canceled_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.canceled'",
            [],
            |row| row.get(0),
        )
        .expect("event count should query");
    let cleanup = clean_marked_state(
        &fixture.placement.state_base,
        &fixture.placement.state_root,
        &StateIdentity::from_model(&fixture.model, &fixture.admission),
        &mut fixture.registry,
    )
    .expect("canceled lease should not block cleanup");

    assert_eq!(report.stopped, vec![service.process_key.clone()]);
    assert_eq!(lease_status, "canceled");
    assert_eq!(run_status, "canceled");
    assert_eq!(process_status, "canceled");
    assert_eq!(service_status, "canceled");
    assert_eq!(canceled_events, 1);
    assert!(cleanup.deleted_path.ends_with("runtime-test/dev/0"));
    assert!(!fixture.placement.state_root.exists());
}

#[test]
fn down_cancels_live_task_process_group_and_unblocks_cleanup() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 23232);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down-task-canceling",
        23232,
    )
    .expect("foreground service should start");
    let mut command = Command::new("/bin/sleep");
    command
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut task_child = command.spawn().expect("task process should spawn");
    let task_pid = task_child.id();
    let task_pgid = unsafe { libc::getpgid(task_pid as libc::pid_t) };
    assert!(task_pgid > 0, "task process group should exist");
    let task_process_key = format!(
        "process-{}-task-smoke-{}-{}",
        service.run_id, task_pid, task_pgid
    );
    let task_start_identity = json!({
        "pid": task_pid,
        "pgid": task_pgid,
        "platformStart": null,
        "observedAtNanos": unique_suffix(),
    })
    .to_string();
    let task_command_json = json!({
        "executable": "/bin/sleep",
        "args": ["30"],
        "cwd": fixture.admission.require_source().unwrap().observed_root.to_string_lossy(),
        "stdoutPath": fixture.placement.logs_dir.join("task.smoke.stdout.log").to_string_lossy(),
        "stderrPath": fixture.placement.logs_dir.join("task.smoke.stderr.log").to_string_lossy(),
    })
    .to_string();
    record_task_started(
        &mut fixture.registry,
        &TaskProcessRecord {
            run_id: &service.run_id,
            process_key: &task_process_key,
            pid: task_pid,
            pgid: task_pgid,
            start_identity: &task_start_identity,
            command_json: &task_command_json,
            computed_model_hash: &service.computed_model_hash,
        },
    )
    .expect("task process should be recorded");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE runs SET status = 'canceling' WHERE run_id = ?1",
            [&service.run_id],
        )
        .expect("test should mark run canceling");
    fixture
        .registry
        .connection()
        .execute(
            "UPDATE run_leases SET status = 'canceling' WHERE run_id = ?1",
            [&service.run_id],
        )
        .expect("test should mark lease canceling");
    commit_slot_marker(
        &fixture.placement,
        &StateIdentity::from_model(&fixture.model, &fixture.admission),
    )
    .expect("slot marker should be written for cleanup proof");

    let report = down_owned_process_groups(&mut fixture.registry, 1000)
        .expect("down should stop service and task");
    let _ = task_child.wait();
    let lease_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM run_leases WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("lease status should query");
    let run_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [&service.run_id],
            |row| row.get(0),
        )
        .expect("run status should query");
    let service_process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&service.process_key],
            |row| row.get(0),
        )
        .expect("service process status should query");
    let task_process_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM processes WHERE process_key = ?1",
            [&task_process_key],
            |row| row.get(0),
        )
        .expect("task process status should query");
    let service_status: String = fixture
        .registry
        .connection()
        .query_row(
            "SELECT status FROM services WHERE service_instance_id = ?1",
            [&service.service_instance_id],
            |row| row.get(0),
        )
        .expect("service status should query");
    let service_canceled_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'service.canceled'",
            [],
            |row| row.get(0),
        )
        .expect("service event count should query");
    let task_canceled_events: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE event_type = 'task.canceled'",
            [],
            |row| row.get(0),
        )
        .expect("task event count should query");
    let cleanup = clean_marked_state(
        &fixture.placement.state_base,
        &fixture.placement.state_root,
        &StateIdentity::from_model(&fixture.model, &fixture.admission),
        &mut fixture.registry,
    )
    .expect("canceled task lease should not block cleanup");

    assert_eq!(
        report.stopped,
        vec![service.process_key.clone(), task_process_key.clone()]
    );
    assert_eq!(lease_status, "canceled");
    assert_eq!(run_status, "canceled");
    assert_eq!(service_process_status, "canceled");
    assert_eq!(task_process_status, "canceled");
    assert_eq!(service_status, "canceled");
    assert_eq!(service_canceled_events, 1);
    assert_eq!(task_canceled_events, 1);
    assert!(
        !process_group_has_non_zombie_member(service.pgid),
        "down should empty service process group"
    );
    assert!(
        !process_group_has_non_zombie_member(task_pgid),
        "down should empty task process group"
    );
    assert!(cleanup.deleted_path.ends_with("runtime-test/dev/0"));
    assert!(!fixture.placement.state_root.exists());
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
    let marker_arg = marker.to_string_lossy().to_string();
    let script = "trap 'exit 0' TERM; /bin/sh -c 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30' child \"$1\" & wait";
    let mut fixture = ServiceFixture::new(
        "/bin/sh",
        &["-c", script, "parent", marker_arg.as_str()],
        23189,
    );
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down-escalate",
        23189,
    )
    .expect("foreground service should start");

    let report = down_owned_process_groups(&mut fixture.registry, 200)
        .expect("down should escalate and stop the process group");
    thread::sleep(Duration::from_millis(2300));

    assert_eq!(report.stopped, vec![service.process_key.clone()]);
    assert!(
        !marker.exists(),
        "child that ignored TERM should have been killed before touching marker"
    );
    let _ = fs::remove_file(marker);
}

#[test]
fn task_child_path_is_assembled_from_tool_roots() {
    // The child PATH is runtime-owned: exactly the tool roots, in declared
    // order — not the runtime's own inherited PATH.
    let Some(python) = python3_path() else {
        return;
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut value = fixture_model(python, &["-c", python_listener_script(), "${port}"], port);
    value["environments"]["dev"]["services"] = json!([]);
    value["tasks"]["smoke"]["requires"] = json!([]);
    set_task_run_args(
        &mut value,
        &["-c", "import os, sys; sys.stdout.write(os.environ['PATH'])"],
    );
    let mut fixture = ServiceFixture::from_value(value);
    let task = fixture
        .admission
        .execution_model
        .tasks
        .get("smoke")
        .expect("task lowered")
        .clone();
    let source_root = fixture
        .admission
        .require_source()
        .expect("run admission resolves source")
        .observed_root
        .clone();
    let run = run_dependent_task(
        &fixture.placement,
        &mut fixture.registry,
        RunContext {
            run_id: "run-path-proof",
            computed_model_hash: &fixture.admission.computed_model_hash,
            source_root: &source_root,
            state_root: &fixture.placement.state_root,
        },
        &[],
        &task,
    )
    .expect("path-printing task should succeed");
    let stdout = fs::read_to_string(&run.stdout_path).expect("task stdout log");
    let expected = Path::new(python)
        .parent()
        .expect("python parent dir")
        .to_string_lossy()
        .to_string();
    assert_eq!(stdout, expected);
}

struct ServiceFixture {
    _tmp: TempDir,
    model: Model,
    admission: Admission,
    placement: nixfied_runtime::state::HostPlacement,
    registry: Registry,
}

impl ServiceFixture {
    fn new(executable: &str, start_args: &[&str], port: u16) -> Self {
        Self::from_model(model(executable, start_args, port))
    }

    fn from_value(value: Value) -> Self {
        Self::from_model(serde_json::from_value(value).expect("fixture model should parse"))
    }

    fn from_model(model: Model) -> Self {
        let tmp = TempDir::new();
        let admission = admission(&model, &tmp.path);
        let placement =
            derive_host_placement(&model, "run-service", &tmp.path).expect("layout should derive");
        materialize_run_roots(&placement).expect("roots should materialize");
        let registry = Registry::open_or_create(
            placement.registry_path(),
            &RegistryIdentity::default_slot(
                &model.project.project_id,
                &model.runtime_abi,
                &model.toolchain_id,
            ),
        )
        .expect("registry should open");
        Self {
            _tmp: tmp,
            model,
            admission,
            placement,
            registry,
        }
    }

    /// Re-lower the (mutated) model into the admission's ExecutionModel. Tests that
    /// edit `self.model` after construction must call this so the executor, which
    /// reads the lowered model, sees the change.
    fn relower(&mut self) {
        self.admission.execution_model =
            nixfied_runtime::execution::lower(&self.model).expect("mutated model should lower");
    }
}

struct StartedSlot<'a> {
    selected: nixfied_runtime::slot::SelectedSlot<'a>,
    placement: nixfied_runtime::state::HostPlacement,
    registry: Registry,
    service: nixfied_runtime::service::StartedService,
}

impl<'a> StartedSlot<'a> {
    fn start(
        model: &'a Model,
        admission: &Admission,
        state_base: &Path,
        slot: u32,
        run_id: &str,
        selected_port: u16,
    ) -> Self {
        let selected = select_slot(model, Some(slot)).expect("slot should select");
        let placement = derive_host_placement_for_slot(model, &selected, run_id, state_base)
            .expect("slot placement should derive");
        materialize_run_roots(&placement).expect("slot roots should materialize");
        let identity = StateIdentity::from_selected_slot(model, admission, &selected);
        commit_slot_marker(&placement, &identity).expect("slot marker should be written");
        let mut registry = Registry::open_or_create(
            placement.registry_path(),
            &RegistryIdentity::for_slot(
                &model.project.project_id,
                selected.environment,
                selected.slot,
                &model.runtime_abi,
                &model.toolchain_id,
            ),
        )
        .expect("slot registry should open");
        let mut service = start_synthetic_service_for_slot(
            admission,
            &placement,
            &mut registry,
            run_id,
            &selected,
            selected_port,
        )
        .expect("slot service should start");
        service
            .wait_for_probe_ready(&mut registry)
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
fn endpoint_port_is_reserved_before_start_and_refuses_a_second_holder() {
    // The port is reserved in the same transaction as the run lease, before any
    // prepare or spawn. A different service instance cannot take a port already
    // held in the slot: the conflict is refused at reservation, not discovered
    // after a process has already been started.
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 24222);
    let run_a = RunRecord {
        run_id: "reserve-a",
        owner_token: "owner-a",
        admission: &fixture.admission,
        placement: &fixture.placement,
    };
    reserve_service_start(
        &mut fixture.registry,
        &run_a,
        "service-instance-a",
        &[PortReservation {
            endpoint_key: "service-instance-a:endpoint",
            address: "127.0.0.1",
            port: 24222,
        }],
    )
    .expect("first reservation should hold the port");

    let run_b = RunRecord {
        run_id: "reserve-b",
        owner_token: "owner-b",
        admission: &fixture.admission,
        placement: &fixture.placement,
    };
    let error = reserve_service_start(
        &mut fixture.registry,
        &run_b,
        "service-instance-b",
        &[PortReservation {
            endpoint_key: "service-instance-b:endpoint",
            address: "127.0.0.1",
            port: 24222,
        }],
    )
    .expect_err("a second instance cannot reserve a held port");

    assert_eq!(error.code, ErrorCode::PortConflict);
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
        source: Some(admitted_source(source_root)),
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

fn model(executable: &str, start_args: &[&str], port: u16) -> Model {
    serde_json::from_value(fixture_model(executable, start_args, port))
        .expect("fixture model should parse")
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

fn assert_registry_tables_scoped_to_slot(registry: &Registry, slot: i64, tables: &[&str]) {
    for table in tables {
        let (min_environment, max_environment, min_slot, max_slot, count): (
            String,
            String,
            i64,
            i64,
            i64,
        ) = registry
            .connection()
            .query_row(
                &format!(
                    "SELECT min(environment), max(environment), min(slot), max(slot), count(*) FROM {table}"
                ),
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .unwrap_or_else(|error| panic!("{table} scope query should succeed: {error}"));
        assert!(count > 0, "{table} should have at least one row");
        assert_eq!(min_environment, "dev", "{table} min environment");
        assert_eq!(max_environment, "dev", "{table} max environment");
        assert_eq!(min_slot, slot, "{table} min slot");
        assert_eq!(max_slot, slot, "{table} max slot");
    }
}

/// SEAM-1: the runtime never invokes `nix`. Poison `PATH` with failing
/// `nix`/`nix-store`/`nix-build` shims that touch a sentinel, then drive the full
/// lifecycle (check -> run -> clean) through the binary and assert the sentinel is
/// never created. Service/task execs run by absolute store path, so poisoning
/// `PATH` cannot starve them - only a nix invocation would trip the sentinel.
#[test]
fn runtime_drives_full_lifecycle_without_invoking_nix() {
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let closure_root = closure_root_for_store_executable(&python)
        .expect("store executable should have a closure root");
    let script = [
        "import socket, sys, time",
        "cmd = sys.argv[1]",
        "if cmd == 'service':",
        "    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)",
        "    s.bind(('127.0.0.1', int(sys.argv[2]))); s.listen(16); time.sleep(30)",
        "elif cmd == 'task':",
        "    socket.create_connection(('127.0.0.1', int(sys.argv[2])), timeout=5).close()",
    ]
    .join("\n");

    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let mut value = fixture_model(&python.to_string_lossy(), &["service", "${port}"], port);
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    set_task_run_args(&mut value, &["task", "${port}"]);
    prepend_invocation_args(&mut value, &["-c", &script]);
    let model: Model = serde_json::from_value(value).expect("seam fixture model should parse");

    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    // A PATH whose nix tools fail loudly and record that they were called.
    let fake_bin = tmp.path.join("fake-bin");
    fs::create_dir_all(&fake_bin).expect("fake bin dir should be created");
    let sentinel = tmp.path.join("nix-was-invoked");
    for tool in ["nix", "nix-store", "nix-build"] {
        let shim = fake_bin.join(tool);
        fs::write(
            &shim,
            format!(
                "#!/bin/sh\ntouch {sentinel:?}\necho '{tool} must not be invoked by nixfied-runtime' >&2\nexit 127\n"
            ),
        )
        .expect("shim should be written");
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))
            .expect("shim should be executable");
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
        let mut invocation = Command::new(runtime_binary());
        invocation
            .arg(command)
            .arg("--allow-non-store-model")
            .arg("--model")
            .arg(&model_path)
            .args(extra)
            .current_dir(&tmp.path)
            .env("PATH", &poisoned_path)
            .env("NIXFIED_STATE_DIR", &state_base)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        invocation.output().expect("runtime should run")
    };

    let check = run_binary("check", &[]);
    assert!(
        check.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let run = run_binary("run", &["--timeout-ms", "5000"]);
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let run_json: Value =
        serde_json::from_slice(&run.stdout).expect("run output should be valid JSON");
    assert_eq!(
        run_json["task"]["success"],
        json!(true),
        "smoke task should succeed: {run_json}"
    );
    let state_root = state_base.join("runtime-test").join("dev").join("0");
    assert!(
        state_root.join(".nixfied-state.json").is_file(),
        "slot marker should exist after run"
    );

    let clean = run_binary("clean", &[]);
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(
        !state_root.exists(),
        "clean should remove the slot state root"
    );

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
fn task_only_run_records_a_durable_runs_row() {
    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let closure_root = closure_root_for_store_executable(&python)
        .expect("store executable should have a closure root");
    // A service-less task: it exits 0 without touching any port.
    let script = "import sys; sys.exit(0)";

    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let mut value = fixture_model(&python.to_string_lossy(), &["service", "${port}"], port);
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    // The environment starts no services and runs only the service-less task.
    value["environments"]["dev"]["services"] = json!([]);
    value["tasks"]["smoke"]["requires"] = json!([]);
    set_task_run_args(&mut value, &["noservice"]);
    prepend_invocation_args(&mut value, &["-c", script]);
    let model: Model = serde_json::from_value(value).expect("task-only model should parse");

    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    let run = Command::new(runtime_binary())
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--timeout-ms")
        .arg("5000")
        .current_dir(&tmp.path)
        .env("NIXFIED_STATE_DIR", &state_base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("runtime should run");
    assert!(
        run.status.success(),
        "task-only run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let registry_path =
        find_file(&state_base, "registry.sqlite3").expect("a registry must exist after the run");
    let conn = rusqlite::Connection::open(&registry_path).expect("registry should open");
    let (count, status): (i64, String) = conn
        .query_row(
            "SELECT count(*), coalesce(max(status), '') FROM runs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("runs row should be queryable");
    assert_eq!(
        count, 1,
        "a task-only run must leave exactly one durable runs row"
    );
    assert_eq!(
        status, "task-succeeded",
        "the run row must record the task's terminal status"
    );
}

/// Bug #1: a task whose exec declares `stdin: inherit` must receive the operator's
/// stdin, not a closed `/dev/null`. The runtime inherits its own stdin to the task
/// process, so a sentinel piped to `nixfied-runtime run` reaches the task.
#[test]
fn inherit_stdin_reaches_a_task_process() {
    use std::io::Write;

    let Some(python) = nix_store_executable(&["python3"]) else {
        return;
    };
    let closure_root = closure_root_for_store_executable(&python)
        .expect("store executable should have a closure root");
    // The task echoes whatever it reads on stdin to stdout (captured to its log).
    let script = "import sys; sys.stdout.write(sys.stdin.read())";

    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);

    let mut value = fixture_model(&python.to_string_lossy(), &["service", "${port}"], port);
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["environments"]["dev"]["services"] = json!([]);
    value["tasks"]["smoke"]["requires"] = json!([]);
    set_task_run_args(&mut value, &[]);
    prepend_invocation_args(&mut value, &["-c", script]);
    value["tasks"]["smoke"]["invocation"]["stdin"] = json!("inherit");
    let model: Model = serde_json::from_value(value).expect("inherit-stdin model should parse");

    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    let mut child = Command::new(runtime_binary())
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--timeout-ms")
        .arg("5000")
        .current_dir(&tmp.path)
        .env("NIXFIED_STATE_DIR", &state_base)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    assert!(
        out.status.success(),
        "inherit-stdin run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let log =
        find_file(&state_base, "task.smoke.stdout.log").expect("task stdout log should exist");
    let captured = fs::read_to_string(&log).expect("task stdout log should be readable");
    assert!(
        captured.contains("nixfied-inherited-stdin-marker"),
        "task with stdin=inherit did not receive the operator's stdin: {captured:?}"
    );
}

/// Recursively locate the single file with `name` written under `root`.
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(root).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|file| file == name) {
            return Some(path);
        }
    }
    None
}

#[test]
fn failed_workflow_run_writes_failure_summary() {
    let Some(shell) = nix_store_executable(&["sh", "bash"]) else {
        return;
    };
    let closure_root = closure_root_for_store_executable(&shell)
        .expect("store executable should have a closure root");
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    let mut value = fixture_model(
        &shell.to_string_lossy(),
        &["service", "--host", "127.0.0.1", "--port", "${port}"],
        port,
    );
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["executable"] = json!(shell.to_string_lossy());
    // A 0-service workflow whose single node fails: the run must leave the same
    // aggregate workflow evidence a success does, linked from the error.
    value["tasks"]["smoke"]["requires"] = json!([]);
    set_task_run_args(&mut value, &["-c", "exit 3"]);
    value["workflows"]["wf"] = json!({
        "servicesRequired": [],
        "nodes": {
            "fail-node": { "taskId": "smoke", "dependsOn": [] }
        }
    });
    let model: Model = serde_json::from_value(value).expect("failure fixture model should parse");
    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    let output = Command::new(runtime_binary())
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--state-base")
        .arg(&state_base)
        .arg("--workflow")
        .arg("wf")
        .current_dir(&tmp.path)
        .output()
        .expect("runtime run should execute");

    assert_eq!(output.status.code(), Some(30), "TaskFailed exit code");
    let error: Value = stderr_json(&output.stderr);
    assert_eq!(error["code"], json!("TASK_FAILED"));
    let details = &error["details"];
    assert!(details["runId"].is_string(), "error must carry the run id");
    assert!(details["stateRoot"].is_string());
    assert!(details["logsDir"].is_string());
    assert_eq!(details["failedNodeId"], json!("fail-node"));
    let summary_path = details["workflowSummaryPath"]
        .as_str()
        .expect("error must link the workflow summary");
    let summary: Value =
        serde_json::from_slice(&fs::read(summary_path).expect("workflow summary should exist"))
            .expect("workflow summary should parse");
    assert_eq!(summary["success"], json!(false));
    let nodes = summary["nodes"].as_array().expect("nodes should be array");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["nodeId"], json!("fail-node"));
    assert_eq!(nodes[0]["success"], json!(false));
    assert_eq!(nodes[0]["exitCode"], json!(3));
    let stdout_path = details["stdoutPath"]
        .as_str()
        .expect("error must link the failed task stdout");
    assert!(PathBuf::from(stdout_path).exists());
}

#[test]
fn service_failure_before_any_node_writes_failed_summary() {
    let Some(shell) = nix_store_executable(&["sh", "bash"]) else {
        return;
    };
    let closure_root = closure_root_for_store_executable(&shell)
        .expect("store executable should have a closure root");
    let listener = TcpListener::bind("127.0.0.1:0").expect("temporary listener should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    // The service dies before ever listening, so the run fails on the ready
    // probe with zero node results — the summary must still record failure.
    let mut value = fixture_model(&shell.to_string_lossy(), &["-c", "exit 1"], port);
    value["closures"]["synthetic-helper"]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"]["synthetic-helper"]["executable"] = json!(shell.to_string_lossy());
    value["workflows"]["wf"] = json!({
        "servicesRequired": ["synthetic"],
        "nodes": {
            "never-runs": { "taskId": "smoke", "dependsOn": [] }
        }
    });
    let model: Model = serde_json::from_value(value).expect("failure fixture model should parse");
    let tmp = TempDir::new();
    let model_path = tmp.path.join("model.json");
    let state_base = tmp.path.join("state");
    fs::create_dir_all(&state_base).expect("state base should be created");
    fs::write(
        &model_path,
        serde_json::to_vec_pretty(&model).expect("model should serialize"),
    )
    .expect("model should be written");

    let output = Command::new(runtime_binary())
        .arg("run")
        .arg("--allow-non-store-model")
        .arg("--model")
        .arg(&model_path)
        .arg("--state-base")
        .arg(&state_base)
        .arg("--workflow")
        .arg("wf")
        .current_dir(&tmp.path)
        .output()
        .expect("runtime run should execute");

    assert!(!output.status.success(), "the run must fail");
    let error: Value = stderr_json(&output.stderr);
    let summary_path = error["details"]["workflowSummaryPath"]
        .as_str()
        .expect("error must link the workflow summary");
    let summary: Value =
        serde_json::from_slice(&fs::read(summary_path).expect("workflow summary should exist"))
            .expect("workflow summary should parse");
    assert_eq!(
        summary["success"],
        json!(false),
        "a run that failed before any node must not summarize as success"
    );
    assert_eq!(summary["nodes"].as_array().map(Vec::len), Some(0));
}

fn fixture_model(executable: &str, start_args: &[&str], port: u16) -> Value {
    common::synthetic_model(executable, start_args, port, port)
}

/// Insert args right after `run[0]` of one invocation value — the old shared
/// exec base args, applied per inline invocation.
fn splice_run_args(invocation: &mut Value, args: &[&str]) {
    let run = invocation["run"].as_array_mut().expect("run is an array");
    for (index, arg) in args.iter().enumerate() {
        run.insert(1 + index, json!(arg));
    }
}

/// Apply shared script args to both the start and smoke invocations.
fn prepend_invocation_args(value: &mut Value, args: &[&str]) {
    splice_run_args(
        &mut value["services"]["synthetic"]["lifecycle"]["start"]["invocation"],
        args,
    );
    splice_run_args(&mut value["tasks"]["smoke"]["invocation"], args);
}

/// Replace the smoke task's argv tail (`run[1..]`), keeping the program word.
fn set_task_run_args(value: &mut Value, args: &[&str]) {
    let run = value["tasks"]["smoke"]["invocation"]["run"]
        .as_array_mut()
        .expect("run is an array");
    run.truncate(1);
    for arg in args {
        run.push(json!(arg));
    }
}

/// Replace the smoke task's argv tail on the typed model.
fn set_smoke_args(model: &mut Model, args: &[&str]) {
    let run = &mut model
        .tasks
        .get_mut("smoke")
        .expect("fixture has task")
        .invocation
        .run;
    run.truncate(1);
    run.extend(args.iter().map(|arg| arg.to_string()));
}

fn python3_path() -> Option<&'static str> {
    [
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
        "/usr/bin/python3",
        "/bin/python3",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists())
}

fn python3_from_path() -> Option<String> {
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|dir| Path::new(dir).join("python3"))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
}

fn python_listener_script() -> &'static str {
    "import socket, sys, time; s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('127.0.0.1', int(sys.argv[1]))); s.listen(16); time.sleep(30)"
}

fn python_wildcard_listener_script() -> &'static str {
    "import socket, sys, time; s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('0.0.0.0', int(sys.argv[1]))); s.listen(16); time.sleep(30)"
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

fn runtime_binary() -> PathBuf {
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

fn nix_store_executable(names: &[&str]) -> Option<PathBuf> {
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
        // Return a /nix/store path whose *file name* is still the requested shell
        // name, never the canonicalized target. A multi-call binary (e.g. busybox)
        // dispatches on `argv[0]`: invoked as `.../bin/sh` it acts as a shell, but
        // invoked by its canonical `.../bin/busybox` path it treats `-c` as an
        // applet name and exits 127. The runtime execs this path verbatim, so the
        // name must be preserved.
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

/// The store scan can surface binaries for a foreign architecture (e.g. an
/// x86-64 python3 in an aarch64 host's store, pulled in by a cross or remote
/// build); spawning is the only reliable arch check.
fn spawnable(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Declare an extra tool closure and add it to both the start and smoke
/// invocations' tool sets, widening the child PATH by the tool's bin dir.
fn add_tool_closure(value: &mut Value, id: &str, executable: &Path) {
    let root = closure_root_for_store_executable(executable)
        .map(|root| root.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());
    let target = value["target"]["closureSystem"].clone();
    value["closures"][id] = json!({
        "kind": "executable", "storePath": root,
        "executable": executable.to_string_lossy(),
        "targetSystem": target,
        "operationBindings": [],
        "requiresExecutable": true, "effects": ["process"]
    });
    for invocation in ["start", "smoke"] {
        let tools = if invocation == "start" {
            &mut value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["tools"]
        } else {
            &mut value["tasks"]["smoke"]["invocation"]["tools"]
        };
        tools
            .as_array_mut()
            .expect("tools is an array")
            .push(json!(id));
    }
}

fn closure_root_for_store_executable(executable: &Path) -> Option<PathBuf> {
    let rest = executable.to_str()?.strip_prefix("/nix/store/")?;
    let package = rest.split('/').next()?;
    Some(Path::new("/nix/store").join(package))
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_child_output(mut child: Child, timeout: Duration) -> Output {
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
        thread::sleep(Duration::from_millis(25));
    }
}

fn process_group_has_non_zombie_member(pgid: i32) -> bool {
    let output = Command::new("ps")
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
