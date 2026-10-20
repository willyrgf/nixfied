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
        Self::prepared_with(health_barrier, |_, _| {})
    }

    fn prepared_with(
        health_barrier: bool,
        configure: impl FnOnce(&mut Value, &std::path::Path),
    ) -> (Self, std::path::PathBuf, std::path::PathBuf) {
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
        configure(&mut value, &markers.path);
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
fn observation_checks_the_whole_group_with_endpointless_and_multiple_endpoint_members() {
    let (owner, _, holding) = Owner::prepared_with(false, |value, markers| {
        let port = available_port_window(2);
        value["placement"]["slotPlacements"]["0"]["candidatePorts"] =
            json!({"start":port,"end":port + 1});
        let mut second = value["services"]["synthetic"]["endpoints"]["synthetic-tcp"].clone();
        second["endpointId"] = json!("z-v6");
        second["host"] = json!("::1");
        value["services"]["synthetic"]["endpoints"]["z-v6"] = second;
        let program =
            value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"][0].clone();
        value["services"]["synthetic"]["lifecycle"]["start"]["invocation"]["run"] = json!([
            program,
            "listen-pair",
            "${port:synthetic-tcp}",
            "${port:z-v6}",
            markers.join("never-replace"),
            markers.join("replaced")
        ]);
        add_service_clone(
            value,
            "aux",
            &[
                "output",
                "hex-block",
                "",
                "",
                markers.join("aux-started").to_str().unwrap(),
            ],
            &[],
        );
        let probe =
            value["services"]["synthetic"]["endpoints"]["synthetic-tcp"]["readyProbe"].clone();
        let service = value["services"]["aux"].as_object_mut().unwrap();
        service.remove("endpoints");
        service.remove("primaryEndpoint");
        value["services"]["aux"]["lifecycle"]["start"]["invocation"]["tools"] =
            json!(["fixture-probe"]);
        value["services"]["aux"]["lifecycle"]["ready"]["probe"] = probe.clone();
        value["services"]["aux"]["lifecycle"]["health"]["probe"] = probe;
        value["services"]["synthetic"]["connectsTo"] = json!(["aux"]);
    });
    let output = owner.fixture.output(
        "ensure",
        &[
            "--task",
            "owner",
            "--until-step",
            "owner.initialize",
            "--wait-timeout-ms",
            "5000",
        ],
    );
    assert_success(&output);
    assert!(wait_for_path(&holding, Duration::from_secs(3)));
    assert!(owner._markers.path.join("aux-started").exists());
    let wire: Value = serde_json::from_slice(&output.stdout).unwrap();
    let services = &wire["observation"]["session"]["services"];
    assert_eq!(services[0]["serviceId"], "aux");
    assert_eq!(services[0]["endpoints"], json!([]));
    assert_eq!(services[1]["serviceId"], "synthetic");
    assert_eq!(services[1]["endpoints"][0]["endpointId"], "synthetic-tcp");
    assert_eq!(services[1]["endpoints"][1]["endpointId"], "z-v6");
    assert_eq!(services[1]["endpoints"][1]["host"], "::1");
    let raw = rusqlite::Connection::open(owner.location.registry_path()).unwrap();
    let counts: (i64, i64, i64) = raw
        .query_row(
            "SELECT (SELECT count(*) FROM runs),
        (SELECT count(*) FROM processes WHERE role='service'), (SELECT count(*) FROM ports)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 2, 2));
    let key = services[0]["processKey"].as_str().unwrap();
    raw.execute(
        "UPDATE processes SET service_name='synthetic' WHERE process_key=?1",
        [key],
    )
    .unwrap();
    let observed = owner.observe();
    raw.execute(
        "UPDATE processes SET service_name='aux' WHERE process_key=?1",
        [key],
    )
    .unwrap();
    assert_eq!(observed.unwrap_err().code, ErrorCode::RegistryCorrupt);
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
fn committed_shared_helper_survives_observer_abandonment_before_any_ack_can_be_delivered() {
    use nixfied_runtime::admission::{InvocationRoot, admit_run};
    use nixfied_runtime::registry::session::{DeclaredRunRequest, RecordedRunRequestWire};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let (owner, release, health_started) = Owner::prepared(true);
    let mut context = AdmissionContext::current(StoreOriginPolicy::AllowNonStoreForTests);
    context.invocation_root = InvocationRoot::Path(owner.fixture.tmp.path.clone());
    let admitted = admit_run(&owner.fixture.manifest_path, &context).unwrap();
    let expected =
        DeclaredRunRequest::new(&admitted, &nixfied_manifest::TaskId::new("owner"), 5000)
            .unwrap()
            .recorded()
            .unwrap();
    let (mut observer, mut channel) = UnixStream::pair().unwrap();
    observer.set_nonblocking(true).unwrap();
    channel.set_nonblocking(true).unwrap();
    let size: libc::c_int = 4096;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                channel.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&size as *const libc::c_int).cast(),
                std::mem::size_of_val(&size) as libc::socklen_t,
            )
        },
        0
    );
    // Fill the helper-to-observer queue independently of the protocol. The
    // observer never reads it, so no acknowledgement byte can be delivered.
    let mut filled = 0;
    loop {
        match channel.write(&[b'x'; 4096]) {
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            result => panic!("cannot arrange bounded reply barrier: {result:?}"),
        }
    }
    assert!(filled > 0);
    let inherited = channel.as_raw_fd();
    let parent = observer.as_raw_fd();
    let mut command = Command::new(runtime_binary());
    command
        .args(["__session-owner", &inherited.to_string()])
        .current_dir(&owner.fixture.tmp.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            if libc::close(parent) != 0
                || libc::fcntl(inherited, libc::F_SETFD, 0) != 0
                || libc::setsid() < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let helper = command.spawn().unwrap();
    drop(channel);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let body = serde_json::to_vec(&json!({"purpose":"shared", "runId":"run-before-ack", "execution":{
            "manifestPath":owner.fixture.manifest_path,"allowNonStoreManifest":true,"task":"owner", "slot":0,
            "stateBase":owner.fixture.state_base,"operationTimeoutMs":5000},
            "expectedRequest":RecordedRunRequestWire::from_recorded(&expected),
            "cursor":{"watermark":0,"history":{"state":"unseen"}}})).unwrap();
        let mut frame = b"NXD2".to_vec();
        frame.extend((body.len() as u32).to_be_bytes());
        frame.extend(body);
        observer.write_all(&frame).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let publication = loop {
            if owner.location.registry_path().exists() {
                let read = (|| -> rusqlite::Result<Option<(i64, i64, i64)>> {
                    let raw = rusqlite::Connection::open_with_flags(
                        owner.location.registry_path(),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )?;
                    raw.busy_timeout(Duration::ZERO)?;
                    let exists: bool = raw.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='events' AND type='table')", [], |row| row.get(0))?;
                    if !exists {
                        return Ok(None);
                    }
                    raw.query_row("SELECT (SELECT count(*) FROM runs),
                        (SELECT count(*) FROM events WHERE event_type='run.created' AND run_id='run-before-ack'),
                        (SELECT count(*) FROM processes)", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map(Some)
                })();
                match read {
                    Ok(Some(counts)) if counts.1 == 1 => break counts,
                    Ok(_) => {}
                    Err(rusqlite::Error::SqliteFailure(error, _))
                        if matches!(
                            error.code,
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        ) => {}
                    Err(error) => panic!("independent publication inspection failed: {error}"),
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no independent committed publication"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            publication,
            (1, 1, 0),
            "publication precedes reply delivery and workloads"
        );
        observer.shutdown(std::net::Shutdown::Write).unwrap();
        drop(observer);
        assert!(
            wait_for_path(&health_started, Duration::from_secs(5)),
            "committed helper stopped when its observer left"
        );
        fs::write(release, []).unwrap();
        let observed = serde_json::to_value(owner.observe().unwrap()).unwrap();
        assert_eq!(observed["session"]["runId"], "run-before-ack");
        assert_eq!(
            fs::read_to_string(owner._markers.path.join("initialize-count")).unwrap(),
            "1"
        );
    }));
    let stopped = owner.fixture.output("down", &["--timeout-ms", "5000"]);
    let finished = wait_for_child_output(helper, Duration::from_secs(5));
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
    assert_success(&stopped);
    assert_eq!(finished.status.code(), Some(27));
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
