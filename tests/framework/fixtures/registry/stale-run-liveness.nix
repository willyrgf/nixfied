{
  emitEvent,
  processStatus,
  processRuns,
  registryRoot,
}:
''
  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  rm -rf "${registryRoot}"
  mkdir -p "${registryRoot}"

  sleep 30 &
  STALE_PID=$!
  kill "$STALE_PID" 2>/dev/null || true
  wait "$STALE_PID" 2>/dev/null || true

  RUN_ID="fixture-stale-run"

  ${emitEvent} \
    --event-type run_started \
    --state running \
    --run-id "$RUN_ID" \
    --command "ci" \
    --slot "0" \
    --env "test" \
    --pid "$STALE_PID" >/dev/null

  RUNS_OUT="$(${processRuns})"
  if printf '%s\n' "$RUNS_OUT" | grep -q "run_id=$RUN_ID"; then
    echo "$RUNS_OUT" >&2
    fail "process-runs should not report stale run as active"
  fi
  if ! printf '%s\n' "$RUNS_OUT" | grep -q "OK: no active runs found"; then
    echo "$RUNS_OUT" >&2
    fail "process-runs should report no active runs"
  fi

  RUNS_ALL_OUT="$(${processRuns} --all)"
  if ! printf '%s\n' "$RUNS_ALL_OUT" | grep -q "run_id=$RUN_ID"; then
    echo "$RUNS_ALL_OUT" >&2
    fail "process-runs --all missing stale run"
  fi
  if ! printf '%s\n' "$RUNS_ALL_OUT" | grep -q "run_id=$RUN_ID state=failed"; then
    echo "$RUNS_ALL_OUT" >&2
    fail "process-runs --all should map stale run to failed"
  fi
  if printf '%s\n' "$RUNS_ALL_OUT" | grep -q "run_id=$RUN_ID state=running"; then
    echo "$RUNS_ALL_OUT" >&2
    fail "process-runs --all should not keep stale run as running"
  fi

  STATUS_OUT="$(${processStatus})"
  if printf '%s\n' "$STATUS_OUT" | grep -q "run_id=$RUN_ID"; then
    echo "$STATUS_OUT" >&2
    fail "process-status should not report stale run as active"
  fi

  STATUS_ALL_OUT="$(${processStatus} --all)"
  if ! printf '%s\n' "$STATUS_ALL_OUT" | grep -q "type=run run_id=$RUN_ID state=failed"; then
    echo "$STATUS_ALL_OUT" >&2
    fail "process-status --all should map stale run to failed"
  fi

  echo "registry stale run liveness fixture ok"
''
