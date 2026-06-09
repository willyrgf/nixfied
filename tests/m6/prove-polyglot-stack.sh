#!/usr/bin/env bash
set -euo pipefail

# M6 proof: a polyglot stack (a Python service and a Perl service, each with a
# dependent task) compiles to one store model and runs end to end through the
# public runtime surfaces using only already-shipped generic primitives.

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-m6-polyglot.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
cd "$repo"

model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo#polyglot-stack-model")"
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
assert list(model["services"]) == ["api", "worker"], model["services"]
assert list(model["tasks"]) == ["ping-api", "ping-worker"], model["tasks"]
assert model["environments"]["dev"]["services"] == ["api", "worker"]
# Two distinct language closures.
assert sorted(c["closureId"] for c in model["closures"]) == ["api", "worker"]
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
    --model "$model_out/model.json" --timeout-ms 15000 >"$run_json"
)

python3 - "$run_json" <<'PY'
import json
import pathlib
import sys

run = json.loads(pathlib.Path(sys.argv[1]).read_text())
services = {s["serviceId"]: s["selectedEndpoint"]["port"] for s in run["services"]}
assert set(services) == {"api", "worker"}, services
# Two services share the slot window on distinct ports.
assert services["api"] != services["worker"], services
results = {t["taskId"]: t["success"] for t in run["tasks"]}
assert results == {"ping-api": True, "ping-worker": True}, results
PY

# Each task talked to its own language's service.
api_log="$(find "$state_base" -name 'task.ping-api.stdout.log' | head -1)"
worker_log="$(find "$state_base" -name 'task.ping-worker.stdout.log' | head -1)"
grep -q "python-ok" "$api_log"
grep -q "perl-ok" "$worker_log"

(
  cd "$work"
  NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" >/dev/null
)
test ! -e "$state_base/polyglot-stack/dev/0"

echo "M6 polyglot stack proof passed"
