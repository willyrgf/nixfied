{
  emitEvent,
  processInspect,
  processStop,
  registryRoot,
}:
''
  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  cleanup() {
    for pid in "$RUN_TARGET_PID" "$SVC_TARGET_PID" "$RUN_SLOT_PID" "$SVC_SLOT_PID" "$RUN_OTHER_PID" "$SVC_OTHER_PID"; do
      if [ -n "''${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
      fi
    done
  }
  trap cleanup EXIT

  rm -rf "${registryRoot}"
  mkdir -p "${registryRoot}"

  RUN_TARGET_ID="fixture-stop-target"
  RUN_SLOT_ID="fixture-stop-same-slot"
  RUN_OTHER_ID="fixture-stop-other-env"

  sleep 120 &
  RUN_TARGET_PID=$!
  sleep 120 &
  SVC_TARGET_PID=$!
  sleep 120 &
  RUN_SLOT_PID=$!
  sleep 120 &
  SVC_SLOT_PID=$!
  sleep 120 &
  RUN_OTHER_PID=$!
  sleep 120 &
  SVC_OTHER_PID=$!

  ${emitEvent} \
    --event-type run_started \
    --state running \
    --run-id "$RUN_TARGET_ID" \
    --command "ci" \
    --slot "0" \
    --env "dev" \
    --pid "$RUN_TARGET_PID" >/dev/null
  ${emitEvent} \
    --event-type service_ready \
    --state ready \
    --service "svc-target" \
    --run-id "$RUN_TARGET_ID" \
    --command "up" \
    --slot "0" \
    --env "dev" \
    --pid "$SVC_TARGET_PID" \
    --log-path "/tmp/svc-target.log" >/dev/null

  ${emitEvent} \
    --event-type run_started \
    --state running \
    --run-id "$RUN_SLOT_ID" \
    --command "ci" \
    --slot "0" \
    --env "dev" \
    --pid "$RUN_SLOT_PID" >/dev/null
  ${emitEvent} \
    --event-type service_ready \
    --state ready \
    --service "svc-slot" \
    --run-id "$RUN_SLOT_ID" \
    --command "up" \
    --slot "0" \
    --env "dev" \
    --pid "$SVC_SLOT_PID" \
    --log-path "/tmp/svc-slot.log" >/dev/null

  ${emitEvent} \
    --event-type run_started \
    --state running \
    --run-id "$RUN_OTHER_ID" \
    --command "ci" \
    --slot "0" \
    --env "test" \
    --pid "$RUN_OTHER_PID" >/dev/null
  ${emitEvent} \
    --event-type service_ready \
    --state ready \
    --service "svc-other" \
    --run-id "$RUN_OTHER_ID" \
    --command "up" \
    --slot "0" \
    --env "test" \
    --pid "$SVC_OTHER_PID" \
    --log-path "/tmp/svc-other.log" >/dev/null

  ${processStop} --run-id "$RUN_TARGET_ID" --dry-run >/dev/null
  kill -0 "$RUN_TARGET_PID" 2>/dev/null || fail "dry-run should not stop target run pid"
  kill -0 "$SVC_TARGET_PID" 2>/dev/null || fail "dry-run should not stop target service pid"

  ${processStop} --run-id "$RUN_TARGET_ID" >/dev/null
  if kill -0 "$RUN_TARGET_PID" 2>/dev/null; then
    fail "run scope stop should stop target run pid"
  fi
  if kill -0 "$SVC_TARGET_PID" 2>/dev/null; then
    fail "run scope stop should stop target service pid"
  fi
  kill -0 "$RUN_SLOT_PID" 2>/dev/null || fail "run scope stop should not stop same-slot run"
  kill -0 "$SVC_SLOT_PID" 2>/dev/null || fail "run scope stop should not stop same-slot service"

  TARGET_INSPECT="$(${processInspect} --id "$RUN_TARGET_ID")"
  printf '%s\n' "$TARGET_INSPECT" | grep -q '"event_type": "run_finished"' || fail "target run should record run_finished after stop"
  printf '%s\n' "$TARGET_INSPECT" | grep -q '"state": "stopped"' || fail "target run should be marked stopped"
  printf '%s\n' "$TARGET_INSPECT" | grep -q '"event_type": "service_stopped"' || fail "target service should record service_stopped"

  ${processStop} --run-id "$RUN_TARGET_ID" --scope slot-env >/dev/null
  if kill -0 "$RUN_SLOT_PID" 2>/dev/null; then
    fail "slot-env scope should stop same-slot run"
  fi
  if kill -0 "$SVC_SLOT_PID" 2>/dev/null; then
    fail "slot-env scope should stop same-slot service"
  fi

  kill -0 "$RUN_OTHER_PID" 2>/dev/null || fail "slot-env scope should not stop other env run"
  kill -0 "$SVC_OTHER_PID" 2>/dev/null || fail "slot-env scope should not stop other env service"

  echo "registry process stop fixture ok"
''
