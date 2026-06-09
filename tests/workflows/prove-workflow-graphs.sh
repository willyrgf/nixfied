#!/usr/bin/env bash
set -euo pipefail

# proof: a workflow is a bounded acyclic graph over generic tasks with a
# service-requirement readiness gate. The generic runtime starts the required
# service, runs the nodes in dependency order, records per-node results and a
# workflow summary, then stops and marker-gated cleans.

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-workflow.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
cd "$repo"

model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo#workflow-model")"
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
assert model["capabilities"]["workflows"] == ["pipeline"], model["capabilities"]["workflows"]
wf = model["workflows"]["pipeline"]
assert wf["servicesRequired"] == ["synthetic"], wf
ids = [n["nodeId"] for n in wf["nodes"]]
assert ids == ["probe", "verify"], ids
verify = next(n for n in wf["nodes"] if n["nodeId"] == "verify")
assert verify["dependsOn"] == ["probe"], verify
PY

cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime >/dev/null 2>&1
runtime_bin="$repo/runtime/target/debug/nixfied-runtime"

state_base="$tmp/state"
work="$tmp/work"
mkdir -p "$work"
run_json="$tmp/run.json"

(
  cd "$work"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" run \
    --model "$model_out/model.json" --workflow pipeline --timeout-ms 15000 >"$run_json"
)

python3 - "$run_json" <<'PY'
import json
import pathlib
import sys

run = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert run["workflowId"] == "pipeline", run.get("workflowId")
nodes = run["workflowNodes"]
# Nodes ran in dependency order and all succeeded.
assert [n["nodeId"] for n in nodes] == ["probe", "verify"], nodes
assert all(n["success"] for n in nodes), nodes
# The synthetic service requirement was started.
assert run["services"][0]["serviceId"] == "synthetic"
# A workflow summary was written and records success.
summary = json.loads(pathlib.Path(run["workflowSummaryPath"]).read_text())
assert summary["workflowId"] == "pipeline"
assert summary["success"] is True
assert [n["nodeId"] for n in summary["nodes"]] == ["probe", "verify"]
PY

# Marker-gated clean removes the workflow slot state.
(
  cd "$work"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" >/dev/null
)
test ! -e "$state_base/workflow-example/dev/0"

echo "workflow graphs proof passed"
