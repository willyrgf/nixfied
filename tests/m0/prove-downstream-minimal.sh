#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-m0-proof.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
cd "$repo"

model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo/examples/m0-minimal#model")"
case "$model_out" in
  /nix/store/*) ;;
  *)
    echo "expected model output under /nix/store, got: $model_out" >&2
    exit 1
    ;;
esac

test -f "$model_out/model.json"
test ! -e "$model_out/manifest.json"
test -f "$model_out/views/schema.json"
test -f "$model_out/views/capabilities.json"
test -f "$model_out/views/docs.md"

python3 - "$model_out" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
model = json.loads((root / "model.json").read_text())
schema = json.loads((root / "views" / "schema.json").read_text())
capabilities = json.loads((root / "views" / "capabilities.json").read_text())

assert schema["source"] == "model.json"
assert schema["modelTypes"]["modelVersion"] == model["modelVersion"]
assert schema["modelTypes"]["runtimeAbi"] == model["runtimeAbi"]
assert schema["modelTypes"]["toolchainId"] == model["toolchainId"]
assert capabilities == model["capabilities"]
assert model["workflows"] == {}
assert model["secrets"] == []
PY

cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime
runtime_bin="$repo/runtime/target/debug/nixfied-runtime"

check_json="$tmp/check.json"
"$runtime_bin" check --model "$model_out/model.json" >"$check_json"

model_only="$tmp/model-only.json"
cp "$model_out/model.json" "$model_only"
"$runtime_bin" check --allow-non-store-model --model "$model_only" >/dev/null

state_base="$tmp/state"
run_json="$tmp/run.json"
NIXFIED_STATE_DIR="$state_base" "$runtime_bin" run --model "$model_out/model.json" >"$run_json"
NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" >/dev/null
NIXFIED_STATE_DIR="$state_base" "$runtime_bin" down --model "$model_out/model.json" >/dev/null
python3 - "$model_out/model.json" "$check_json" "$run_json" "$state_base" <<'PY'
import json
import pathlib
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
check = json.loads(pathlib.Path(sys.argv[2]).read_text())
run = json.loads(pathlib.Path(sys.argv[3]).read_text())
state_base = pathlib.Path(sys.argv[4])
state_root = state_base / model["project"]["projectId"] / "dev" / "0"
assert check["computedModelHash"] == run["computedModelHash"]
assert run["task"]["success"] is True
assert pathlib.Path(run["summaryPath"]).is_file()
assert (state_root / ".nixfied-state.json").is_file()
PY
NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" >/dev/null
test ! -e "$state_base/m0-minimal/dev/0"

cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test m0_service
cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test m0_state
