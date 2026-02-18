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
  (
    unset LOG_LEVEL NIXFIED_LOG_LEVEL OUTPUT_MODE NIXFIED_OUTPUT_MODE NIXFIED_LOG_FILE
    ${processManager} > "$PM_LOG" 2>&1
  ) &
  PM_PID=$!

  START_LOG_FOUND=0
  for i in $(seq 1 50); do
    if grep -q "INFO: Starting" "$PM_LOG"; then
      START_LOG_FOUND=1
      break
    fi
    if ! kill -0 "$PM_PID" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  [ "$START_LOG_FOUND" -eq 1 ] || fail "process manager start log missing"

  kill -TERM "$PM_PID" 2>/dev/null || true
  wait "$PM_PID" 2>/dev/null || true

  if kill -0 "$PM_PID" 2>/dev/null; then
    fail "process manager still running after TERM"
  fi

  echo "lib process fixture ok"

''
