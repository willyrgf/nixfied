#!/usr/bin/env bash
set -euo pipefail

# proof: upgrading the Nixfied input pin is non-destructive.
#
# The upgrade surface owns only the flake input/import wiring. It must repin the
# nixfied input and leave the project-owned nixfied.nix byte-for-byte untouched,
# after which the project still compiles a Nix-store model.

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_base="${NIXFIED_TEST_TMPDIR:-${TMPDIR:-/tmp}}"
if [[ -d /private/tmp ]]; then
  tmp_base="/private/tmp"
fi
tmp="$(mktemp -d "$tmp_base/nixfied-upgrade.XXXXXX")"
tmp="$(cd "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT

project="$tmp/upgrade-proof"

# Install with a deliberately unbuildable placeholder pin so the later upgrade to
# the local checkout is observably what makes the project compile.
nix run "$repo#install" -- \
  --root "$project" \
  --project-id upgrade-proof \
  --name "Upgrade Proof" \
  --nixfied-url "github:willyrgf/nixfied"

test -f "$project/flake.nix"
test -f "$project/nixfied.nix"
grep -F 'nixfied.url = "github:willyrgf/nixfied";' "$project/flake.nix" >/dev/null

# Make a project-owned edit to nixfied.nix; the upgrade must never touch it.
printf '\n# project-owned-sentinel: do not touch on upgrade\n' >>"$project/nixfied.nix"
before_hash="$(shasum -a 256 "$project/nixfied.nix" | awk '{print $1}')"

# Repin to the local checkout. --no-lock keeps the run hermetic; the lock is
# created from the path: input at build time below.
nix run "$repo#upgrade" -- \
  --root "$project" \
  --nixfied-url "path:$repo" \
  --no-lock

# Input pin rewritten; project semantics preserved.
grep -F "nixfied.url = \"path:$repo\";" "$project/flake.nix" >/dev/null
if grep -F 'github:willyrgf/nixfied' "$project/flake.nix" >/dev/null; then
  echo "upgrade left the stale github pin in flake.nix" >&2
  exit 1
fi

after_hash="$(shasum -a 256 "$project/nixfied.nix" | awk '{print $1}')"
if [[ "$before_hash" != "$after_hash" ]]; then
  echo "upgrade modified project-owned nixfied.nix" >&2
  exit 1
fi
grep -F 'nixfied.project.projectId = "upgrade-proof";' "$project/nixfied.nix" >/dev/null
grep -F 'project-owned-sentinel' "$project/nixfied.nix" >/dev/null

# The repinned project compiles a Nix-store model end to end.
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

# Negative: upgrading a directory with no flake.nix refuses and changes nothing.
empty="$tmp/empty"
mkdir -p "$empty"
refusal="$tmp/refusal.txt"
if nix run "$repo#upgrade" -- --root "$empty" --nixfied-url "path:$repo" --no-lock >"$refusal" 2>&1; then
  echo "upgrade should refuse a directory with no flake.nix" >&2
  exit 1
fi
grep -F "No files were changed." "$refusal" >/dev/null
test ! -e "$empty/nixfied.nix"

echo "install upgrade proof passed"
