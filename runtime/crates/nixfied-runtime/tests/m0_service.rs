use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use nixfied_model::{DirtyPolicy, Model, SourceMode};
use nixfied_runtime::registry::{Registry, RegistryIdentity};
use nixfied_runtime::service::{
    run_dependent_task, service_address_hash, service_instance_id, start_synthetic_service,
    start_synthetic_service_for_slot, wait_for_readiness_probe,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    derive_host_placement, derive_host_placement_for_slot, materialize_run_roots,
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

    assert_eq!(error.code, ErrorCode::ModelAdmission);
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
fn ps_reconciles_dead_owned_process_as_stale_and_releases_port() {
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
    let released_ports: i64 = fixture
        .registry
        .connection()
        .query_row(
            "SELECT count(*) FROM ports WHERE status = 'released'",
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
    assert_eq!(released_ports, 1);
    assert_eq!(stale_events, 1);
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
        "toolchainId": "nixfied-toolchain:m1:1",
        "runtimeAbi": "nixfied-runtime-abi:m1:1",
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
                        "kind": "fixed",
                        "port": port
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
