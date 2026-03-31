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
    initLeaf = lifecycle.initLeaf;
    preflight-init = lifecycle.preflightInit;
    preflightInit = lifecycle.preflightInit;
    preflight-start = lifecycle.preflightStart;
    preflightStart = lifecycle.preflightStart;
    start = lifecycle.start;
    start-leaf = lifecycle.startLeaf;
    startLeaf = lifecycle.startLeaf;
    stop = lifecycle.stop;
    restart = lifecycle.restart;
    status = lifecycle.status;
    health = lifecycle.health;
    ready = lifecycle.ready;
    ready-test = lifecycle.readyTest;
    readyTest = lifecycle.readyTest;
    check-config = lifecycle.checkConfig;
    checkConfig = lifecycle.checkConfig;
    setup-db = lifecycle.setupDb;
    setupDb = lifecycle.setupDb;
    full-start = lifecycle.fullStart;
    full-start-leaf = lifecycle.fullStartLeaf;
    fullStart = lifecycle.fullStart;
    fullStartLeaf = lifecycle.fullStartLeaf;
    full-start-test = lifecycle.fullStartTest;
    full-start-test-leaf = lifecycle.fullStartTestLeaf;
    fullStartTest = lifecycle.fullStartTest;
    fullStartTestLeaf = lifecycle.fullStartTestLeaf;
    list-instances = lifecycle.listInstances;
    listInstances = lifecycle.listInstances;
    shell = shell;
    backup = backupMod.backup;
    restore = backupMod.restore;
    list-backups = backupMod.listBackups;
    listBackups = backupMod.listBackups;
    verify-backup = backupMod.verifyBackup;
    verifyBackup = backupMod.verifyBackup;
    cleanup-backups = backupMod.cleanupBackups;
    cleanupBackups = backupMod.cleanupBackups;
    test-migrations = migration.testMigrations;
    testMigrations = migration.testMigrations;
    ensure-migration-tested = migrationSafety.ensureMigrationTested;
    ensureMigrationTested = migrationSafety.ensureMigrationTested;
    check-port = portMgmt.checkPort;
    checkPort = portMgmt.checkPort;
    kill-port = portMgmt.killPort;
    killPort = portMgmt.killPort;
  };
}
