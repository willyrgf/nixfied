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

    MINIO_PID=$(start_service minio -- ${minioStart})
    READY=0
    for _ in $(seq 1 50); do
      if ${minioReady} >/dev/null 2>&1; then
        READY=1
        break
      fi
      sleep 0.2
    done
    [ "$READY" -eq 1 ] || fail "minio did not become ready"

    ${minioHealth} >/dev/null || fail "minioHealth should pass while running"

    ${minioStatus} >/dev/null || fail "minioStatus should pass while running"

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

    ${minioStop}
    if kill -0 "$MINIO_PID" 2>/dev/null; then
      stop_service "$MINIO_PID" "minio"
    fi

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

    set +e
    ${minioReady} >/dev/null 2>&1
    RC=$?
    set -e
    [ "$RC" -ne 0 ] || fail "minioReady should fail after stop"

    echo "minio bucket ops fixture ok"

''
