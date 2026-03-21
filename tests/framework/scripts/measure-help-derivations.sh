#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: tests/framework/scripts/measure-help-derivations.sh [<flake-root>]

Measure incremental derivations built for the public help surfaces by running
them against a disposable path copy of the target flake.

This is useful for local telemetry, but it is not an authoritative cold-store
budget. For hard derivation budgets, run the same surfaces on a disposable or
otherwise clean Nix store.
EOF
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  usage
  exit 0
fi

flake_root="${1:-.}"
flake_root="$(cd "$flake_root" && pwd -P)"

copy_root=""
cleanup() {
  if [ -n "$copy_root" ] && [ -d "$copy_root" ]; then
    rm -rf "$copy_root"
  fi
}
trap cleanup EXIT

copy_root="$(mktemp -d "${TMPDIR:-/tmp}/nixfied-help-budget.XXXXXX")"
copy_root="$(cd "$copy_root" && pwd -P)"
rsync -a --delete --exclude '.git' "$flake_root/" "$copy_root/"
printf '\n# benchmark-tag %s\n' "$(date -u +%Y%m%dT%H%M%SZ)" >> "$copy_root/flake.nix"

count_built_derivations() {
  local ref="$1"
  shift

  local output rc count
  set +e
  output="$(nix run "$ref" "$@" 2>&1 >/dev/null)"
  rc=$?
  set -e

  if [ "$rc" -ne 0 ]; then
    printf 'ERROR: command failed ref=%s rc=%s\n' "$ref" "$rc" >&2
    printf '%s\n' "$output" >&2
    return "$rc"
  fi

  count="$(
    printf '%s\n' "$output" | awk '
      /^this derivation will be built:/ { sum += 1 }
      /^these [0-9]+ derivations will be built:/ { sum += $2 }
      END { print sum + 0 }
    '
  )"
  printf '%s\n' "$count"
}

measure() {
  local name="$1"
  local ref="$2"
  shift 2
  local count
  count="$(count_built_derivations "$ref" "$@")"
  printf 'OK: %s derivationsBuilt=%s\n' "$name" "$count"
}

printf 'WARN: counts reflect incremental builds against the current store\n'
printf 'WARN: use a disposable or clean store for authoritative hard budgets\n'
printf 'INFO: flakeRoot=%s\n' "$flake_root"
printf 'INFO: disposableCopy=%s\n' "$copy_root"

measure "help" "path:$copy_root#help"
measure "ci -- --help" "path:$copy_root#ci" -- --help
measure "run-task -- task.test.isolation.unit --help" \
  "path:$copy_root#run-task" -- task.test.isolation.unit --help
