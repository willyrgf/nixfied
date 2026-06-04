#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_base="${NIXFIED_TEST_TMPDIR:-${TMPDIR:-/tmp}}"
if [[ -d /private/tmp ]]; then
  tmp_base="/private/tmp"
fi
tmp="$(mktemp -d "$tmp_base/nixfied-m0-install.XXXXXX")"
tmp="$(cd "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT

project="$tmp/install-proof"

nix run "$repo#install" -- \
  --root "$project" \
  --project-id install-proof \
  --name "Install Proof" \
  --nixfied-url "path:$repo"

test -f "$project/flake.nix"
test -f "$project/nixfied.nix"

grep -F 'nixfied.url = "path:' "$project/flake.nix" >/dev/null
grep -F 'model = nixfied.lib.${system}.compileModel ./nixfied.nix;' "$project/flake.nix" >/dev/null
grep -F 'nixfied.project.projectId = "install-proof";' "$project/nixfied.nix" >/dev/null
grep -F 'nixfied.project.name = "Install Proof";' "$project/nixfied.nix" >/dev/null

model_out="$(nix build --no-link --print-out-paths "$project#model")"
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

refusal="$tmp/refusal.txt"
if nix run "$repo#install" -- --root "$project" >"$refusal" 2>&1; then
  echo "second install should refuse existing flake.nix" >&2
  exit 1
fi
grep -F "No files were changed." "$refusal" >/dev/null
