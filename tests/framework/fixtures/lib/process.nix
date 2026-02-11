{
  processManager,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  PM_LOG="$PWD/process-manager.log"
  ${processManager} > "$PM_LOG" 2>&1 &
  PM_PID=$!
  sleep 1

  kill -TERM "$PM_PID" 2>/dev/null || true
  wait "$PM_PID" 2>/dev/null || true

  if kill -0 "$PM_PID" 2>/dev/null; then
    fail "process manager still running after TERM"
  fi

  grep -q "INFO: Starting" "$PM_LOG" || fail "process manager start log missing"

  echo "lib process fixture ok"

''
