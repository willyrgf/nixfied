#!/usr/bin/env bash

set -euo pipefail

proof_fail() {
  printf 'ERROR: %s\n' "$1" >&2
  exit 1
}

proof_require_dir() {
  local path="$1"
  [ -d "$path" ] || proof_fail "missing directory: $path"
}

proof_require_file() {
  local path="$1"
  [ -f "$path" ] || proof_fail "missing file: $path"
}

proof_require_non_empty() {
  local value="$1"
  local label="$2"
  [ -n "$value" ] && [ "$value" != "null" ] || proof_fail "missing value: $label"
}

proof_require_contains() {
  local file="$1"
  local needle="$2"
  grep -Fq -- "$needle" "$file" || proof_fail "expected '$needle' in $file"
}
