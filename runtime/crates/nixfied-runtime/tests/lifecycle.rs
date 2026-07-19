use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

mod common;
use common::*;

/// Kill-and-recover: the runtime is SIGKILL'd while postgres is still
/// running, leaving an orphaned service process. The next run of the same
/// model must reconcile the evidence, stop the orphan, adopt the pgdata
/// cluster, and complete smoke-query against the live cluster. Clean then
/// removes the state root.
///
/// Skipped when `NIXFIED_TEST_POSTGRES_MODEL` is not set — only the
/// nix-wrapped test runner (`.#test`) provides it.
#[test]
fn interrupt_and_recover_adopts_orphaned_postgres() {
    let model_dir = match std::env::var("NIXFIED_TEST_POSTGRES_MODEL") {
        Ok(v) => v,
        Err(_) => return,
    };
    let model_path = format!("{model_dir}/model.json");
    let model: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&model_path).expect("postgres test model should be readable"),
    )
    .expect("postgres test model should be JSON");
    let port = model["placement"]["slotPlacements"]["0"]["candidatePorts"]["start"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .expect("postgres test model should carry a valid slot-zero port window start");

    let tmp = TempDir::new();
    let state_base = tmp.path.join("state");
    std::fs::create_dir_all(&state_base).expect("state base should be created");

    let mut child = Command::new(runtime_binary())
        .arg("run")
        .arg("--model")
        .arg(&model_path)
        .arg("--task")
        .arg("smoke-query")
        .arg("--timeout-ms")
        .arg("120000")
        .env("NIXFIED_STATE_DIR", &state_base)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("runtime should spawn");

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut postgres_up = false;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            postgres_up = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !postgres_up {
        let _ = child.kill();
        child.wait().ok();
        panic!("postgres never came up on port {port} within 90 s");
    }

    let pid = child.id() as libc::pid_t;
    let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    child.wait().expect("killed child should reap");

    let ps = Command::new(runtime_binary())
        .arg("ps")
        .arg("--model")
        .arg(&model_path)
        .env("NIXFIED_STATE_DIR", &state_base)
        .output()
        .expect("ps should run");
    assert!(
        ps.status.success(),
        "ps after interrupt failed: {}",
        String::from_utf8_lossy(&ps.stderr)
    );
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

    // Stop the orphaned postgres process group so run_has_live_process returns
    // false, then directly expire the stale run lease so recovery can acquire a
    // new lease immediately without waiting for the 30s TTL (white-box test).
    let mut seen_pgids = std::collections::BTreeSet::new();
    for process in &live_processes {
        if let Some(pgid) = process["pgid"].as_i64()
            && pgid > 0
            && seen_pgids.insert(pgid)
        {
            let _ = unsafe { libc::kill(-(pgid as libc::pid_t), libc::SIGKILL) };
        }
    }
    thread::sleep(Duration::from_millis(500));

    let registry_path = state_base.join("registry/postgres-example/dev/0/registry.sqlite3");
    let expire = Command::new("sqlite3")
        .arg(&registry_path)
        .arg(
            "UPDATE run_leases SET expires_at = '2000-01-01T00:00:00.000Z' \
             WHERE status IN ('active', 'canceling')",
        )
        .output()
        .expect("sqlite3 must be on PATH — nixfied test environment provides pkgs.sqlite");
    assert!(
        expire.status.success(),
        "sqlite3 lease expiry update failed: {}",
        String::from_utf8_lossy(&expire.stderr)
    );

    let recovery = Command::new(runtime_binary())
        .arg("run")
        .arg("--model")
        .arg(&model_path)
        .arg("--task")
        .arg("smoke-query")
        .arg("--timeout-ms")
        .arg("120000")
        .env("NIXFIED_STATE_DIR", &state_base)
        .output()
        .expect("recovery run should complete");
    assert!(
        recovery.status.success(),
        "recovery run failed: {}",
        String::from_utf8_lossy(&recovery.stderr)
    );

    let pgdata = state_base.join("postgres-example/dev/0/pgdata/PG_VERSION");
    assert!(
        pgdata.exists(),
        "recovery run should have adopted the existing pgdata cluster"
    );

    let clean = Command::new(runtime_binary())
        .arg("clean")
        .arg("--model")
        .arg(&model_path)
        .env("NIXFIED_STATE_DIR", &state_base)
        .output()
        .expect("clean should run");
    assert!(
        clean.status.success(),
        "clean after recovery failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let state_root = state_base.join("postgres-example/dev/0");
    assert!(
        !state_root.exists(),
        "clean should have removed the state root"
    );
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
