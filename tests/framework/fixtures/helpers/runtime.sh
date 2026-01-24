fail() {
  echo "FAIL: $*" >&2
  exit 1
}

TMPDIR=$(mktemp -d)
PIDS=()
cleanup() {
  for pid in "${PIDS[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

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

LOG="$TMPDIR/log_capture.log"
log_capture "$LOG" -- "$BASH" -c 'echo "hello"; echo "err" >&2'
grep -q "hello" "$LOG" || fail "log_capture missing stdout"
grep -q "err" "$LOG" || fail "log_capture missing stderr"

TEE_LOG="$TMPDIR/log_capture_tee.log"
TEE_OUT=$(LOG_TEE=1 log_capture "$TEE_LOG" -- "$BASH" -c 'echo "tee-ok"')
echo "$TEE_OUT" | grep -q "tee-ok" || fail "LOG_TEE did not stream output"
grep -q "tee-ok" "$TEE_LOG" || fail "LOG_TEE did not write log"

SUM_LOG="$TMPDIR/summary.log"
echo "summary line" > "$SUM_LOG"
SUM_OK=$(summary_parse "$SUM_LOG" 5 0)
echo "$SUM_OK" | grep -q "Summary" || fail "summary_parse missing header"
echo "$SUM_OK" | grep -q "Exit code: 0" || fail "summary_parse missing exit code"
SUM_FAIL=$(summary_parse "$SUM_LOG" 5 2)
echo "$SUM_FAIL" | grep -q "Exit code: 2" || fail "summary_parse missing failure code"
echo "$SUM_FAIL" | grep -q "Last 50 lines" || fail "summary_parse missing tail"

PORT=$(pick_port) || fail "failed to pick port"
nc -l 127.0.0.1 "$PORT" >/dev/null 2>&1 &
PORT_PID=$!
PIDS+=("$PORT_PID")
wait_port "$PORT" 5 1 || fail "wait_port did not detect listener"
kill -TERM "$PORT_PID" 2>/dev/null || true
wait "$PORT_PID" 2>/dev/null || true

BAD_PORT=$(pick_port) || fail "failed to pick bad port"
if wait_port "$BAD_PORT" 1 1; then
  fail "wait_port should have timed out"
fi

HTTP_PORT=$(pick_port) || fail "failed to pick http port"
python3 -m http.server "$HTTP_PORT" --bind 127.0.0.1 >/dev/null 2>&1 &
HTTP_PID=$!
PIDS+=("$HTTP_PID")
wait_http "http://127.0.0.1:$HTTP_PORT" 5 1 || fail "wait_http failed"
kill -TERM "$HTTP_PID" 2>/dev/null || true
wait "$HTTP_PID" 2>/dev/null || true

BAD_HTTP_PORT=$(pick_port) || fail "failed to pick bad http port"
if wait_http "http://127.0.0.1:$BAD_HTTP_PORT" 1 1; then
  fail "wait_http should have timed out"
fi

PID=$(start_service sleeper -- sleep 5)
sleep 0.2
if ! kill -0 "$PID" 2>/dev/null; then
  fail "start_service PID not running"
fi
stop_service "$PID" "sleeper"

WS_PORT=$(pick_port) || fail "failed to pick with_service port"
with_service web --wait-port "$WS_PORT" -- python3 -m http.server "$WS_PORT" --bind 127.0.0.1 --run \
  "$BASH" -c "nc -z 127.0.0.1 $WS_PORT"

CLEANUP_LOG="$TMPDIR/cleanup.log"
with_cleanup "echo first >> \"$CLEANUP_LOG\""
with_cleanup "echo second >> \"$CLEANUP_LOG\""
_run_cleanups
_cleanup_actions=()
_cleanup_initialized=false
trap - EXIT INT TERM
EXPECTED=$(printf "second\nfirst\n")
ACTUAL=$(cat "$CLEANUP_LOG")
if [ "$ACTUAL" != "$EXPECTED" ]; then
  echo "expected cleanup order:" >&2
  printf "%s" "$EXPECTED" >&2
  echo "" >&2
  echo "actual cleanup order:" >&2
  printf "%s" "$ACTUAL" >&2
  echo "" >&2
  fail "with_cleanup order mismatch"
fi
