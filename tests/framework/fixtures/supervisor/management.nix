{
  supervisorRestart,
  supervisorRotateLogs,
  supervisorIsRunning,
  supervisorLogs,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  eval "$("$SLOT_INFO")"
  mkdir -p "$LOG_DIR"

  BIG_LOG="$LOG_DIR/app.log"
  for _ in $(seq 1 200); do
    echo "log line"
  done > "$BIG_LOG"

  ${supervisorRotateLogs} 100 2 >/dev/null
  [ -f "$BIG_LOG.1.gz" ] || fail "rotateLogs did not produce app.log.1.gz"

  set +e
  ${supervisorIsRunning} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "isRunning should be non-zero when supervisor is stopped"

  set +e
  ${supervisorLogs} "missing-service" 5 >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "supervisorLogs should fail for missing service log"

  set +e
  ${supervisorRestart} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "supervisorRestart should fail without service arg"

  echo "supervisor management fixture ok"

''
