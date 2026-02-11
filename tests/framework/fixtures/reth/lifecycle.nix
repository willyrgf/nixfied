{
  rethInit,
  rethStart,
  rethStop,
  rethHealth,
  rethStatus,
  rethCheckConfig,
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

  ${rethInit}
  ${rethInit}

  ${rethCheckConfig} >/dev/null || fail "rethCheckConfig failed"

  RETH_PID=$(start_service reth -- ${rethStart})
  READY=0
  for _ in $(seq 1 80); do
    if ${rethHealth} >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.2
  done
  [ "$READY" -eq 1 ] || fail "reth did not become healthy"

  ${rethStatus} >/dev/null || fail "rethStatus should pass while running"

  ${rethStop}
  if kill -0 "$RETH_PID" 2>/dev/null; then
    stop_service "$RETH_PID" "reth"
  fi

  echo "reth lifecycle fixture ok"

''
