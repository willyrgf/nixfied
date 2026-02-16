{
  rethInit,
  rethStart,
  rethStop,
  rethHealth,
  rethReady,
  rethStatus,
  rethCheckConfig,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  wait_ok() {
    local attempts="$1"
    local interval="$2"
    shift 2
    local i
    for i in $(seq 1 "$attempts"); do
      if "$@" >/dev/null 2>&1; then
        return 0
      fi
      sleep "$interval"
    done
    return 1
  }

  wait_fail() {
    local attempts="$1"
    local interval="$2"
    shift 2
    local i
    for i in $(seq 1 "$attempts"); do
      if "$@" >/dev/null 2>&1; then
        sleep "$interval"
      else
        return 0
      fi
    done
    return 1
  }

  cleanup_reth() {
    ${rethStop} >/dev/null 2>&1 || true
    if [ -n "''${RETH_PID:-}" ] && kill -0 "$RETH_PID" 2>/dev/null; then
      stop_service "$RETH_PID" "reth"
    fi
  }

  eval "$("$SLOT_INFO")"

  if nc -z 127.0.0.1 "$RETHHTTP_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$RETHWS_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$RETHAUTH_PORT" >/dev/null 2>&1; then
    echo "SKIP: reth lifecycle ports already in use http=$RETHHTTP_PORT ws=$RETHWS_PORT auth=$RETHAUTH_PORT"
    exit 0
  fi

  RETH_DIR="$BASE_DIR/reth-$SLOT-$ENV"

  ${rethInit}
  ${rethInit}

  ${rethCheckConfig} >/dev/null || fail "rethCheckConfig failed"

  set +e
  ${rethHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "rethHealth should fail before start"

  set +e
  ${rethReady} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "rethReady should fail before start"

  RETH_PID=""
  RETH_PID=$(start_service reth -- ${rethStart})
  READY=0
  for _ in $(seq 1 240); do
    if ${rethReady} >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.2
  done
  if [ "$READY" -ne 1 ]; then
    print_log_tail "$RETH_DIR/logs/reth.log" 50
    cleanup_reth
    fail "reth did not become healthy"
  fi

  if ! wait_ok 20 0.2 ${rethHealth}; then
    print_log_tail "$RETH_DIR/logs/reth.log" 50
    cleanup_reth
    fail "rethHealth should pass while running"
  fi
  if ! wait_ok 20 0.2 ${rethReady}; then
    print_log_tail "$RETH_DIR/logs/reth.log" 50
    cleanup_reth
    fail "rethReady should pass while running"
  fi

  if ! wait_ok 15 0.2 ${rethStatus}; then
    cleanup_reth
    fail "rethStatus should pass while running"
  fi

  cleanup_reth

  RETH_DOWN=0
  for _ in $(seq 1 40); do
    if ${rethHealth} >/dev/null 2>&1; then
      sleep 0.2
    else
      RETH_DOWN=1
      break
    fi
  done
  [ "$RETH_DOWN" -eq 1 ] || fail "rethHealth should fail after stop"

  if ! wait_fail 20 0.2 ${rethReady}; then
    fail "rethReady should fail after stop"
  fi

  echo "reth lifecycle fixture ok"

''
