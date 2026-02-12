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
  heliosStatus,
  heliosCheckConfig,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  pick_port() {
    local port
    local i
    for i in $(seq 1 40); do
      port=$(( (RANDOM % 20000) + 20000 ))
      if ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
        echo "$port"
        return 0
      fi
    done
    return 1
  }

  eval "$("$SLOT_INFO")"
  export RETHHTTP_PORT=$(pick_port)
  export RETHWS_PORT=$(pick_port)
  export RETHAUTH_PORT=$(pick_port)
  export HELIOSRPC_PORT=$(pick_port)
  export HELIOS_CONSENSUS_RPC_URL="http://127.0.0.1:$RETHHTTP_PORT"

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

  ${heliosInit}
  ${heliosInit}

  ${heliosCheckConfig} >/dev/null || fail "heliosCheckConfig failed"

  set +e
  ${heliosHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "heliosHealth should fail before start"

  HELIOS_PID=$(start_service helios -- ${heliosStart})
  READY=0
  for _ in $(seq 1 120); do
    if ${heliosHealth} >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.2
  done
  [ "$READY" -eq 1 ] || fail "helios did not become healthy"

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
