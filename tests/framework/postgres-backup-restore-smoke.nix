{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  slotsStub =
    let
      slotInfo = pkgs.writeShellScript "postgres-backup-slot-info" ''
        printf 'SLOT=%s\n' "''${SLOT:-0}"
        printf 'ENV=%s\n' "''${ENV:-dev}"
        printf 'BACKUP_BASE_DIR=%s\n' "''${BACKUP_BASE_DIR:-}"
        printf 'PGDATA=%s\n' "''${PGDATA:-''${PGDATA_ROOT:-}/postgres}"
      '';
    in
    {
      getSlotInfo = slotInfo;
      getServiceDir = name: "\${PGDATA_ROOT}/${name}";
      portVarName = _: "POSTGRES_PORT";
    };

  backupMod = import ../../nixfied/framework/runtime/services/postgres/backup.nix {
    inherit pkgs;
    project = { };
    slots = slotsStub;
    config = {
      package = pkgs.postgresql_16;
      portKey = "postgres";
      dataDirName = "postgres";
    };
    loggingPrelude = shellHelpers.loggingPrelude;
  };
in
pkgs.runCommand "postgres-backup-restore-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RESTORE_BIN="${backupMod.restore}"
  LIST_BIN="${backupMod.listBackups}"

  export SLOT=0
  export ENV=dev
  export PGDATA_ROOT="$TMPDIR/service-root"
  export PGDATA="$PGDATA_ROOT/postgres"
  export BACKUP_BASE_DIR="$TMPDIR/backups"

  mkdir -p "$PGDATA" "$BACKUP_BASE_DIR/base"

  echo "original" > "$PGDATA/original.marker"
  mkdir -p "$BACKUP_BASE_DIR/base/bad-backup"
  printf 'not-a-tar' > "$BACKUP_BASE_DIR/base/bad-backup/base.tar.gz"

  set +e
  "$RESTORE_BIN" "$BACKUP_BASE_DIR/base/bad-backup" > "$TMPDIR/restore-fail.out" 2>&1
  restore_fail_rc="$?"
  set -e

  if [ "$restore_fail_rc" -eq 0 ]; then
    cat "$TMPDIR/restore-fail.out"
    fail "restore should fail for invalid archive"
  fi

  require_file "$PGDATA/original.marker"
  if find "$PGDATA_ROOT" -maxdepth 1 -name '.postgres-restore-staging.*' | ${pkgs.gnugrep}/bin/grep -q .; then
    fail "restore failure should not leave staging directories behind"
  fi

  rm -rf "$PGDATA"
  mkdir -p "$PGDATA"
  echo "old-data" > "$PGDATA/old.marker"

  mkdir -p "$TMPDIR/good-backup-payload"
  echo "16" > "$TMPDIR/good-backup-payload/PG_VERSION"
  echo "new-data" > "$TMPDIR/good-backup-payload/new.marker"
  mkdir -p "$BACKUP_BASE_DIR/base/good-backup"
  ${pkgs.gnutar}/bin/tar czf "$BACKUP_BASE_DIR/base/good-backup/base.tar.gz" -C "$TMPDIR/good-backup-payload" .

  "$RESTORE_BIN" "$BACKUP_BASE_DIR/base/good-backup" > "$TMPDIR/restore-success.out" 2>&1

  require_file "$PGDATA/PG_VERSION"
  require_file "$PGDATA/new.marker"
  require_not_file "$PGDATA/old.marker"
  if find "$PGDATA_ROOT" -maxdepth 1 -name 'postgres.previous.*' | ${pkgs.gnugrep}/bin/grep -q .; then
    fail "successful restore should not leave previous data directories behind"
  fi

  cat > "$BACKUP_BASE_DIR/base/backup-20260101-000000.manifest.json" <<'EOF'
  {
    "timestamp": "20260101-000000",
    "created_at": "2026-01-01T00:00:00Z",
    "git_commit": "abc1234"
  }
EOF
  cat > "$BACKUP_BASE_DIR/base/backup-20260101-000000.manifest.fields" <<'EOF'
created_at='2026-01-01T00:00:00Z'
git_commit='abc1234'
EOF

  "$LIST_BIN" > "$TMPDIR/list-backups.out"
  require_contains "$TMPDIR/list-backups.out" "backup-20260101-000000 (created: 2026-01-01T00:00:00Z, commit: abc1234)"

  echo "OK: postgres restore uses stage-and-swap and manifests are parsed structurally" > "$out"
''
