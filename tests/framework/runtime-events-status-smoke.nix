{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  runtimeEvents = import ../../nixfied/framework/core/runtime-events.nix {
    inherit pkgs;
    project = {
      project.id = "runtime-events-status-smoke";
    };
  };
in
pkgs.runCommand "runtime-events-status-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export REGISTRY_ROOT="$TMPDIR/registry"
  export BASE_DIR="$TMPDIR/base"
  LOG_FILE="$BASE_DIR/postgres/postgres.log"
  mkdir -p "$(dirname "$LOG_FILE")"
  printf '%s\n' "runtime events log line" > "$LOG_FILE"

  "${runtimeEvents.emitEvent}" --event-type slot_acquired --run-id slot-run --slot 0 --env test > /dev/null
  "${runtimeEvents.emitEvent}" --event-type service_ready --service postgres --run-id svc-run --slot 0 --env test --owner-scope persistent --log-path "$LOG_FILE" > /dev/null

  STATUS_OUT="$("${runtimeEvents.serviceStatus}" --service postgres --slot 0 --env test)"
  eval "$STATUS_OUT"
  [ "$REGISTRY_FOUND" = "1" ] || fail "expected registry status to be found"
  [ "$REGISTRY_RUNNING" = "true" ] || fail "expected registry running=true after service_ready"
  [ "$REGISTRY_STATE" = "ready" ] || fail "expected registry_state=ready"
  [ "$OWNER_RUN_ID" = "svc-run" ] || fail "expected owner run id from latest service event"
  [ "$OWNER_SCOPE" = "persistent" ] || fail "expected owner scope from latest service event"
  [ "$LOG_PATH" = "$LOG_FILE" ] || fail "expected log path from latest service event"
  [ "$SLOT_OWNER" = "slot-run" ] || fail "expected slot owner from latest slot event"

  LOG_OUT="$("${runtimeEvents.serviceLogs}" --service postgres --slot 0 --env test --lines 1)"
  [ "$LOG_OUT" = "runtime events log line" ] || fail "service-logs should resolve the latest emitted log path"

  "${runtimeEvents.emitEvent}" --event-type slot_released --run-id slot-run --slot 0 --env test > /dev/null
  STATUS_OUT="$("${runtimeEvents.serviceStatus}" --service postgres --slot 0 --env test)"
  eval "$STATUS_OUT"
  [ -z "$SLOT_OWNER" ] || fail "expected slot owner to clear after slot_released"

  "${runtimeEvents.emitEvent}" --event-type service_stopped --service postgres --run-id svc-run --slot 0 --env test > /dev/null
  STATUS_OUT="$("${runtimeEvents.serviceStatus}" --service postgres --slot 0 --env test)"
  eval "$STATUS_OUT"
  [ "$REGISTRY_RUNNING" = "false" ] || fail "expected registry running=false after service_stopped"
  [ "$REGISTRY_STATE" = "stopped" ] || fail "expected registry_state=stopped after service_stopped"

  if ${pkgs.findutils}/bin/find "$REGISTRY_ROOT/runtime-events" -name 'status.env' | ${pkgs.gnugrep}/bin/grep -q .; then
    fail "runtime-events should no longer persist status.env sidecars"
  fi

  echo "OK: runtime event status and logs are derived from append-only indices without status sidecars" > "$out"
''
