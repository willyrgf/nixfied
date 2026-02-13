{
  rethBin,
  heliosBin,
  rethInit,
  rethStart,
  rethStop,
  rethHealth,
  heliosInit,
  heliosStart,
  heliosStop,
  heliosHealth,
  heliosReady,
  heliosStatus,
  heliosCheckConfig,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  eval "$("$SLOT_INFO")"

  if nc -z 127.0.0.1 "$RETHHTTP_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$RETHWS_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$RETHAUTH_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$HELIOSRPC_PORT" >/dev/null 2>&1; then
    echo "SKIP: helios lifecycle ports already in use reth_http=$RETHHTTP_PORT reth_ws=$RETHWS_PORT reth_auth=$RETHAUTH_PORT helios_rpc=$HELIOSRPC_PORT"
    exit 0
  fi

  export HELIOS_CONSENSUS_RPC_URL="http://127.0.0.1:$RETHHTTP_PORT"
  # Fast-fail readiness checks before start.
  export HELIOS_READY_TIMEOUT_SECS="0"
  export HELIOS_READY_INTERVAL_SECS="1"

  RETH_VERSION_OUT=$(${rethBin} --version 2>&1 || true)
  echo "$RETH_VERSION_OUT" | grep -qi "mock" && fail "reth binary is mocked: $RETH_VERSION_OUT"

  HELIOS_VERSION_OUT=$(${heliosBin} --version 2>&1 || true)
  echo "$HELIOS_VERSION_OUT" | grep -qi "mock" && fail "helios binary is mocked: $HELIOS_VERSION_OUT"

  ${rethInit}
  ${rethInit}
  set +e
  ${rethHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "rethHealth should fail before start"

  RETH_PID=$(start_service reth -- ${rethStart})
  RETH_READY=0
  for _ in $(seq 1 120); do
    if ${rethHealth} >/dev/null 2>&1; then
      RETH_READY=1
      break
    fi
    sleep 0.25
  done
  [ "$RETH_READY" -eq 1 ] || fail "reth did not become healthy"
  ${rethHealth} >/dev/null || fail "rethHealth should pass while running"

  ${heliosInit}
  ${heliosInit}

  HELIOS_CHECK_OUTPUT="$(${heliosCheckConfig} 2>&1)" || {
    echo "$HELIOS_CHECK_OUTPUT" >&2
    fail "heliosCheckConfig failed"
  }
  HELIOS_NETWORK_VALUE="$(
    printf '%s\n' "$HELIOS_CHECK_OUTPUT" \
      | sed -n 's/.* network=\([^[:space:]]*\).*/\1/p' \
      | tail -1
  )"

  set +e
  ${heliosHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "heliosHealth should fail before start"

  set +e
  ${heliosReady} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "heliosReady should fail before start"

  if [ "$HELIOS_NETWORK_VALUE" = "local" ]; then
    echo "SKIP: helios lifecycle start checks skipped network=local requires beacon consensus endpoint"
  else
    HELIOS_PID=$(start_service helios -- ${heliosStart})
    export HELIOS_READY_TIMEOUT_SECS="120"
    if ! ${heliosReady} >/dev/null 2>&1; then
      HELIOS_DIR="$BASE_DIR/helios-$SLOT-$ENV"
      if [ -f "$HELIOS_DIR/logs/helios.log" ]; then
        tail -50 "$HELIOS_DIR/logs/helios.log" >&2 || true
      else
        echo "helios log missing path=$HELIOS_DIR/logs/helios.log" >&2
      fi
      ${heliosStop} >/dev/null 2>&1 || true
      if kill -0 "$HELIOS_PID" 2>/dev/null; then
        stop_service "$HELIOS_PID" "helios"
      fi
      fail "helios did not become ready"
    fi
    ${heliosHealth} >/dev/null || fail "heliosHealth should pass while running"

    ${heliosStatus} >/dev/null || fail "heliosStatus should pass while running"

    ${heliosStop}
    if kill -0 "$HELIOS_PID" 2>/dev/null; then
      stop_service "$HELIOS_PID" "helios"
    fi

    HELIOS_DOWN=0
    for _ in $(seq 1 40); do
      if ${heliosHealth} >/dev/null 2>&1; then
        sleep 0.25
      else
        HELIOS_DOWN=1
        break
      fi
    done
    [ "$HELIOS_DOWN" -eq 1 ] || fail "heliosHealth should fail after stop"

    export HELIOS_READY_TIMEOUT_SECS="0"
    set +e
    ${heliosReady} >/dev/null 2>&1
    RC=$?
    set -e
    [ "$RC" -ne 0 ] || fail "heliosReady should fail after stop"
  fi

  ${rethStop}
  if kill -0 "$RETH_PID" 2>/dev/null; then
    stop_service "$RETH_PID" "reth"
  fi

  RETH_DOWN=0
  for _ in $(seq 1 40); do
    if ${rethHealth} >/dev/null 2>&1; then
      sleep 0.25
    else
      RETH_DOWN=1
      break
    fi
  done
  [ "$RETH_DOWN" -eq 1 ] || fail "rethHealth should fail after stop"

  echo "helios lifecycle fixture ok"

''
