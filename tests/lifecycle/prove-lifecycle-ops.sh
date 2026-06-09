#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo"

run_exact_test() {
  local test_target="$1"
  local test_name="$2"
  local list_output
  list_output="$(cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test "$test_target" -- --list)"
  if ! grep -Fxq "$test_name: test" <<<"$list_output"; then
    echo "expected test $test_target::$test_name to exist" >&2
    exit 1
  fi
  cargo test --manifest-path "$repo/runtime/Cargo.toml" -p nixfied-runtime --test "$test_target" "$test_name" -- --exact
}

run_exact_test service lifecycle_events_follow_declared_class_order_and_clean_terminal
run_exact_test service health_failure_after_ready_records_distinct_lifecycle_failure
