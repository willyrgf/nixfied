use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use nixfied_model::{DirtyPolicy, Model, SourceMode};
use nixfied_runtime::cancellation::CancellationToken;
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::service::registry::{TaskProcessRecord, record_task_started};
use nixfied_runtime::service::{
    run_dependent_task, run_dependent_task_cancellable, service_address_hash, service_instance_id,
    start_synthetic_service, start_synthetic_service_for_slot, wait_for_readiness_probe,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, clean_marked_state, derive_host_placement, derive_host_placement_for_slot,
    materialize_run_roots, write_slot_marker,
};
use nixfied_runtime::{Admission, AdmittedSource, ErrorCode};
use serde_json::{Value, json};

use nixfied_runtime::control::{down_owned_process_groups, ps};

#[test]
fn starts_foreground_service_in_owned_process_group_and_records_before_ready() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38180);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-service",
        38180,
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
        json!(fixture.admission.source.observed_root.to_string_lossy())
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
}

#[test]
fn service_start_rejects_exec_cwd_escape() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38180);
    fixture
        .model
        .execs
        .get_mut("m0-helper")
        .expect("fixture exec should exist")
        .cwd = "..".to_string();

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-cwd-escape",
        38180,
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
    let mut value = fixture_model("/bin/sleep", &["30"], 38180);
    value["services"]["synthetic"]["endpoints"][0]["port"] = json!({
        "kind": "candidate-window",
        "start": 38180,
        "end": 38180
    });
    add_slot_one(&mut value, 38280, 38280);
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
        &model,
        &admission,
        &placement,
        &mut registry,
        "run-slot-1",
        &selected_slot,
        38280,
    )
    .expect("slot 1 service should accept slot placement port");

    assert_eq!(service.selected_endpoint.port, 38280);
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
    let mut value = fixture_model(&python, &["-c", python_listener_script(), "${port}"], 38210);
    value["services"]["synthetic"]["endpoints"][0]["port"] = json!({
        "kind": "candidate-window",
        "start": 38210,
        "end": 38210
    });
    add_slot_one(&mut value, 38310, 38320);
    let model: Model = serde_json::from_value(value).expect("fixture model should parse");
    let admission = admission(&model, &tmp.path);

    let mut slot0 = StartedSlot::start(&model, &admission, &tmp.path, 0, "run-slot-0", 38210);
    let mut slot1 = StartedSlot::start(&model, &admission, &tmp.path, 1, "run-slot-1", 38310);

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
    let mut value = fixture_model("/bin/sleep", &["30"], 38180);
    add_slot_one(&mut value, 38280, 38280);
    let model: Model = serde_json::from_value(value).expect("fixture model should parse");
    let service = model
        .services
        .get("synthetic")
        .expect("fixture has synthetic service");

    let slot_0_address = service_address_hash(&model, "dev", 0, "synthetic");
    let slot_1_address = service_address_hash(&model, "dev", 1, "synthetic");
    let slot_0_instance = service_instance_id(&slot_0_address, &service.identity);
    let slot_1_instance = service_instance_id(&slot_1_address, &service.identity);

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
    fixture
        .model
        .tasks
        .get_mut("smoke")
        .expect("fixture has task")
        .args = vec![
        "-c".to_string(),
        "import sys; sys.stdout.write('task-ok')".to_string(),
    ];
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
        .expect("owned listener should become ready");

    let task = run_dependent_task(
        &fixture.model,
        &fixture.placement,
        &mut fixture.registry,
        &service,
        "smoke",
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38186);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-task-not-ready",
        38186,
    )
    .expect("foreground service should start");

    let error = run_dependent_task(
        &fixture.model,
        &fixture.placement,
        &mut fixture.registry,
        &service,
        "smoke",
    )
    .expect_err("task should wait for probe-ready service");

    assert_eq!(error.code, ErrorCode::ModelAdmission);
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
    let service = fixture
        .model
        .services
        .get("synthetic")
        .expect("fixture has service");
    let endpoint = service.endpoints.first().expect("fixture has endpoint");

    let error = wait_for_readiness_probe(service, endpoint, port)
        .expect_err("closed endpoint should time out");

    assert_eq!(error.code, ErrorCode::ReadinessTimeout);
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
        .wait_for_probe_ready_cancellable(&fixture.model, &mut fixture.registry, &cancellation)
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
    fixture
        .model
        .tasks
        .get_mut("smoke")
        .expect("fixture has task")
        .args = vec![
        "-c".to_string(),
        "import signal, subprocess, sys; signal.signal(signal.SIGTERM, lambda *_: sys.exit(0)); subprocess.Popen(['/bin/sh', '-c', 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30', 'child', sys.argv[1]]).wait()".to_string(),
        marker_arg.clone(),
    ];
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
        .expect("owned listener should become ready");
    let cancellation = CancellationToken::new();
    let canceler = cancellation.clone();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        canceler.cancel();
    });

    let error = run_dependent_task_cancellable(
        &fixture.model,
        &fixture.placement,
        &mut fixture.registry,
        &service,
        "smoke",
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
        &fs::read(&fixture.placement.summary_path).expect("summary should read"),
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
fn task_timeout_records_canceled_summary_and_terminates_task_group() {
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
        .execs
        .get_mut("m0-helper")
        .expect("fixture has exec")
        .timeout_ms = 100;
    fixture
        .model
        .tasks
        .get_mut("smoke")
        .expect("fixture has task")
        .args = vec![
        "-c".to_string(),
        "import subprocess, sys; subprocess.Popen(['/bin/sh', '-c', 'trap \"\" TERM; sleep 2; touch \"$1\"; sleep 30', 'child', sys.argv[1]]).wait()".to_string(),
        marker_arg.clone(),
    ];
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
        .expect("owned listener should become ready");

    let error = run_dependent_task(
        &fixture.model,
        &fixture.placement,
        &mut fixture.registry,
        &service,
        "smoke",
    )
    .expect_err("task should time out as cancellation");
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
            "SELECT count(*) FROM events WHERE event_type IN ('task.canceling','task.canceled')",
            [],
            |row| row.get(0),
        )
        .expect("events should query");
    let summary: Value = serde_json::from_slice(
        &fs::read(&fixture.placement.summary_path).expect("summary should read"),
    )
    .expect("summary should parse");

    assert_eq!(error.code, ErrorCode::Canceled);
    assert!(error.message.contains("timed out"));
    assert!(!task_observations.is_empty());
    assert!(task_observations.iter().all(|process| !process.live));
    for process in &task_observations {
        assert!(
            !process_group_has_non_zombie_member(process.pgid),
            "timed-out task process group should be empty"
        );
    }
    assert_eq!(task_status, "canceled");
    assert_eq!(lease_status, "canceled");
    assert_eq!(task_events, 2);
    assert_eq!(summary["timedOut"], json!(true));
    assert_eq!(summary["canceled"], json!(true));
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
    value["closures"][0]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"][0]["executable"] = json!(shell.to_string_lossy());
    value["execs"]["m0-helper"]["executable"] = json!(shell.to_string_lossy());
    value["execs"]["m0-helper"]["args"] = json!(["-c", script, "parent", marker_arg, started_arg]);
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
    let error: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be runtime error JSON");
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
    value["closures"][0]["storePath"] = json!(closure_root.to_string_lossy());
    value["closures"][0]["executable"] = json!(shell.to_string_lossy());
    value["execs"]["m0-helper"]["executable"] = json!(shell.to_string_lossy());
    value["execs"]["m0-helper"]["args"] =
        json!(["-c", script, "wrapper", started_arg, stopping_arg, python]);
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
    let error: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be runtime error JSON");
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
    let probe = fixture
        .model
        .services
        .get_mut("synthetic")
        .expect("fixture has service")
        .probes
        .first_mut()
        .expect("fixture has probe");
    probe.max_attempts = 30;
    probe.retry_interval_ms = 20;
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
    let mut fixture = ServiceFixture::new("/bin/sh", &["-c", "sleep 30 & exit 0"], 38182);

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-escape",
        38182,
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
        38183,
    );

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-setsid-escape",
        38183,
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
        38185,
    );
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-delayed-escape",
        38185,
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
        .wait_for_probe_ready(&fixture.model, &mut fixture.registry)
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38184);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-first",
        38184,
    )
    .expect("first foreground service should start");

    let error = match start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-second",
        38184,
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["1"], 38187);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-ps-stale",
        38187,
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
fn ps_marks_expired_dead_run_lease_as_stale() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38228);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-lease-stale",
        38228,
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
        38228,
    )
    .expect("expired dead lease should reconcile before new service start");
    restarted
        .stop(&mut fixture.registry, 1000)
        .expect("restarted service should stop");
}

#[test]
fn active_run_lease_refuses_new_service_start_even_after_terminal_service_row() {
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38229);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-active-lease",
        38229,
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
        38229,
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38230);
    let mut service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-expired-live-lease",
        38230,
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
        38230,
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38188);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down",
        38188,
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38231);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down-canceling-lease",
        38231,
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
    write_slot_marker(
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
    let mut fixture = ServiceFixture::new("/bin/sleep", &["30"], 38232);
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down-task-canceling",
        38232,
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
        "taskId": "smoke",
        "executable": "/bin/sleep",
        "args": ["30"],
        "cwd": fixture.admission.source.observed_root.to_string_lossy(),
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
    write_slot_marker(
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
        38189,
    );
    let service = start_synthetic_service(
        &fixture.model,
        &fixture.admission,
        &fixture.placement,
        &mut fixture.registry,
        "run-down-escalate",
        38189,
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

struct ServiceFixture {
    _tmp: TempDir,
    model: Model,
    admission: Admission,
    placement: nixfied_runtime::state::HostPlacement,
    registry: Registry,
}

impl ServiceFixture {
    fn new(executable: &str, start_args: &[&str], port: u16) -> Self {
        let tmp = TempDir::new();
        let model = model(executable, start_args, port);
        let admission = admission(&model, &tmp.path);
        let placement =
            derive_host_placement(&model, "run-service", &tmp.path).expect("layout should derive");
        materialize_run_roots(&placement).expect("roots should materialize");
        let registry = Registry::open_or_create(
            placement.registry_path(),
            &RegistryIdentity::m0(
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
        write_slot_marker(&placement, &identity).expect("slot marker should be written");
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
            model,
            admission,
            &placement,
            &mut registry,
            run_id,
            &selected,
            selected_port,
        )
        .expect("slot service should start");
        service
            .wait_for_probe_ready(model, &mut registry)
            .expect("slot service should become ready");
        Self {
            selected,
            placement,
            registry,
            service,
        }
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
        admission_fingerprint_policy: "m0-placeholder".to_string(),
    }
}

fn model(executable: &str, start_args: &[&str], port: u16) -> Model {
    serde_json::from_value(fixture_model(executable, start_args, port))
        .expect("fixture model should parse")
}

fn add_slot_one(value: &mut Value, start: u16, end: u16) {
    value["slotPolicy"]["max"] = json!(1);
    value["runtimeConstraints"]["slotMax"] = json!(1);
    value["capabilities"]["slots"] = json!([0, 1]);
    value["placement"]["slotPlacements"]["1"] = json!({
        "slot": 1,
        "stateRootTemplate": "${projectId}/${environment}/${slot}",
        "registryDir": "registry",
        "runDirTemplate": "runs/${runId}",
        "logsDirTemplate": "runs/${runId}/logs",
        "artifactsDirTemplate": "runs/${runId}/artifacts",
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

fn fixture_model(executable: &str, start_args: &[&str], port: u16) -> Value {
    json!({
        "modelVersion": 1,
        "toolchainId": "nixfied-toolchain:m2b:1",
        "runtimeAbi": "nixfied-runtime-abi:m2b:1",
        "generator": {
            "name": "nixfied",
            "version": "m0",
            "emitter": "nix/compiler/emit-model.nix"
        },
        "project": {
            "projectId": "runtime-test",
            "name": "Runtime Test"
        },
        "target": {
            "system": host_system(),
            "os": host_os(),
            "arch": host_arch(),
            "closureSystem": host_system(),
            "requiredRuntimeCapabilities": {
                "processGroup": true,
                "tcpPortOwnership": true,
                "sqliteWal": true
            }
        },
        "codebases": [{
            "codebaseId": "main",
            "logicalRoot": ".",
            "sourceMode": "live-workspace",
            "sourceIdentity": "live",
            "sourcePolicy": {
                "dirtyPolicy": "warn",
                "admissionFingerprintPolicy": "m0-placeholder"
            }
        }],
        "environments": {
            "dev": {
                "environmentId": "dev",
                "services": ["synthetic"],
                "tasks": ["smoke"]
            }
        },
        "slotPolicy": {
            "min": 0,
            "default": 0,
            "max": 0
        },
        "capabilities": {
            "environments": ["dev"],
            "slots": [0],
            "services": ["synthetic"],
            "tasks": ["smoke"],
            "workflows": [],
            "surfaces": m0_surface_names()
        },
        "runtimeConstraints": {
            "allowedEnvironments": ["dev"],
            "slotMin": 0,
            "slotDefault": 0,
            "slotMax": 0,
            "allowPortOverride": false,
            "collisionPolicy": "fail"
        },
        "surfaces": m0_surfaces(),
        "placement": {
            "stateRootTemplate": "${projectId}/${environment}/${slot}",
            "registryDir": "registry",
            "runDirTemplate": "runs/${runId}",
            "logsDirTemplate": "runs/${runId}/logs",
            "artifactsDirTemplate": "runs/${runId}/artifacts",
            "candidatePorts": {
                "start": port,
                "end": port
            },
            "slotPlacements": {
                "0": {
                    "slot": 0,
                    "stateRootTemplate": "${projectId}/${environment}/${slot}",
                    "registryDir": "registry",
                    "runDirTemplate": "runs/${runId}",
                    "logsDirTemplate": "runs/${runId}/logs",
                    "artifactsDirTemplate": "runs/${runId}/artifacts",
                    "candidatePorts": {
                        "start": port,
                        "end": port
                    }
                }
            }
        },
        "state": {
            "markerIdentity": "nixfied-m0",
            "stateEpoch": "m0",
            "cleanupPolicy": "delete-on-clean",
            "persistence": "run-scoped"
        },
        "secrets": [],
        "closures": [{
            "closureId": "m0-helper",
            "kind": "executable",
            "storePath": "/nix/store/test-m0-helper",
            "executable": executable,
            "targetSystem": host_system(),
            "operationBindings": [
                "service.synthetic.start",
                "service.synthetic.stop",
                "task.smoke.run"
            ],
            "requiresExecutable": true,
            "effects": ["process", "network-listener"]
        }],
        "execs": {
            "m0-helper": {
                "execId": "m0-helper",
                "closureId": "m0-helper",
                "executable": executable,
                "args": [],
                "env": {},
                "codebaseId": "main",
                "cwd": ".",
                "stdin": "null",
                "timeoutMs": 30000,
                "outputCapture": "stdout-stderr",
                "cancellationMode": "kill-process-group"
            }
        },
        "services": {
            "synthetic": {
                "serviceId": "synthetic",
                "foreground": true,
                "lifecycle": [
                    {
                        "operationId": "service.synthetic.start",
                        "class": "start",
                        "execId": "m0-helper",
                        "execArgs": start_args,
                        "probeId": null,
                        "terminal": {
                            "success": "spawned",
                            "failure": "failed"
                        }
                    },
                    {
                        "operationId": "service.synthetic.ready",
                        "class": "ready",
                        "execId": null,
                        "execArgs": [],
                        "probeId": "synthetic-tcp",
                        "terminal": {
                            "success": "ready",
                            "failure": "not-ready"
                        }
                    },
                    {
                        "operationId": "service.synthetic.stop",
                        "class": "stop",
                        "execId": "m0-helper",
                        "execArgs": ["stop"],
                        "probeId": null,
                        "terminal": {
                            "success": "stopped",
                            "failure": "failed"
                        }
                    }
                ],
                "endpoints": [{
                    "endpointId": "synthetic-tcp",
                    "protocol": "tcp",
                    "host": "127.0.0.1",
                    "port": {
                        "kind": "candidate-window",
                        "start": port,
                        "end": port
                    },
                    "ownershipVerification": "required",
                    "socketActivation": "disabled"
                }],
                "probes": [{
                    "probeId": "synthetic-tcp",
                    "target": {
                        "kind": "tcp-connect",
                        "endpointId": "synthetic-tcp"
                    },
                    "timeoutMs": 250,
                    "retryIntervalMs": 25,
                    "maxAttempts": 40
                }],
                "readinessProbe": "synthetic-tcp",
                "stopPolicy": {
                    "signal": "TERM",
                    "timeoutMs": 5000
                },
                "stateRefs": ["slot"],
                "logRefs": ["service.synthetic"],
                "containment": "process-group",
                "lifetime": "run-scoped",
                "identity": {
                    "serviceAddressHash": "service-address",
                    "endpointIdentityHash": "endpoint",
                    "stateIdentityHash": "state",
                    "runtimeCompatibilityHash": "runtime",
                    "targetIdentityHash": "target"
                }
            }
        },
        "tasks": {
            "smoke": {
                "taskId": "smoke",
                "operationId": "task.smoke.run",
                "execId": "m0-helper",
                "args": ["task", "--host", "127.0.0.1", "--port", "${port}"],
                "dependsOnServicesReady": ["synthetic"],
                "exitPolicy": {
                    "successCodes": [0]
                },
                "outputCapture": "stdout-stderr",
                "artifactRefs": [],
                "logRefs": ["task.smoke"],
                "summaryRefs": ["summary"]
            }
        },
        "workflows": {},
        "docs": {
            "title": "Runtime Test",
            "summary": "Runtime admission fixture."
        }
    })
}

fn m0_surface_names() -> Vec<&'static str> {
    vec![
        "model",
        "schema",
        "docs",
        "capabilities",
        "check",
        "run",
        "ps",
        "down",
        "clean",
    ]
}

fn m0_surfaces() -> Vec<Value> {
    m0_surface_names()
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "aliases": [],
                "inputSchema": {},
                "outputSchema": {},
                "exitClasses": ["ok", "error"],
                "evaluationPermission": "never",
                "maturity": "m0"
            })
        })
        .collect()
}

fn host_system() -> String {
    format!("{}-{}", host_arch(), host_os())
}

fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn host_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    }
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

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "nixfied-service-test-{}-{}",
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

fn unique_suffix() -> u128 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be available")
        .as_nanos();
    now + u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn temp_marker(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    path
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
        if canonical.starts_with("/nix/store") {
            return Some(canonical);
        }
    }
    None
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
