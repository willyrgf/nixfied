#!/usr/bin/env bash
set -euo pipefail

# C2 guard: milestone vocabulary (m0, m2c, pre-m1, ...) must not reappear in
# product surfaces. Development history is exempt: the RFC plans, this repo's
# git history, and the interim milestone proof scripts under tests/mN keep their
# milestone names by design. This guard scopes to the live product:
# the Nix module/compiler/adapter surface, the runtime crate sources, the
# example projects, and the flake.

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"

# Lowercase milestone tokens: a bare mN word, an :mN: contract segment, or pre-m1.
pattern='(\bm[0-9]+\b|:m[0-9]+:|pre-m[0-9]+)'

roots=(
  nix
  examples
  flake.nix
  runtime/crates/nixfied-model/src
  runtime/crates/nixfied-runtime/src
  runtime/crates/nixfied-cli/src
  runtime/crates/nixfied-conformance/src
)

hits="$(grep -rnIE "$pattern" "${roots[@]}" 2>/dev/null || true)"
if [[ -n "$hits" ]]; then
  echo "milestone vocabulary leaked into product surfaces:" >&2
  echo "$hits" >&2
  exit 1
fi

echo "guard passed: no milestone vocabulary in product surfaces"
