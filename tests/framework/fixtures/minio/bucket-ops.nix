{
  minioInit,
  minioStart,
  minioStop,
  minioHealth,
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

  eval "$("$SLOT_INFO")"
  export MINIOAPI_PORT=$(pick_port)
  export MINIOCONSOLE_PORT=$(pick_port)

  ${minioInit}
  ${minioInit}

  MINIO_DIR="$BASE_DIR/minio-$SLOT-$ENV"
  [ -d "$MINIO_DIR/data" ] || fail "missing minio data dir"

  ${minioCheckConfig} >/dev/null || fail "minioCheckConfig failed"

  MINIO_PID=$(start_service minio -- ${minioStart})
  READY=0
  for _ in $(seq 1 50); do
    if ${minioHealth} >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.2
  done
  [ "$READY" -eq 1 ] || fail "minio did not become healthy"

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

  echo "minio bucket ops fixture ok"

''
