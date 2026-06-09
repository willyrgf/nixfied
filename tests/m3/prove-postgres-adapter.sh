#!/usr/bin/env bash
set -euo pipefail

# M3 proof: a concrete Postgres service is expressible purely as a Nix adapter
# that emits generic primitives, and the generic runtime starts/readies/queries/
# stops it without any Postgres-specific code path. A minimal non-Postgres
# project still compiles without the adapter.

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-m3-postgres.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
cd "$repo"

model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo#postgres-model")"
case "$model_out" in
  /nix/store/*) ;;
  *)
    echo "expected model output under /nix/store, got: $model_out" >&2
    exit 1
    ;;
esac
test -f "$model_out/model.json"
test ! -e "$model_out/manifest.json"

python3 - "$model_out/model.json" <<'PY'
import json
import pathlib
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert list(model["services"]) == ["postgres"], model["services"]
assert list(model["tasks"]) == ["smoke-query"], model["tasks"]
assert model["capabilities"]["services"] == ["postgres"]
assert model["services"]["postgres"]["containment"] == "process-tree"
# initdb / postgres / psql closures are all generic ClosureSpecs.
closure_ids = sorted(c["closureId"] for c in model["closures"])
assert closure_ids == ["pg-ctl", "pg-initdb", "pg-psql", "pg-server"], closure_ids
# initdb is bound to the prepare lifecycle op.
prepare = next(op for op in model["services"]["postgres"]["lifecycle"] if op["class"] == "prepare")
assert prepare["execId"] == "pg-init", prepare
assert model["workflows"] == {}
assert model["secrets"] == []
PY

cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime >/dev/null 2>&1
runtime_bin="$repo/runtime/target/debug/nixfied-runtime"

state_base="$tmp/state"
work="$tmp/work"
mkdir -p "$work"
run_json="$tmp/run.json"

# Run from a workspace dir so the live-workspace codebase root resolves; the
# runtime drives initdb (prepare) -> postgres (start) -> TCP-ownership readiness
# -> health -> psql smoke query -> stop, generically.
(
  cd "$work"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" run \
    --model "$model_out/model.json" --timeout-ms 30000 >"$run_json"
)

python3 - "$model_out/model.json" "$run_json" "$state_base" <<'PY'
import json
import pathlib
import sqlite3
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
run = json.loads(pathlib.Path(sys.argv[2]).read_text())
state_base = pathlib.Path(sys.argv[3])

# The smoke query ran against the real server and succeeded.
assert run["task"]["success"] is True, run["task"]
assert run["services"][0]["serviceId"] == "postgres"
assert pathlib.Path(run["summaryPath"]).is_file()
stdout = pathlib.Path(run["task"]["stdoutPath"]).read_text().strip()
assert stdout == "1", f"expected SELECT 1 to print 1, got {stdout!r}"

# The generic lifecycle recorded ready, healthy and stopped terminals.
state_root = state_base / model["project"]["projectId"] / "dev" / "0"
db = state_root / "registry" / "registry.sqlite3"
con = sqlite3.connect(db)
terminals = set()
for (payload,) in con.execute(
    "SELECT payload_json FROM events WHERE event_type = 'service.lifecycle.terminal'"
):
    terminals.add(json.loads(payload)["terminalResult"])
con.close()
for expected in ("ready", "healthy", "stopped"):
    assert expected in terminals, (expected, terminals)
PY

# Reconcile then marker-gated clean removes the Postgres data dir with the slot.
(
  cd "$work"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" >/dev/null
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" >/dev/null
)
test ! -e "$state_base/postgres-example/dev/0"

# A minimal non-Postgres project still compiles without the adapter.
minimal_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo#m0-minimal-model")"
python3 - "$minimal_out/model.json" <<'PY'
import json
import pathlib
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert "postgres" not in model["services"], "minimal project must not require the adapter"
assert list(model["services"]) == ["synthetic"], model["services"]
PY

echo "M3 postgres adapter proof passed"
