{ }:
''
  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  TMPDIR=$(mktemp -d)
  PIDS=()
  cleanup() {
    for pid in "''${PIDS[@]}"; do
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
  log_capture "$LOG" -- "$BASH" -c "echo \"hello\"; echo \"err\" >&2"
  grep -q "hello" "$LOG" || fail "log_capture missing stdout"
  grep -q "err" "$LOG" || fail "log_capture missing stderr"

  TEE_LOG="$TMPDIR/log_capture_tee.log"
  TEE_OUT=$(LOG_TEE=1 log_capture "$TEE_LOG" -- "$BASH" -c "echo \"tee-ok\"")
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

  # Example: summary_parse should be resilient to missing log files.
  SUM_MISSING=$(summary_parse "$TMPDIR/does-not-exist.log" 1 3)
  echo "$SUM_MISSING" | grep -q "Exit code: 3" || fail "summary_parse missing exit code for missing log"

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

  # Example: artifact_path should create the artifacts directory.
  CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  ART_PATH=$(artifact_path "hello.txt")
  echo "artifact ok" > "$ART_PATH"
  if [ ! -f "$ART_PATH" ]; then
    fail "artifact_path did not create file"
  fi
  if [ ! -d "$CI_ARTIFACTS_DIR" ]; then
    fail "artifact_path did not create artifacts dir"
  fi

  PID=$(start_service sleeper -- sleep 5)
  sleep 0.2
  if ! kill -0 "$PID" 2>/dev/null; then
    fail "start_service PID not running"
  fi
  stop_service "$PID" "sleeper"

  PID2=""
  start_service_into PID2 sleeper2 -- sleep 5
  if [ -z "$PID2" ]; then
    fail "start_service_into did not populate pid variable"
  fi
  stop_service "$PID2" "sleeper2"

  unset SERVICE_OWNER_SCOPE SERVICE_REUSE_POLICY SERVICE_DISCOVERY_SCOPE

  export SERVICE_REUSE_POLICY="same-slot"
  KEEP_POLICY_PID=""
  start_service_into KEEP_POLICY_PID keep-policy -- sleep 30
  _run_cleanups
  if ! kill -0 "$KEEP_POLICY_PID" 2>/dev/null; then
    fail "start_service should preserve process for same-slot reuse"
  fi
  stop_service "$KEEP_POLICY_PID" "keep-policy"
  _cleanup_actions=()
  _cleanup_initialized=false

  export SERVICE_REUSE_POLICY="never"
  CLEAN_POLICY_PID=""
  start_service_into CLEAN_POLICY_PID clean-policy -- sleep 30
  _run_cleanups
  sleep 0.2
  if kill -0 "$CLEAN_POLICY_PID" 2>/dev/null; then
    fail "start_service should stop process for never reuse"
  fi
  _cleanup_actions=()
  _cleanup_initialized=false

  export SERVICE_REUSE_POLICY="never"
  KEEP_OVERRIDE_PID=""
  start_service_into KEEP_OVERRIDE_PID keep-override --keep-running -- sleep 30
  _run_cleanups
  if ! kill -0 "$KEEP_OVERRIDE_PID" 2>/dev/null; then
    fail "start_service --keep-running should override cleanup policy"
  fi
  stop_service "$KEEP_OVERRIDE_PID" "keep-override"
  _cleanup_actions=()
  _cleanup_initialized=false

  export SERVICE_REUSE_POLICY="same-slot"
  CLEAN_OVERRIDE_PID=""
  start_service_into CLEAN_OVERRIDE_PID clean-override --cleanup -- sleep 30
  _run_cleanups
  sleep 0.2
  if kill -0 "$CLEAN_OVERRIDE_PID" 2>/dev/null; then
    fail "start_service --cleanup should override keep policy"
  fi
  _cleanup_actions=()
  _cleanup_initialized=false

  POLICY_ERR="$TMPDIR/start-policy-invalid.log"
  set +e
  SERVICE_REUSE_POLICY=cross-run SERVICE_OWNER_SCOPE=ephemeral SERVICE_DISCOVERY_SCOPE=global \
    start_service invalid-policy -- sleep 30 > /dev/null 2>"$POLICY_ERR"
  POLICY_RC=$?
  set -e
  if [ "$POLICY_RC" -eq 0 ]; then
    fail "start_service should fail for invalid policy matrix"
  fi
  grep -q "cross-run reuse requires SERVICE_OWNER_SCOPE=persistent and SERVICE_DISCOVERY_SCOPE=global" "$POLICY_ERR" \
    || fail "start_service should print actionable policy matrix error"

  unset SERVICE_OWNER_SCOPE SERVICE_REUSE_POLICY SERVICE_DISCOVERY_SCOPE

  READY_PID_FILE="$TMPDIR/readiness-pid"
  FREE_PORT=$(pick_port) || fail "failed to pick free port for readiness failure"
  set +e
  start_service bad-ready --wait-port "$FREE_PORT" --timeout 1 -- \
    "$BASH" -c "echo \$\$ > \"$READY_PID_FILE\"; sleep 30" >/dev/null 2>&1
  READY_RC=$?
  set -e
  if [ "$READY_RC" -eq 0 ]; then
    fail "start_service should fail readiness check"
  fi
  if [ ! -f "$READY_PID_FILE" ]; then
    fail "start_service readiness test did not create pid file"
  fi
  READY_PID=$(cat "$READY_PID_FILE")
  sleep 0.2
  if kill -0 "$READY_PID" 2>/dev/null; then
    fail "start_service did not stop failed readiness process"
  fi

  WS_PORT=$(pick_port) || fail "failed to pick with_service port"
  with_service web --wait-port "$WS_PORT" -- python3 -m http.server "$WS_PORT" --bind 127.0.0.1 --run \
    "$BASH" -c "nc -z 127.0.0.1 $WS_PORT"
  # Example: with_service should stop the daemon after the run step.
  if nc -z 127.0.0.1 "$WS_PORT" >/dev/null 2>&1; then
    fail "with_service did not stop service"
  fi

  CLEANUP_LOG="$TMPDIR/cleanup.log"
  with_cleanup "$BASH" -c "echo first >> \"$CLEANUP_LOG\""
  with_cleanup "$BASH" -c "echo second >> \"$CLEANUP_LOG\""
  cleanup_from_function() {
    echo function >> "$CLEANUP_LOG"
  }
  with_cleanup cleanup_from_function
  _run_cleanups
  _cleanup_actions=()
  _cleanup_initialized=false
  trap - EXIT INT TERM
  EXPECTED=$(printf "function\nsecond\nfirst\n")
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

''
