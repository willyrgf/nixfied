{
  supervisorStartDaemon,
  supervisorStop,
  supervisorHealth,
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
  ${supervisorHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "health should be non-zero when supervisor is stopped"

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

  ${supervisorStartDaemon} >/dev/null || fail "supervisorStartDaemon failed"

  READY=0
  for _ in $(seq 1 80); do
    if ${supervisorHealth} >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.25
  done
  [ "$READY" -eq 1 ] || fail "supervisor did not become healthy"

  ${supervisorIsRunning} >/dev/null || fail "isRunning should pass while supervisor is running"
  ${supervisorRestart} "app" >/dev/null || fail "supervisorRestart should pass with service arg"
  ${supervisorHealth} >/dev/null || fail "health should pass after restart"

  ${supervisorStop} >/dev/null || fail "supervisorStop failed"

  for _ in $(seq 1 40); do
    set +e
    ${supervisorIsRunning} >/dev/null 2>&1
    RC=$?
    set -e
    if [ "$RC" -ne 0 ]; then
      break
    fi
    sleep 0.25
  done

  set +e
  ${supervisorIsRunning} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "isRunning should be non-zero after stop"

  set +e
  ${supervisorHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "health should be non-zero after stop"

  echo "supervisor management fixture ok"

''
