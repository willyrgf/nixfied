mod common;

use std::fs;
use std::num::NonZeroU64;
use std::time::Duration;

use common::*;
use nixfied_runtime::admission::{AdmissionContext, StoreOriginPolicy, admit_control};
use nixfied_runtime::cancellation::{CancellationToken, WaitBudget};
use nixfied_runtime::control::observation::{Selector, await_selected, context, inspect};
use nixfied_runtime::state::{SlotLocation, derive_slot_location};
use nixfied_runtime::{ControlAdmission, ErrorCode};
use serde_json::{Value, json};

fn budget(milliseconds: u64) -> WaitBudget {
    WaitBudget::new(NonZeroU64::new(milliseconds).unwrap())
}

struct Owner {
    fixture: RuntimeFixture,
    admission: ControlAdmission,
    location: SlotLocation,
    acknowledgement: Value,
    _markers: TempDir,
}

impl Owner {
    fn start(health_barrier: bool) -> (Self, std::path::PathBuf) {
        let (mut owner, release, marker) = Self::prepared(health_barrier);
        let output = owner
            .fixture
            .output("run", &["--task", "owner", "--daemon"]);
        assert_success(&output);
        owner.acknowledgement = serde_json::from_slice(&output.stdout).unwrap();
        assert!(wait_for_path(&marker, Duration::from_secs(5)));
        (owner, release)
    }

    fn prepared(health_barrier: bool) -> (Self, std::path::PathBuf, std::path::PathBuf) {
        let markers = TempDir::new();
        let counter = markers.path.join("initialize-count");
        let hold = markers.path.join("holding");
        let release = markers.path.join("release-health");
        let port = available_port_window(1);
        let mut value = test_child_manifest(port, port);
        add_task_clone(
            &mut value,
            "initialize",
            &["synthetic"],
            &["output", "occurrence", counter.to_str().unwrap(), "1"],
        );
        value["tasks"]["initialize"]["exitPolicy"]["successCodes"] = json!([7]);
        add_task_clone(
            &mut value,
            "hold",
            &[],
            &["output", "hex-block", "", "", hold.to_str().unwrap()],
        );
        clear_task_deadline(&mut value, "hold");
        value["tasks"]["owner"] = json!({"kind":"composite","steps":{
            "initialize":{"task":"initialize"},
            "hold":{"task":"hold","dependsOn":["initialize"]}
        }});
        if health_barrier {
            let health =
                &mut value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["healthProbe"];
            let program = health["run"][0].clone();
            health["run"] = json!([
                program,
                "prepare",
                markers.path.join("health-started"),
                release
            ]);
            value["services"]["synthetic"]["lifecycle"]["health"]["policy"]["timeoutMs"] =
                json!(10000);
        }
        let fixture = RuntimeFixture::new(value);
        let admission = admit_control(
            &fixture.manifest_path,
            &AdmissionContext::current(StoreOriginPolicy::AllowNonStoreForTests),
        )
        .unwrap();
        let location =
            derive_slot_location(admission.project_id(), "dev", 0, &fixture.state_base).unwrap();
        let marker = if health_barrier {
            markers.path.join("health-started")
        } else {
            hold
        };
        let owner = Self {
            fixture,
            admission,
            location,
            acknowledgement: Value::Null,
            _markers: markers,
        };
        (owner, release, marker)
    }

    fn observe(
        &self,
    ) -> nixfied_runtime::RuntimeResult<nixfied_runtime::control::observation::TaskObservation>
    {
        await_selected(
            &self.admission,
            &self.location,
            &Selector::current(),
            "owner.initialize",
            &budget(5000),
            &CancellationToken::new(),
        )
    }
}

#[test]
fn concurrent_ensure_commands_commit_one_owner_and_one_canonical_initialization() {
    let (owner, release, health_started) = Owner::prepared(true);
    let mut children = Vec::new();
    for _ in 0..8 {
        children.push(
            owner
                .fixture
                .command(
                    "ensure",
                    &[
                        "--task",
                        "owner",
                        "--until-step",
                        "owner.initialize",
                        "--wait-timeout-ms",
                        "10000",
                    ],
                )
                .spawn()
                .unwrap(),
        );
    }
    assert!(wait_for_path(&health_started, Duration::from_secs(5)));
    fs::write(release, []).unwrap();
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    let mut established = 0;
    let mut selected = None;
    for output in outputs {
        assert_success(&output);
        let wire: Value = serde_json::from_slice(&output.stdout).unwrap();
        if wire["disposition"] == "established" {
            established += 1;
        } else {
            assert_eq!(wire["disposition"], "observed");
        }
        let run = wire["observation"]["session"]["runId"]
            .as_str()
            .unwrap()
            .to_owned();
        if let Some(expected) = &selected {
            assert_eq!(&run, expected);
        } else {
            selected = Some(run);
        }
        assert_eq!(wire["observation"]["task"]["exitCode"], 7);
    }
    assert_eq!(established, 1);
    let connection = owner.fixture.registry();
    let counts: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT count(*) FROM runs),
        (SELECT count(*) FROM events WHERE event_type='run.created'),
        (SELECT count(*) FROM processes WHERE role='service'),
        (SELECT count(*) FROM processes WHERE root_step_path='owner.initialize')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
    assert_eq!(
        fs::read_to_string(owner._markers.path.join("initialize-count")).unwrap(),
        "1"
    );
}

#[test]
fn ensure_establishes_and_reuses_full_u64_timeouts_without_an_instant_overflow() {
    let (owner, _, _) = Owner::prepared(false);
    for disposition in ["established", "observed"] {
        let output = owner.fixture.output(
            "ensure",
            &[
                "--task",
                "owner",
                "--until-step",
                "owner.initialize",
                "--timeout-ms",
                "18446744073709551615",
                "--wait-timeout-ms",
                "18446744073709551615",
            ],
        );
        assert_success(&output);
        let wire: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(wire["disposition"], disposition);
        assert_eq!(
            wire["observation"]["session"]["request"]["operationTimeoutMs"],
            u64::MAX
        );
    }
    let stored: (String, String, i64) = owner.fixture.registry().query_row(
        "SELECT operation_timeout_ms, typeof(operation_timeout_ms), (SELECT count(*) FROM runs) FROM runs", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(stored, ("18446744073709551615".into(), "text".into(), 1));
}

#[test]
fn initiating_observer_signal_after_commit_leaves_shared_owner_and_other_observers_alive() {
    let (owner, release, health_started) = Owner::prepared(true);
    let initiator = owner
        .fixture
        .command(
            "ensure",
            &[
                "--task",
                "owner",
                "--until-step",
                "owner.initialize",
                "--wait-timeout-ms",
                "10000",
            ],
        )
        .spawn()
        .unwrap();
    assert!(wait_for_path(&health_started, Duration::from_secs(5)));
    let raw_owner: String = owner
        .fixture
        .registry()
        .query_row("SELECT owner_identity FROM runs", [], |row| row.get(0))
        .unwrap();
    let raw_owner: Value = serde_json::from_str(&raw_owner).unwrap();
    let owner_pid = raw_owner["pid"].as_i64().unwrap() as i32;
    let other = owner
        .fixture
        .command(
            "ensure",
            &[
                "--task",
                "owner",
                "--until-step",
                "owner.initialize",
                "--wait-timeout-ms",
                "10000",
            ],
        )
        .spawn()
        .unwrap();
    assert_eq!(
        unsafe { libc::kill(initiator.id() as i32, libc::SIGTERM) },
        0
    );
    let canceled = initiator.wait_with_output().unwrap();
    assert_eq!(canceled.status.code(), Some(27));
    let error: Value = serde_json::from_slice(&canceled.stderr).unwrap();
    assert_eq!(error["code"], "CANCELED");
    assert_eq!(unsafe { libc::kill(owner_pid, 0) }, 0);
    fs::write(release, []).unwrap();
    assert_success(&other.wait_with_output().unwrap());
    assert_eq!(
        owner
            .fixture
            .registry()
            .query_row("SELECT count(*) FROM runs", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn reuse_rejects_request_timeout_difference_and_full_admission_still_precedes_selection() {
    let (owner, _) = Owner::start(false);
    let mismatch = owner.fixture.output(
        "ensure",
        &[
            "--task",
            "owner",
            "--until-step",
            "owner.initialize",
            "--timeout-ms",
            "0",
        ],
    );
    assert_eq!(mismatch.status.code(), Some(40));
    let error: Value = serde_json::from_slice(&mismatch.stderr).unwrap();
    assert_eq!(
        error["details"]["sessionObservation"],
        json!({"kind":"request-mismatch", "field":"operation-timeout",
        "selection":{"state":"pinned","runId":owner.acknowledgement["runId"]}})
    );
    let rejected = owner
        .fixture
        .output("ensure", &["--task", "owner", "--until-step", "owner.hold"]);
    assert_eq!(rejected.status.code(), Some(37));
    assert_eq!(
        owner
            .fixture
            .registry()
            .query_row("SELECT count(*) FROM runs", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.fixture.output("down", &["--timeout-ms", "5000"]);
    }
}

#[test]
fn absence_and_context_are_read_only_and_exact_run_inspection_never_starts_work() {
    let fixture = RuntimeFixture::new(test_child_manifest(23080, 23080));
    let admission = admit_control(
        &fixture.manifest_path,
        &AdmissionContext::current(StoreOriginPolicy::AllowNonStoreForTests),
    )
    .unwrap();
    let location =
        derive_slot_location(admission.project_id(), "dev", 0, &fixture.state_base).unwrap();
    let resolved = serde_json::to_value(context(&admission, &location).unwrap()).unwrap();
    assert_eq!(
        resolved,
        json!({"projectId":"runtime-test","environment":"dev","slot":0,
        "stateBase":fixture.state_base,"stateRoot":fixture.state_base.join("data/runtime-test/dev/0"),
        "registryDir":fixture.state_base.join("registry/runtime-test/dev/0"),
        "registryPath":fixture.state_base.join("registry/runtime-test/dev/0/registry.sqlite3")})
    );
    for selector in [
        Selector::current(),
        Selector::run_id("unknown-run").unwrap(),
    ] {
        let wire = serde_json::to_value(
            inspect(
                &admission,
                &location,
                &selector,
                &budget(1000),
                &CancellationToken::new(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(wire, json!({"result":"absent","context":resolved}));
        let error = await_selected(
            &admission,
            &location,
            &selector,
            "owner.initialize",
            &budget(1000),
            &CancellationToken::new(),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::SessionUnavailable);
        assert_eq!(error.details["sessionObservation"]["reason"], "absent");
    }
    assert!(!fixture.state_base.exists());
}

#[test]
fn committed_root_evidence_accepts_nonzero_exit_and_returns_independent_capture_files() {
    let (owner, _) = Owner::start(false);
    let observation = serde_json::to_value(owner.observe().unwrap()).unwrap();
    assert_eq!(
        observation["session"]["runId"],
        owner.acknowledgement["runId"]
    );
    assert_eq!(
        observation["session"]["progress"],
        json!({"state":"outcome-pending"})
    );
    assert_eq!(observation["session"]["owner"]["observation"], "live");
    assert_eq!(
        observation["session"]["services"][0]["leader"]["observation"],
        "live"
    );
    assert_eq!(observation["task"]["stepPath"], "owner.initialize");
    assert_eq!(observation["task"]["taskId"], "initialize");
    assert_eq!(observation["task"]["exitCode"], 7);
    let stdout = observation["task"]["stdoutPath"].as_str().unwrap();
    let stderr = observation["task"]["stderrPath"].as_str().unwrap();
    assert_eq!(fs::read(stdout).unwrap(), b"stdout occurrence 1\n");
    assert_eq!(fs::read(stderr).unwrap(), b"stderr occurrence 1\n");
    let registry = owner.fixture.registry();
    let (runs, services, root): (i64, i64, String) = registry.query_row(
        "SELECT (SELECT count(*) FROM runs), (SELECT count(*) FROM processes WHERE role='service'), (SELECT root_step_path FROM processes WHERE process_key=?1)",
        [observation["task"]["processKey"].as_str().unwrap()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!((runs, services, root.as_str()), (1, 1, "owner.initialize"));
}

#[test]
fn ready_status_before_initial_health_never_satisfies_root_observation() {
    let (owner, release) = Owner::start(true);
    let error = await_selected(
        &owner.admission,
        &owner.location,
        &Selector::current(),
        "owner.initialize",
        &budget(30),
        &CancellationToken::new(),
    )
    .unwrap_err();
    let status: String = owner
        .fixture
        .registry()
        .query_row(
            "SELECT status FROM processes WHERE role='service'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    fs::write(&release, []).unwrap();
    assert_eq!(status, "ready");
    assert_eq!(error.code, ErrorCode::ObservationTimeout);
    assert_eq!(error.details["sessionObservation"]["stage"], "task");
    assert!(owner.observe().is_ok());
}

#[test]
fn partial_terminal_evidence_and_finished_capture_with_failed_settlement_are_unavailable() {
    let (owner, _) = Owner::start(false);
    owner.observe().unwrap();
    for sql in [
        "UPDATE processes SET execution_outcome=NULL, exit_code=NULL WHERE root_step_path='owner.initialize'",
        "UPDATE processes SET status='running', ownership='unresolved' WHERE root_step_path='owner.initialize'",
    ] {
        let connection = rusqlite::Connection::open(owner.location.registry_path()).unwrap();
        connection.execute(sql, []).unwrap();
        let observed = owner.observe();
        connection.execute("UPDATE processes SET status='succeeded', ownership='settled', execution_outcome='succeeded', exit_code=7 WHERE root_step_path='owner.initialize'", []).unwrap();
        let error = observed.unwrap_err();
        assert_eq!(error.code, ErrorCode::SessionUnavailable);
        assert_eq!(
            error.details["sessionObservation"]["reason"],
            "evidence-incomplete"
        );
    }
}

#[test]
fn settled_service_with_live_leader_is_unavailable_and_observer_cancel_leaves_owner_alive() {
    let (owner, _) = Owner::start(false);
    let wire = serde_json::to_value(owner.observe().unwrap()).unwrap();
    let pid = wire["session"]["owner"]["pid"].as_u64().unwrap() as i32;
    let connection = rusqlite::Connection::open(owner.location.registry_path()).unwrap();
    connection.execute("UPDATE processes SET status='stopped', ownership='settled', execution_outcome='failed' WHERE role='service'", []).unwrap();
    let observed = owner.observe();
    connection.execute("UPDATE processes SET status='ready', ownership='unresolved', execution_outcome=NULL WHERE role='service'", []).unwrap();
    let error = observed.unwrap_err();
    assert_eq!(
        error.details["sessionObservation"]["reason"],
        "service-settled"
    );
    let canceled = CancellationToken::new();
    canceled.cancel();
    let error = await_selected(
        &owner.admission,
        &owner.location,
        &Selector::current(),
        "owner.initialize",
        &budget(1),
        &canceled,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Canceled);
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    assert!(owner.observe().is_ok());
}
