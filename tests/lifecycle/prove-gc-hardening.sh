#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_root="${NIXFIED_TEST_TMPDIR:-${TMPDIR:-/tmp}}"
if [[ "$(uname -s)" == "Darwin" && "$tmp_root" == /var/* && -d /private/tmp ]]; then
  tmp_root="/private/tmp"
fi
tmp_root="$(cd "$tmp_root" && pwd -P)"
tmp="$(mktemp -d "$tmp_root/nixfied-gc-proof.XXXXXX")"
live_pids=()
live_pgids=()
cleanup() {
  for pgid in "${live_pgids[@]:-}"; do
    kill -TERM "-$pgid" 2>/dev/null || true
    kill -KILL "-$pgid" 2>/dev/null || true
  done
  for pid in "${live_pids[@]:-}"; do
    kill -TERM "$pid" 2>/dev/null || true
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$tmp"
}
trap cleanup EXIT
cd "$repo"

run_exact_test() {
  local test_target="$1"
  local test_name="$2"
  local list_output
  list_output="$(cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test "$test_target" -- --list)"
  if ! grep -Fxq "$test_name: test" <<<"$list_output"; then
    echo "expected test $test_target::$test_name to exist" >&2
    exit 1
  fi
  cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test "$test_target" "$test_name" -- --exact
}

wait_for_process_row_and_stop_runtime() {
  local runtime_pid="$1"
  local db="$2"
  python3 - "$runtime_pid" "$db" <<'PY'
import os
import pathlib
import signal
import sqlite3
import sys
import time

runtime_pid = int(sys.argv[1])
db = pathlib.Path(sys.argv[2])
deadline = time.monotonic() + 10
while time.monotonic() < deadline:
    try:
        os.kill(runtime_pid, 0)
    except ProcessLookupError:
        raise SystemExit("runtime exited before service process row was visible")
    if db.exists():
        try:
            with sqlite3.connect(db) as conn:
                row = conn.execute(
                    """
                    SELECT pgid
                    FROM processes
                    WHERE service_instance_id IS NOT NULL
                      AND status IN ('starting','running','ready')
                    ORDER BY rowid DESC
                    LIMIT 1
                    """
                ).fetchone()
            if row is not None:
                os.kill(runtime_pid, signal.SIGSTOP)
                print(row[0])
                raise SystemExit(0)
        except sqlite3.Error:
            pass
    time.sleep(0.005)
raise SystemExit(f"timed out waiting for service process row in {db}")
PY
}

wait_for_group_empty() {
  local pgid="$1"
  python3 - "$pgid" <<'PY'
import subprocess
import sys
import time

pgid = int(sys.argv[1])
deadline = time.monotonic() + 5
while time.monotonic() < deadline:
    output = subprocess.check_output(["ps", "-axo", "pgid=,stat="], text=True)
    live = False
    for line in output.splitlines():
        fields = line.split()
        if len(fields) < 2:
            continue
        try:
            current = int(fields[0])
        except ValueError:
            continue
        if current == pgid and not fields[1].startswith("Z"):
            live = True
            break
    if not live:
        raise SystemExit(0)
    time.sleep(0.025)
raise SystemExit(f"process group {pgid} still has non-zombie members")
PY
}

assert_crashed_owner_is_live() {
  local report="$1"
  python3 - "$report" <<'PY'
import json
import pathlib
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text())
service = next(
    process for process in report["processes"]
    if process["serviceInstanceId"] is not None
)
assert service["live"] is True, report
assert service["reconciledStatus"] == "running", report
PY
}

assert_stale_after_os_death() {
  local report="$1"
  local db="$2"
  python3 - "$report" "$db" <<'PY'
import json
import pathlib
import sqlite3
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text())
db = pathlib.Path(sys.argv[2])
service = next(
    process for process in report["processes"]
    if process["serviceInstanceId"] is not None
)
assert service["live"] is False, report
assert service["reconciledStatus"] == "stale", report
with sqlite3.connect(db) as conn:
    lease_statuses = [row[0] for row in conn.execute("SELECT status FROM run_leases")]
    port_statuses = [row[0] for row in conn.execute("SELECT status FROM ports")]
    process_events = conn.execute(
        "SELECT count(*) FROM events WHERE event_type = 'process.stale'"
    ).fetchone()[0]
    lease_events = conn.execute(
        "SELECT count(*) FROM events WHERE event_type = 'run.lease-stale'"
    ).fetchone()[0]
assert lease_statuses == ["stale"], lease_statuses
assert port_statuses == ["stale"], port_statuses
assert process_events == 1
assert lease_events == 1
PY
}

assert_down_reports_stale() {
  local report="$1"
  python3 - "$report" <<'PY'
import json
import pathlib
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert report["stale"], report
assert report["stopped"] == [], report
PY
}

prove_kill9_crash_reconciliation() {
  local model_out runtime_bin state_base db runtime_pid service_pgid
  model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo#minimal-model")"
  cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime
  runtime_bin="$repo/runtime/target/debug/nixfied-runtime"
  state_base="$tmp/state"
  db="$state_base/minimal/dev/0/registry/registry.sqlite3"

  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" run --model "$model_out/model.json" >"$tmp/run.json" 2>"$tmp/run.err" &
  runtime_pid=$!
  live_pids+=("$runtime_pid")
  service_pgid="$(wait_for_process_row_and_stop_runtime "$runtime_pid" "$db")"
  live_pgids+=("$service_pgid")
  kill -KILL "$runtime_pid"
  wait "$runtime_pid" 2>/dev/null || true
  live_pids=()

  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" >"$tmp/ps-live-after-runtime-kill.json"
  assert_crashed_owner_is_live "$tmp/ps-live-after-runtime-kill.json"

  kill -KILL "-$service_pgid" 2>/dev/null || true
  wait_for_group_empty "$service_pgid"
  live_pgids=()
  python3 - "$db" <<'PY'
import sqlite3
import sys

with sqlite3.connect(sys.argv[1]) as conn:
    conn.execute(
        """
        UPDATE run_leases
        SET expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 seconds')
        WHERE status IN ('active','canceling')
        """
    )
PY

  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" >"$tmp/ps-stale-after-os-death.json"
  assert_stale_after_os_death "$tmp/ps-stale-after-os-death.json" "$db"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" down --model "$model_out/model.json" >"$tmp/down-after-stale.json"
  assert_down_reports_stale "$tmp/down-after-stale.json"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" >"$tmp/clean-after-stale.json"
  test ! -e "$state_base/minimal/dev/0"
}

run_exact_test state cleanup_refuses_unmarked_roots
run_exact_test state cleanup_refuses_path_escape
run_exact_test state cleanup_refuses_marker_mismatch
run_exact_test state cleanup_refuses_protected_state
run_exact_test state cleanup_refuses_persistent_state
run_exact_test state cleanup_refuses_symlink_traversal
run_exact_test state cleanup_refuses_active_registry_refs
run_exact_test state cleanup_deletes_matching_inactive_state
run_exact_test state cleanup_finishes_interrupted_delete_when_target_is_already_absent
run_exact_test state cleanup_delete_failure_records_failed_without_deleted_success
run_exact_test state clean_reconciles_stale_refs_before_marker_owned_delete
run_exact_test state clean_marks_active_port_stale_after_owner_process_is_proven_dead
run_exact_test registry rejects_incompatible_registry_identity
run_exact_test registry rejects_incompatible_user_version
run_exact_test registry rejects_existing_v1_registry_missing_required_shape
run_exact_test registry rejects_nonempty_unversioned_registry
run_exact_test service ps_reconciles_dead_owned_process_and_port_as_stale
run_exact_test service down_completes_canceling_lease_and_unblocks_cleanup
run_exact_test service down_cancels_live_task_process_group_and_unblocks_cleanup

prove_kill9_crash_reconciliation
