{
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
  export HELIOSRPC_PORT=$(pick_port)
  export RETHHTTP_PORT=$(pick_port)

  ${heliosInit}
  ${heliosInit}

  ${heliosCheckConfig} >/dev/null || fail "heliosCheckConfig failed"

  HELIOS_PID=$(start_service helios -- ${heliosStart})
  READY=0
  for _ in $(seq 1 80); do
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

  echo "helios lifecycle fixture ok"

''
