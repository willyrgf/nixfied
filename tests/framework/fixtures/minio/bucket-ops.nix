{
  minioInit,
  minioStart,
  minioStop,
  minioHealth,
  minioReady,
  minioStatus,
  minioCheckConfig,
  minioBucketCreate,
  minioBucketDelete,
  minioBucketList,
  minioPolicyApply,
}:
''

    set -euo pipefail

    fail() {
      echo "FAIL: $*" >&2
      exit 1
    }

    wait_ok() {
      local attempts="$1"
      local interval="$2"
      shift 2
      local i
      for i in $(seq 1 "$attempts"); do
        if "$@" >/dev/null 2>&1; then
          return 0
        fi
        sleep "$interval"
      done
      return 1
    }

    wait_fail() {
      local attempts="$1"
      local interval="$2"
      shift 2
      local i
      for i in $(seq 1 "$attempts"); do
        if "$@" >/dev/null 2>&1; then
          sleep "$interval"
        else
          return 0
        fi
      done
      return 1
    }

    cleanup_minio() {
      ${minioStop} >/dev/null 2>&1 || true
      if [ -n "''${MINIO_PID:-}" ] && kill -0 "$MINIO_PID" 2>/dev/null; then
        stop_service "$MINIO_PID" "minio"
      fi
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

    used_ports=""
    assign_port() {
      local name="$1"
      local candidate
      while true; do
        candidate="$(pick_port)" || return 1
        case " $used_ports " in
          *" $candidate "*) ;;
          *)
            used_ports="$used_ports $candidate"
            export "$name=$candidate"
            return 0
            ;;
        esac
      done
    }

    eval "$("$SLOT_INFO")"
    assign_port API_PORT || fail "failed to pick API_PORT"
    assign_port CONSOLE_PORT || fail "failed to pick CONSOLE_PORT"
    # Support both fixture naming (MINIOAPI_PORT/MINIOCONSOLE_PORT) and
    # project naming (MINIO_PORT/MINIO_CONSOLE_PORT).
    export MINIOAPI_PORT="$API_PORT"
    export MINIOCONSOLE_PORT="$CONSOLE_PORT"
    export MINIO_PORT="$API_PORT"
    export MINIO_CONSOLE_PORT="$CONSOLE_PORT"

    ${minioInit}
    ${minioInit}

    MINIO_DIR="$BASE_DIR/minio-$SLOT-$ENV"
    [ -d "$MINIO_DIR/data" ] || fail "missing minio data dir"

    ${minioCheckConfig} >/dev/null || fail "minioCheckConfig failed"

    set +e
    ${minioHealth} >/dev/null 2>&1
    RC=$?
    set -e
    [ "$RC" -ne 0 ] || fail "minioHealth should fail before start"

    set +e
    ${minioReady} >/dev/null 2>&1
    RC=$?
    set -e
    [ "$RC" -ne 0 ] || fail "minioReady should fail before start"

    MINIO_PID=""
    MINIO_PID=$(start_service minio -- ${minioStart})
    READY=0
    for _ in $(seq 1 50); do
      if ${minioReady} >/dev/null 2>&1; then
        READY=1
        break
      fi
      sleep 0.2
    done
    if [ "$READY" -ne 1 ]; then
      print_log_tail "$MINIO_DIR/logs/minio.log" 50
      cleanup_minio
      fail "minio did not become ready"
    fi

    if ! wait_ok 25 0.2 ${minioHealth}; then
      print_log_tail "$MINIO_DIR/logs/minio.log" 50
      cleanup_minio
      fail "minioHealth should pass while running"
    fi

    if ! wait_ok 15 0.2 ${minioStatus}; then
      cleanup_minio
      fail "minioStatus should pass while running"
    fi

    ${minioBucketCreate} "fixture-bucket"
    LIST_OUT="$PWD/minio-buckets.log"
    ${minioBucketList} > "$LIST_OUT"
    grep -q "fixture-bucket" "$LIST_OUT" || fail "bucket list missing fixture-bucket"

    set +e
    ${minioPolicyApply} "fixture-bucket" "$PWD/no-such-policy.json" >/dev/null 2>&1
    RC=$?
    set -e
    [ "$RC" -ne 0 ] || fail "policyApply should fail for missing file"

    POLICY_FILE="$PWD/public-policy.json"
    cat > "$POLICY_FILE" <<JSON
    {
      "Version": "2012-10-17",
      "Statement": [{"Effect": "Allow", "Principal": {"AWS": ["*"]}, "Action": ["s3:GetObject"], "Resource": ["arn:aws:s3:::fixture-bucket/*"]}]
    }
  JSON
    ${minioPolicyApply} "fixture-bucket" "$POLICY_FILE" >/dev/null || fail "policyApply failed"

    ${minioBucketDelete} "fixture-bucket" >/dev/null || fail "bucketDelete failed"
    ${minioBucketList} > "$LIST_OUT"
    if grep -q "fixture-bucket" "$LIST_OUT"; then
      fail "bucket should be deleted"
    fi

    cleanup_minio

    MINIO_DOWN=0
    for _ in $(seq 1 40); do
      if ${minioHealth} >/dev/null 2>&1; then
        sleep 0.2
      else
        MINIO_DOWN=1
        break
      fi
    done
    [ "$MINIO_DOWN" -eq 1 ] || fail "minioHealth should fail after stop"

    if ! wait_fail 20 0.2 ${minioReady}; then
      fail "minioReady should fail after stop"
    fi

    echo "minio bucket ops fixture ok"

''
