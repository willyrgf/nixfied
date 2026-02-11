{
  backup,
  listBackups,
  verifyBackup,
  cleanupBackups,
  restore,
  findBackupForCommit,
  testRollback,
  testMigrations,
  ensureMigrationTested,
  markMigrationTested,
  detectDrift,
  checkPort,
  killPort,
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
  export PGDATA="$BASE_DIR/postgres-$SLOT-$ENV"
  mkdir -p "$PGDATA"

  BACKUP_DIR="$BACKUP_BASE_DIR/base"
  mkdir -p "$BACKUP_DIR"

  B1="$BACKUP_DIR/backup-001"
  B2="$BACKUP_DIR/backup-002"
  mkdir -p "$B1" "$B2"
  echo "backup one" > "$B1/data.txt"
  sleep 1
  echo "backup two" > "$B2/data.txt"

  GIT_COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "deadbeef")
  cat > "$B1.manifest.json" <<JSON
  {
    "git_commit": "$GIT_COMMIT",
    "name": "backup-001",
    "created_at": "now"
  }
JSON
  cat > "$B2.manifest.json" <<JSON
  {
    "git_commit": "$GIT_COMMIT",
    "name": "backup-002",
    "created_at": "now"
  }
JSON

  LIST_OUT="$PWD/postgres-list.log"
  ${listBackups} > "$LIST_OUT"
  grep -q "backup-001" "$LIST_OUT" || fail "listBackups missing backup-001"

  ${verifyBackup} "$B1" >/dev/null

  B_BAD="$BACKUP_DIR/bad-archive"
  mkdir -p "$B_BAD"
  printf "not-a-gzip" > "$B_BAD/base.tar.gz"
  set +e
  ${verifyBackup} "$B_BAD" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "verifyBackup should fail for corrupted archive"

  FOUND=$(${findBackupForCommit} "$GIT_COMMIT")
  [ -n "$FOUND" ] || fail "findBackupForCommit returned empty"

  ${testRollback} "$GIT_COMMIT" >/dev/null

  ${cleanupBackups} 1 >/dev/null
  COUNT=$(find "$BACKUP_DIR" -maxdepth 1 -type d -name 'backup-*' | wc -l | tr -d '[:space:]')
  if [ "$COUNT" -gt 1 ]; then
    fail "cleanupBackups should keep at most one backup, got $COUNT"
  fi

  set +e
  ${restore} "$BACKUP_DIR/does-not-exist" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "restore should fail for missing backup"

  set +e
  ${findBackupForCommit} "does-not-exist" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "findBackupForCommit should fail for unknown commit"

  MIG_DIR="$PWD/migrations"
  mkdir -p "$MIG_DIR"
  cat > "$MIG_DIR/001_init.sql" <<SQL
  create table if not exists t(id int);
SQL

  set +e
  ${ensureMigrationTested} "$MIG_DIR" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "ensureMigrationTested should fail before mark"

  ${markMigrationTested} "$MIG_DIR" >/dev/null
  ${ensureMigrationTested} "$MIG_DIR" >/dev/null
  ${detectDrift} "$MIG_DIR" >/dev/null

  echo "-- drift" >> "$MIG_DIR/001_init.sql"
  set +e
  ${detectDrift} "$MIG_DIR" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "detectDrift should fail after migration change"

  ${testMigrations} >/dev/null || fail "testMigrations should skip cleanly when no command configured"

  PORT=$(pick_port) || fail "failed to pick port"
  nc -l 127.0.0.1 "$PORT" >/dev/null 2>&1 &
  NC_PID=$!
  sleep 0.3

  set +e
  ${checkPort} "$PORT" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "checkPort should fail while listener is running"

  ${killPort} "$PORT" >/dev/null
  sleep 0.3
  ${checkPort} "$PORT" >/dev/null || fail "checkPort should pass after killPort"

  kill -TERM "$NC_PID" 2>/dev/null || true
  wait "$NC_PID" 2>/dev/null || true

  echo "postgres extensions fixture ok"

''
