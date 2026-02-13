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

  cleanup_reth() {
    ${rethStop} >/dev/null 2>&1 || true
    if [ -n "''${RETH_PID:-}" ] && kill -0 "$RETH_PID" 2>/dev/null; then
      stop_service "$RETH_PID" "reth"
    fi
  }

  cleanup_helios() {
    ${heliosStop} >/dev/null 2>&1 || true
    if [ -n "''${HELIOS_PID:-}" ] && kill -0 "$HELIOS_PID" 2>/dev/null; then
      stop_service "$HELIOS_PID" "helios"
    fi
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

  RETH_PID=""
  RETH_PID=$(start_service reth -- ${rethStart})
  RETH_READY=0
  for _ in $(seq 1 120); do
    if ${rethHealth} >/dev/null 2>&1; then
      RETH_READY=1
      break
    fi
    sleep 0.25
  done
  if [ "$RETH_READY" -ne 1 ]; then
    cleanup_reth
    fail "reth did not become healthy"
  fi
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
    # In this harness, network=local does not provide a usable beacon consensus endpoint.
    # Keep this explicit skip instead of a brittle best-effort readiness assertion.
    echo "SKIP: helios lifecycle start checks skipped network=local requires beacon consensus endpoint"
  else
    HELIOS_PID=""
    HELIOS_PID=$(start_service helios -- ${heliosStart})
    export HELIOS_READY_TIMEOUT_SECS="2"
    HELIOS_READY_OK=0
    for _ in $(seq 1 120); do
      if ${heliosReady} >/dev/null 2>&1; then
        HELIOS_READY_OK=1
        break
      fi
      sleep 0.25
    done
    if [ "$HELIOS_READY_OK" -ne 1 ]; then
      HELIOS_DIR="$BASE_DIR/helios-$SLOT-$ENV"
      print_log_tail "$HELIOS_DIR/logs/helios.log" 50
      cleanup_helios
      fail "helios did not become ready"
    fi
    ${heliosHealth} >/dev/null || fail "heliosHealth should pass while running"

    ${heliosStatus} >/dev/null || fail "heliosStatus should pass while running"

    cleanup_helios

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

  cleanup_reth

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
