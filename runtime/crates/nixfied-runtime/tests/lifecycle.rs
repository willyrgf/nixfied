use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

mod common;
use common::*;

/// Kill-and-recover: the runtime is SIGKILL'd while postgres is still
/// running, leaving an orphaned service process. The next run of the same
/// manifest must stop the orphan, start a fresh process against retained
/// persistent pgdata, and complete smoke-query. Explicit purge then removes data.
///
/// Skipped when `NIXFIED_TEST_POSTGRES_MANIFEST` is not set — only the
/// nix-wrapped test runner (`.#test`) provides it.
#[test]
fn interrupt_and_recover_stops_orphan_and_starts_fresh_postgres() {
    let manifest_dir = match std::env::var("NIXFIED_TEST_POSTGRES_MANIFEST") {
        Ok(v) => v,
        Err(_) => return,
    };
    let manifest_path = format!("{manifest_dir}/manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("postgres test manifest should be readable"),
    )
    .expect("postgres test manifest should be JSON");
    let port = manifest["placement"]["slotPlacements"]["0"]["candidatePorts"]["start"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .expect("postgres test manifest should carry a valid slot-zero port window start");

    let tmp = TempDir::new();
    let state_base = tmp.path.join("state");
    // The store manifest is admitted as-is: no non-store escape hatch.
    let runtime = |operation: &str, extra: &[&str]| {
        let mut command = Command::new(runtime_binary());
        command
            .arg(operation)
            .arg("--manifest")
            .arg(&manifest_path)
            .args(extra)
            .env("NIXFIED_STATE_DIR", &state_base);
        command
    };
    let run_args = ["--task", "smoke-query", "--timeout-ms", "120000"];

    let mut child = runtime("run", &run_args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("runtime should spawn");

    let deadline = Duration::from_secs(90);
    if poll(deadline, || TcpStream::connect(("127.0.0.1", port)).ok()).is_none() {
        let _ = child.kill();
        child.wait().ok();
        panic!("postgres never came up on port {port} within 90 s");
    }

    // Listening can precede the runtime's process-record commit. Interrupt only
    // after this run has durable ownership evidence, so recovery has a row to
    // reconcile rather than accidentally exercising unrecorded-child loss.
    let recorded = poll(deadline, || {
        assert!(
            child.try_wait().unwrap().is_none(),
            "runtime exited before committed live service evidence"
        );
        try_registry_ro(&state_base)?
            .query_row(
                "SELECT pid FROM processes WHERE service_instance_id IS NOT NULL
                 AND status IN ('running', 'ready') LIMIT 1",
                [],
                |row| row.get::<_, i32>(0),
            )
            .ok()
            .filter(|pid| unsafe { libc::kill(*pid, 0) } == 0)
    });
    if recorded.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("postgres listened but no committed live service process appeared");
    }

    let pid = child.id() as libc::pid_t;
    let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    child.wait().expect("killed child should reap");

    let ps = runtime("ps", &[]).output().expect("ps should run");
    assert_success(&ps);
    let ps_json: serde_json::Value =
        serde_json::from_slice(&ps.stdout).expect("ps output should be JSON");
    let live_processes: Vec<&serde_json::Value> = ps_json["processes"]
        .as_array()
        .expect("processes should be an array")
        .iter()
        .filter(|p| p["live"] == serde_json::json!(true))
        .collect();
    assert!(
        !live_processes.is_empty(),
        "interrupted run should leave at least one live orphaned process: {ps_json}"
    );

    let (predecessor_run, predecessor_outcome): (String, Option<String>) = registry_ro(&state_base)
        .query_row("SELECT run_id, execution_outcome FROM runs", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let sentinel = state_base.join("data/postgres-example/dev/0/retained-sentinel");
    std::fs::write(&sentinel, b"retained application data").unwrap();

    // The next owner must recover the live orphan itself, without fixture
    // signaling or adoption into the new session.
    let recovery = runtime("run", &run_args)
        .output()
        .expect("recovery run should complete");
    assert_success(&recovery);

    let connection = registry_ro(&state_base);
    let (processes, runs, starts): (i64, i64, i64) = connection.query_row(
        "SELECT (SELECT count(*) FROM processes WHERE service_instance_id IS NOT NULL),
                (SELECT count(DISTINCT run_id) FROM processes WHERE service_instance_id IS NOT NULL),
                (SELECT count(*) FROM events WHERE event_type = 'service.starting')",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(
        (processes, runs, starts),
        (2, 2, 2),
        "recovery must start a fresh service"
    );
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"retained application data"
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "completed recovery session must stop postgres"
    );
    let recovered_outcome: String = connection
        .query_row(
            "SELECT execution_outcome FROM runs WHERE run_id = ?1",
            [&predecessor_run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        recovered_outcome,
        predecessor_outcome.unwrap_or_else(|| "interrupted".into())
    );
    let new_outcome: String = connection
        .query_row(
            "SELECT execution_outcome FROM runs WHERE run_id != ?1",
            [&predecessor_run],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(new_outcome, "succeeded");
    drop(connection);

    let pgdata = state_base.join("data/postgres-example/dev/0/pgdata/PG_VERSION");
    assert!(pgdata.exists(), "recovery must retain persistent pgdata");

    let clean = runtime("clean", &["--purge"])
        .output()
        .expect("clean should run");
    assert_success(&clean);
    let state_root = state_base.join("data/postgres-example/dev/0");
    assert!(
        !state_root.exists(),
        "clean should have removed the state root"
    );
}
