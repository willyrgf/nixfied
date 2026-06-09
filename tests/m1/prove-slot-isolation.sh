#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_root="${NIXFIED_TEST_TMPDIR:-${TMPDIR:-/tmp}}"
if [[ "$(uname -s)" == "Darwin" && "$tmp_root" == /var/* && -d /private/tmp ]]; then
  tmp_root="/private/tmp"
fi
tmp_root="$(cd "$tmp_root" && pwd -P)"
tmp="$(mktemp -d "$tmp_root/nixfied-m1-slot-proof.XXXXXX")"
live_pids=()
cleanup() {
  for pid in "${live_pids[@]:-}"; do
    kill -TERM "$pid" 2>/dev/null || true
    kill -CONT "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$tmp"
}
trap cleanup EXIT
cd "$repo"

project="$tmp/project"
mkdir -p "$project"
cat >"$project/flake.nix" <<EOF
{
  description = "Nixfied M1 slot isolation proof project";

  inputs = {
    nixfied.url = "git+file://$repo";
  };

  outputs =
    { self, nixfied }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = f: builtins.listToAttrs (
        map (system: {
          name = system;
          value = f system;
        }) systems
      );
    in
    {
      packages = forAllSystems (system: {
        default = nixfied.lib.\${system}.compileModel ./nixfied.nix;
        model = nixfied.lib.\${system}.compileModel ./nixfied.nix;
      });
    };
}
EOF

cat >"$project/nixfied.nix" <<'EOF'
{ adapters, ... }:
{
  imports = [ adapters.synthetic ];
  nixfied.project.projectId = "m1-slot-isolation";
  nixfied.project.name = "M1 Slot Isolation";
  nixfied.codebases.main.logicalRoot = ".";
  nixfied.slotPolicy.max = 1;
  nixfied.placement.ports.base = 39180;
  nixfied.placement.ports.windowSize = 11;
  nixfied.placement.ports.slotStride = 100;
}
EOF

model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$project#model")"
case "$model_out" in
  /nix/store/*) ;;
  *)
    echo "expected model output under /nix/store, got: $model_out" >&2
    exit 1
    ;;
esac

python3 - "$model_out/model.json" <<'PY'
import json
import pathlib
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert model["slotPolicy"] == {"min": 0, "default": 0, "max": 1}
assert model["runtimeConstraints"]["slotMin"] == 0
assert model["runtimeConstraints"]["slotDefault"] == 0
assert model["runtimeConstraints"]["slotMax"] == 1
assert model["capabilities"]["slots"] == [0, 1]
assert set(model["placement"]["slotPlacements"].keys()) == {"0", "1"}
assert model["placement"]["slotPlacements"]["0"]["candidatePorts"] == {"start": 39180, "end": 39190}
assert model["placement"]["slotPlacements"]["1"]["candidatePorts"] == {"start": 39280, "end": 39290}
PY

cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime
runtime_bin="$repo/runtime/target/debug/nixfied-runtime"
state_base="$tmp/state"

wait_for_process_row_and_stop() {
  local pid="$1"
  local db="$2"
  python3 - "$pid" "$db" <<'PY'
import os
import pathlib
import signal
import sqlite3
import sys
import time

pid = int(sys.argv[1])
db = pathlib.Path(sys.argv[2])
deadline = time.monotonic() + 10
while time.monotonic() < deadline:
    if db.exists():
        try:
            with sqlite3.connect(db) as conn:
                count = conn.execute(
                    "SELECT count(*) FROM processes WHERE status IN ('starting','running','ready')"
                ).fetchone()[0]
            if count:
                os.kill(pid, signal.SIGSTOP)
                raise SystemExit(0)
        except sqlite3.Error:
            pass
    time.sleep(0.005)
raise SystemExit(f"timed out waiting for live process row in {db}")
PY
}

assert_ps_liveness() {
  local report="$1"
  local expected="$2"
  python3 - "$report" "$expected" <<'PY'
import json
import pathlib
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text())
expected = sys.argv[2] == "true"
observed = any(
    process["live"] and process["serviceInstanceId"] is not None
    for process in report["processes"]
)
assert observed is expected, report
PY
}

assert_down_stopped() {
  local report="$1"
  python3 - "$report" <<'PY'
import json
import pathlib
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert report["stopped"], report
PY
}

check0="$tmp/check-slot-0.json"
check1="$tmp/check-slot-1.json"
"$runtime_bin" check --model "$model_out/model.json" --slot 0 >"$check0"
"$runtime_bin" check --model "$model_out/model.json" --slot 1 >"$check1"
if "$runtime_bin" check --model "$model_out/model.json" --slot 2 >"$tmp/check-slot-2.json" 2>"$tmp/check-slot-2.err"; then
  echo "slot 2 should have been refused" >&2
  exit 1
fi

live_state_base="$tmp/live-state"
live_run0="$tmp/live-run-slot-0.json"
live_run1="$tmp/live-run-slot-1.json"
(
  cd "$project"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" run --model "$model_out/model.json" --slot 0 >"$live_run0" 2>"$tmp/live-run-slot-0.err"
) &
live_pid0=$!
live_pids+=("$live_pid0")
(
  cd "$project"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" run --model "$model_out/model.json" --slot 1 >"$live_run1" 2>"$tmp/live-run-slot-1.err"
) &
live_pid1=$!
live_pids+=("$live_pid1")
wait_for_process_row_and_stop "$live_pid0" "$live_state_base/m1-slot-isolation/dev/0/registry/registry.sqlite3"
wait_for_process_row_and_stop "$live_pid1" "$live_state_base/m1-slot-isolation/dev/1/registry/registry.sqlite3"
(
  cd "$project"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 0 >"$tmp/live-ps-slot-0.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 1 >"$tmp/live-ps-slot-1.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" down --model "$model_out/model.json" --slot 0 --timeout-ms 1000 >"$tmp/live-down-slot-0.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 0 >"$tmp/live-ps-slot-0-after-down.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 1 >"$tmp/live-ps-slot-1-after-slot-0-down.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" down --model "$model_out/model.json" --slot 1 --timeout-ms 1000 >"$tmp/live-down-slot-1.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 1 >"$tmp/live-ps-slot-1-after-down.json"
)
assert_ps_liveness "$tmp/live-ps-slot-0.json" true
assert_ps_liveness "$tmp/live-ps-slot-1.json" true
assert_down_stopped "$tmp/live-down-slot-0.json"
assert_ps_liveness "$tmp/live-ps-slot-0-after-down.json" false
assert_ps_liveness "$tmp/live-ps-slot-1-after-slot-0-down.json" true
assert_down_stopped "$tmp/live-down-slot-1.json"
assert_ps_liveness "$tmp/live-ps-slot-1-after-down.json" false
for pid in "$live_pid0" "$live_pid1"; do
  kill -TERM "$pid" 2>/dev/null || true
  kill -CONT "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
done
live_pids=()
(
  cd "$project"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" clean --model "$model_out/model.json" --slot 0 >"$tmp/live-clean-slot-0.json"
  NIXFIED_STATE_DIR="$live_state_base" "$runtime_bin" clean --model "$model_out/model.json" --slot 1 >"$tmp/live-clean-slot-1.json"
)

run0="$tmp/run-slot-0.json"
run1="$tmp/run-slot-1.json"
(
  cd "$project"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" run --model "$model_out/model.json" --slot 0 >"$run0"
) &
pid0=$!
(
  cd "$project"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" run --model "$model_out/model.json" --slot 1 >"$run1"
) &
pid1=$!
status=0
wait "$pid0" || status=$?
wait "$pid1" || status=$?
if [[ "$status" -ne 0 ]]; then
  exit "$status"
fi

(
  cd "$project"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 0 >"$tmp/ps-slot-0.json"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" --slot 1 >"$tmp/ps-slot-1.json"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" down --model "$model_out/model.json" --slot 0 >"$tmp/down-slot-0.json"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" down --model "$model_out/model.json" --slot 1 >"$tmp/down-slot-1.json"
)

python3 - "$model_out/model.json" "$check0" "$check1" "$run0" "$run1" "$state_base" <<'PY'
import json
import pathlib
import sqlite3
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
check0 = json.loads(pathlib.Path(sys.argv[2]).read_text())
check1 = json.loads(pathlib.Path(sys.argv[3]).read_text())
run0 = json.loads(pathlib.Path(sys.argv[4]).read_text())
run1 = json.loads(pathlib.Path(sys.argv[5]).read_text())
state_base = pathlib.Path(sys.argv[6])
project_id = model["project"]["projectId"]

assert check0["slot"] == 0
assert check1["slot"] == 1
assert check0["environment"] == "dev"
assert check1["environment"] == "dev"
assert check0["computedModelHash"] == run0["computedModelHash"] == run1["computedModelHash"]
assert run0["task"]["success"] is True
assert run1["task"]["success"] is True

roots = [state_base / project_id / "dev" / str(slot) for slot in (0, 1)]
for slot, root in enumerate(roots):
    marker = json.loads((root / ".nixfied-state.json").read_text())
    assert marker["environment"] == "dev"
    assert marker["slot"] == slot
    assert (root / "registry" / "registry.sqlite3").is_file()

summary0 = pathlib.Path(run0["summaryPath"])
summary1 = pathlib.Path(run1["summaryPath"])
run_dir0 = summary0.parent
run_dir1 = summary1.parent
logs0 = run_dir0 / "logs"
logs1 = run_dir1 / "logs"
artifacts0 = run_dir0 / "artifacts"
artifacts1 = run_dir1 / "artifacts"

distinct_pairs = [
    (roots[0], roots[1]),
    (roots[0] / "registry" / "registry.sqlite3", roots[1] / "registry" / "registry.sqlite3"),
    (run_dir0, run_dir1),
    (logs0, logs1),
    (artifacts0, artifacts1),
    (summary0, summary1),
]
for left, right in distinct_pairs:
    assert left != right, (left, right)
    assert left.exists(), left
    assert right.exists(), right

svc0 = run0["services"][0]
svc1 = run1["services"][0]
assert svc0["selectedEndpoint"]["port"] == 39180
assert svc1["selectedEndpoint"]["port"] == 39280
assert svc0["selectedEndpoint"]["port"] != svc1["selectedEndpoint"]["port"]
assert svc0["serviceInstanceId"] != svc1["serviceInstanceId"]

for slot, root in enumerate(roots):
    db = root / "registry" / "registry.sqlite3"
    with sqlite3.connect(db) as conn:
        meta = conn.execute(
            "SELECT environment, slot FROM registry_meta WHERE id = 1"
        ).fetchone()
        assert meta == ("dev", slot)
        for table in ["runs", "services", "processes", "ports", "events"]:
            count, min_env, max_env, min_slot, max_slot = conn.execute(
                f"SELECT count(*), min(environment), max(environment), min(slot), max(slot) FROM {table}"
            ).fetchone()
            assert count > 0, table
            assert (min_env, max_env, min_slot, max_slot) == ("dev", "dev", slot, slot)
PY

(
  cd "$project"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" --slot 0 >"$tmp/clean-slot-0.json"
)
test ! -e "$state_base/m1-slot-isolation/dev/0"
test -e "$state_base/m1-slot-isolation/dev/1"

(
  cd "$project"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" --slot 1 >"$tmp/clean-slot-1.json"
)
test ! -e "$state_base/m1-slot-isolation/dev/1"

cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test m0_service two_slots_keep_services_state_and_controls_isolated
