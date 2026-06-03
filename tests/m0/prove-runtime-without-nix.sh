#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-m0-no-nix.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

model_out="$(nix build --no-link --print-out-paths "$repo#m0-minimal-model")"
cargo build --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime
runtime_bin="$repo/runtime/target/debug/nixfied-runtime"

fake_bin="$tmp/fake-bin"
sentinel="$tmp/nix-was-called"
mkdir -p "$fake_bin"
cat >"$fake_bin/nix" <<EOF
#!/usr/bin/env bash
touch "$sentinel"
echo "nix must not be invoked by nixfied-runtime" >&2
exit 127
EOF
chmod +x "$fake_bin/nix"

state_base="$tmp/state"
check_json="$tmp/check.json"
PATH="$fake_bin:$PATH" "$runtime_bin" check --model "$model_out/model.json" >"$check_json"
PATH="$fake_bin:$PATH" NIXFIED_STATE_DIR="$state_base" "$runtime_bin" ps --model "$model_out/model.json" >/dev/null
PATH="$fake_bin:$PATH" NIXFIED_STATE_DIR="$state_base" "$runtime_bin" down --model "$model_out/model.json" >/dev/null
PATH="$fake_bin:$PATH" python3 - "$model_out/model.json" "$check_json" "$state_base" <<'PY'
import json
import pathlib
import sys

model = json.loads(pathlib.Path(sys.argv[1]).read_text())
check = json.loads(pathlib.Path(sys.argv[2]).read_text())
state_base = pathlib.Path(sys.argv[3])
state_root = state_base / model["project"]["projectId"] / "dev" / "0"
state_root.mkdir(parents=True, exist_ok=True)
marker = {
    "markerVersion": 1,
    "markerIdentity": model["state"]["markerIdentity"],
    "projectId": model["project"]["projectId"],
    "environment": "dev",
    "slot": 0,
    "stateKind": "slot",
    "serviceInstanceId": None,
    "stateEpoch": model["state"]["stateEpoch"],
    "cleanupPolicy": model["state"]["cleanupPolicy"],
    "modelPath": check["modelPath"],
    "computedModelHash": check["computedModelHash"],
    "runtimeAbi": model["runtimeAbi"],
    "toolchainId": model["toolchainId"],
    "target": model["target"],
}
(state_root / ".nixfied-state.json").write_text(json.dumps(marker, sort_keys=True))
PY
PATH="$fake_bin:$PATH" NIXFIED_STATE_DIR="$state_base" "$runtime_bin" clean --model "$model_out/model.json" >/dev/null
test ! -e "$state_base/m0-minimal/dev/0"

if [[ -e "$sentinel" ]]; then
  echo "fake nix sentinel was touched; runtime invoked nix" >&2
  exit 1
fi
