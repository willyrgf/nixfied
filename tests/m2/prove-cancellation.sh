#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo"

if [[ ! -x /usr/bin/perl ]]; then
  echo "M2 process escape proof requires /usr/bin/perl for POSIX setsid coverage" >&2
  exit 1
fi

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

run_exact_test m0_service cancellation_interrupts_readiness_and_terminates_service_group
run_exact_test m0_service cancellation_interrupts_task_and_terminates_task_group
run_exact_test m0_service cli_signal_during_shutdown_records_canceled_terminal_state
run_exact_test m0_service cli_signal_cancels_run_and_empties_service_group
run_exact_test m0_service task_timeout_records_canceled_summary_and_terminates_task_group
run_exact_test m0_service down_completes_canceling_lease_and_unblocks_cleanup
run_exact_test m0_service down_cancels_live_task_process_group_and_unblocks_cleanup
run_exact_test m0_service down_escalates_until_owned_process_group_is_empty
run_exact_test m0_service daemonizing_service_escape_is_recorded_and_refused
run_exact_test m0_service setsid_descendant_escape_is_recorded_and_refused
run_exact_test m0_service stop_refuses_delayed_setsid_escape
run_exact_test m0_service readiness_refuses_monitored_setsid_escape
run_exact_test m0_service readiness_timeout_prefers_escape_discovered_during_probe
run_exact_test m0_service readiness_records_foreground_exit_as_escape
run_exact_test m0_service ps_reconciles_dead_owned_process_and_port_as_stale
run_exact_test m0_service ps_rejects_live_process_with_mismatched_start_identity_as_stale
run_exact_test m0_service ps_marks_expired_dead_run_lease_as_stale
run_exact_test m0_service active_run_lease_refuses_new_service_start_even_after_terminal_service_row
run_exact_test m0_service expired_live_run_lease_still_refuses_new_service_start

"$repo/tests/m0/prove-runtime-without-nix.sh"
