# PostgreSQL runtime adapter
{
  pkgs,
  project,
  slots,
}:

let
  runtimeDefaults = import ../../../../framework/core/runtime-defaults.nix;
  summary = import ../../../../framework/runtime/helpers/summary.nix { inherit pkgs project; };
  helpers = import ../../../../framework/runtime/helpers/helpers.nix {
    inherit pkgs project;
    inherit (summary) summaryParser;
  };
  loggingPrelude = helpers.loggingPrelude;
  config = import ./config.nix { inherit pkgs project; };
  pgPackage = config.package or pkgs.postgresql_16;
  pgDatabase = config.database or "app";
  portKey = config.portKey or "postgres";
  portVar = slots.portVarName portKey;

  lifecycle = import ./lifecycle.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  backupMod = import ./backup.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  migration = import ./migration.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  migrationSafety = import ./migration-safety.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  portMgmt = import ./port-management.nix {
    inherit pkgs loggingPrelude;
  };
  shell = pkgs.writeShellScript "postgres-shell" ''
    set -euo pipefail
    source <(${slots.getSlotInfo})

    PORT_VAR="${portVar}"
    PGPORT="''${!PORT_VAR}"
    PGDATABASE="''${PGDATABASE:-${pgDatabase}}"

    exec ${pgPackage}/bin/psql "postgresql://${runtimeDefaults.hosts.localhost}:$PGPORT/$PGDATABASE" "$@"
  '';
in
{
  version = 1;
  operations = {
    init = lifecycle.init;
    init-leaf = lifecycle.initLeaf;
    preflight-init = lifecycle.preflightInit;
    preflight-start = lifecycle.preflightStart;
    start = lifecycle.start;
    start-leaf = lifecycle.startLeaf;
    stop = lifecycle.stop;
    restart = lifecycle.restart;
    status = lifecycle.status;
    health = lifecycle.health;
    ready = lifecycle.ready;
    ready-test = lifecycle.readyTest;
    check-config = lifecycle.checkConfig;
    setup-db = lifecycle.setupDb;
    full-start = lifecycle.fullStart;
    full-start-leaf = lifecycle.fullStartLeaf;
    full-start-test = lifecycle.fullStartTest;
    full-start-test-leaf = lifecycle.fullStartTestLeaf;
    list-instances = lifecycle.listInstances;
    shell = shell;
    backup = backupMod.backup;
    restore = backupMod.restore;
    list-backups = backupMod.listBackups;
    verify-backup = backupMod.verifyBackup;
    cleanup-backups = backupMod.cleanupBackups;
    test-migrations = migration.testMigrations;
    ensure-migration-tested = migrationSafety.ensureMigrationTested;
    check-port = portMgmt.checkPort;
    kill-port = portMgmt.killPort;
  };
}
