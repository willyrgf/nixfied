{
  portCleanup,
  portConflictChecker,
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

  PORT=$(pick_port) || fail "failed to pick port"
  nc -l 127.0.0.1 "$PORT" >/dev/null 2>&1 &
  NC_PID=$!
  sleep 0.3

  set +e
  ${portConflictChecker} "$PORT" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "conflict checker should fail while port is in use"

  ${portCleanup} "$PORT" >/dev/null
  sleep 0.3

  ${portConflictChecker} "$PORT" >/dev/null || fail "conflict checker should pass after cleanup"

  kill -TERM "$NC_PID" 2>/dev/null || true
  wait "$NC_PID" 2>/dev/null || true

  echo "lib port-utils fixture ok"

''
