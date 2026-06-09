#!/usr/bin/env bash
set -euo pipefail

# Guard: milestone vocabulary must not reappear in the product or its tests.
# Only true development history is exempt -- the RFC plan documents and git
# history, which are not scanned here. This script documents the token shapes it
# forbids, so it excludes itself from the scan.

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"

self="$(basename "${BASH_SOURCE[0]}")"

# Forbidden token shapes: a bare milestone word, an :milestone: contract segment,
# or a pre-milestone tag.
pattern='(\bm[0-9]+\b|:m[0-9]+:|pre-m[0-9]+)'

roots=(
  nix
  examples
  flake.nix
  tests
  runtime/crates/nixfied-model/src
  runtime/crates/nixfied-model/tests
  runtime/crates/nixfied-runtime/src
  runtime/crates/nixfied-runtime/tests
  runtime/crates/nixfied-cli/src
  runtime/crates/nixfied-conformance/src
)

hits="$(grep -rnIE --exclude="$self" "$pattern" "${roots[@]}" 2>/dev/null || true)"
if [[ -n "$hits" ]]; then
  echo "milestone vocabulary leaked into product or test surfaces:" >&2
  echo "$hits" >&2
  exit 1
fi

echo "guard passed: no milestone vocabulary in product or test surfaces"
