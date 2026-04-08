{
  pkgs,
  project,
  slots,
}:
let
  common = import ./common.nix {
    inherit
      (pkgs) lib
      ;
    inherit
      pkgs
      project
      slots
      ;
  };
  base = common.mkBaseOperations {
    serviceName = "postgres";
    displayName = "PostgreSQL";
    dataDirName = "postgres";
    logFileName = "postgres.log";
    pidFileName = "postgres.pid";
  };
  mkCommand = base.mkCommand;
in
{
  version = 1;
  operations = base.operations // {
    "setup-db" = mkCommand "setup-db" ''
      mkdir -p "$db_dir"
      touch "$db_dir/$default_database.sql"
      append_operation "setup-db:$service_name"
      echo "OK: service=$service_name database=$default_database setup=complete"
    '';

    "ready-test" = mkCommand "ready-test" ''
      require_running
      touch "$db_dir/$test_database.sql"
      append_operation "ready-test:$service_name"
      echo "OK: service=$service_name test_database=$test_database ready=up"
    '';

    "list-instances" = mkCommand "list-instances" ''
      state="stopped"
      if is_running; then
        state="running"
      fi
      append_operation "list-instances:$service_name"
      echo "INFO: service=$service_name state=$state slot=''${SLOT:-} env=''${ENV:-}"
    '';

    backup = mkCommand "backup" ''
      require_running
      backup_path="''${1:-$backup_dir/default-backup.sql}"
      mkdir -p "$(dirname "$backup_path")"
      printf 'service=%s\ndatabase=%s\n' "$service_name" "$default_database" > "$backup_path"
      append_operation "backup:$service_name"
      echo "OK: service=$service_name backup=$backup_path"
    '';

    restore = mkCommand "restore" ''
      backup_path="''${1:-}"
      if [ -z "$backup_path" ]; then
        echo "ERROR: usage: restore <backup-path>"
        exit 2
      fi
      if [ ! -f "$backup_path" ]; then
        echo "ERROR: backup missing path=$backup_path"
        exit 1
      fi
      cp "$backup_path" "$state_dir/restored-backup.sql"
      append_operation "restore:$service_name"
      echo "OK: service=$service_name restore=$backup_path"
    '';

    "list-backups" = mkCommand "list-backups" ''
      append_operation "list-backups:$service_name"
      found=0
      for backup_path in "$backup_dir"/*; do
        if [ ! -e "$backup_path" ]; then
          continue
        fi
        found=1
        echo "INFO: backup=$backup_path"
      done
      if [ "$found" -eq 0 ]; then
        echo "INFO: backup=none"
      fi
    '';

    "verify-backup" = mkCommand "verify-backup" ''
      backup_path="''${1:-}"
      if [ -z "$backup_path" ]; then
        echo "ERROR: usage: verify-backup <backup-path>"
        exit 2
      fi
      if [ ! -f "$backup_path" ]; then
        echo "ERROR: backup missing path=$backup_path"
        exit 1
      fi
      append_operation "verify-backup:$service_name"
      echo "OK: service=$service_name backup=$backup_path verified"
    '';

    "cleanup-backups" = mkCommand "cleanup-backups" ''
      keep_count="''${1:-1}"
      if ! printf '%s' "$keep_count" | grep -Eq '^[0-9]+$'; then
        echo "ERROR: keep-count must be an integer"
        exit 2
      fi
      append_operation "cleanup-backups:$service_name"
      if [ "$keep_count" -eq 0 ]; then
        rm -f "$backup_dir"/*
      else
        count=0
        for backup_path in $(${pkgs.coreutils}/bin/ls -1t "$backup_dir" 2>/dev/null || true); do
          count="$(( count + 1 ))"
          if [ "$count" -le "$keep_count" ]; then
            continue
          fi
          rm -f "$backup_dir/$backup_path"
        done
      fi
      echo "OK: service=$service_name backups_kept=$keep_count"
    '';

    "test-migrations" = mkCommand "test-migrations" ''
      require_running
      mkdir -p "$migration_dir"
      printf '%s\n' "$default_database" > "$migration_dir/last-tested-db"
      append_operation "test-migrations:$service_name"
      echo "OK: service=$service_name migrations=tested"
    '';

    "ensure-migration-tested" = mkCommand "ensure-migration-tested" ''
      if [ ! -f "$migration_dir/last-tested-db" ]; then
        echo "ERROR: service=$service_name migrations=untested"
        exit 1
      fi
      append_operation "ensure-migration-tested:$service_name"
      echo "OK: service=$service_name migrations=verified"
    '';

    "check-port" = mkCommand "check-port" ''
      port_value="''${1:-''${port_primary:-}}"
      if [ -z "$port_value" ]; then
        echo "ERROR: port is required"
        exit 2
      fi
      if ! printf '%s' "$port_value" | ${pkgs.gnugrep}/bin/grep -Eq '^[0-9]+$'; then
        echo "ERROR: invalid port=$port_value"
        exit 2
      fi
      append_operation "check-port:$service_name"
      echo "OK: service=$service_name port=$port_value available"
    '';

    "kill-port" = mkCommand "kill-port" ''
      port_value="''${1:-''${port_primary:-}}"
      if [ -z "$port_value" ]; then
        echo "ERROR: port is required"
        exit 2
      fi
      append_operation "kill-port:$service_name"
      echo "OK: service=$service_name port=$port_value cleared"
    '';
  };
}
