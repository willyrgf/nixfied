#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-views.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
cd "$repo"

model_out="$(nix build --no-link --no-write-lock-file --print-out-paths "$repo#minimal-model")"
case "$model_out" in
  /nix/store/*) ;;
  *)
    echo "expected model output under /nix/store, got: $model_out" >&2
    exit 1
    ;;
esac

cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-cli -p nixfied-runtime
cli_bin="$repo/runtime/target/debug/nixfied"
runtime_bin="$repo/runtime/target/debug/nixfied-runtime"

model_json="$tmp/model.json"
schema_json="$tmp/schema.json"
capabilities_json="$tmp/capabilities.json"
docs_md="$tmp/docs.md"

"$cli_bin" model --model "$model_out/model.json" >"$model_json"
"$cli_bin" schema --model "$model_out/model.json" >"$schema_json"
"$cli_bin" capabilities --model "$model_out/model.json" >"$capabilities_json"
"$cli_bin" docs --model "$model_out/model.json" >"$docs_md"

python3 - "$model_out" "$model_json" "$schema_json" "$capabilities_json" "$docs_md" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
model = json.loads((root / "model.json").read_text())
cli_model = json.loads(pathlib.Path(sys.argv[2]).read_text())
schema = json.loads(pathlib.Path(sys.argv[3]).read_text())
capabilities = json.loads(pathlib.Path(sys.argv[4]).read_text())
docs = pathlib.Path(sys.argv[5]).read_text()

expected_surfaces = [
    "model",
    "schema",
    "docs",
    "capabilities",
    "check",
    "run",
    "ps",
    "down",
    "clean",
]

assert cli_model == model
assert [surface["name"] for surface in model["surfaces"]] == expected_surfaces
assert model["capabilities"]["surfaces"] == expected_surfaces
assert schema["source"] == "model.json"
assert schema["surfaces"] == model["surfaces"]
assert schema["runtimeInputs"] == model["runtimeConstraints"]
assert capabilities == model["capabilities"]
assert "# " + model["docs"]["title"] in docs
for surface in expected_surfaces:
    assert f"- {surface}" in docs
PY

"$runtime_bin" check --model "$model_out/model.json" >/dev/null

mutated="$tmp/mutated-output"
mkdir -p "$mutated"
cp "$model_out/model.json" "$mutated/model.json"
mkdir -p "$mutated/views"
printf '{"surfaces":["view-only-fake"]}\n' >"$mutated/views/capabilities.json"
rm -f "$mutated/views/schema.json" "$mutated/views/docs.md"

mutated_capabilities="$tmp/mutated-capabilities.json"
"$cli_bin" capabilities --model "$mutated/model.json" >"$mutated_capabilities"
"$runtime_bin" check --allow-non-store-model --model "$mutated/model.json" >/dev/null

python3 - "$mutated/model.json" "$mutated/views/capabilities.json" "$mutated_capabilities" <<'PY'
import json
import pathlib
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
materialized = json.loads(pathlib.Path(sys.argv[2]).read_text())
cli_capabilities = json.loads(pathlib.Path(sys.argv[3]).read_text())

assert materialized["surfaces"] == ["view-only-fake"]
assert cli_capabilities == model["capabilities"]
assert "view-only-fake" not in cli_capabilities["surfaces"]
PY

missing_views="$tmp/missing-views-output"
mkdir -p "$missing_views"
cp "$model_out/model.json" "$missing_views/model.json"
"$runtime_bin" check --allow-non-store-model --model "$missing_views/model.json" >/dev/null
test ! -e "$missing_views/views/schema.json"
test ! -e "$missing_views/views/docs.md"
test ! -e "$missing_views/views/capabilities.json"
