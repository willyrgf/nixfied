{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  cfg = config.nixfied.services.postgres;
  probeLib = import ./probes.nix { inherit lib; };
  contractSchema = import ./contract-schema.nix { inherit lib; };
  operationContractBuilder = import ./operation-contract-builder.nix;
  sourceOptions = import ./source-options.nix { inherit lib; };
  sourceSpec = sourceOptions.mkSourceSpec { };
  envConfigSpec = t.submodule {
    options.extraConfig = lib.mkOption {
      type = t.lines;
      default = "";
    };
  };
  serviceDir = contractSchema.mkServiceDirExpr cfg.dataDirName;
in
{
  options.nixfied.services.postgres = {
    enable = lib.mkOption {
      type = t.bool;
      default = false;
    };
    database = lib.mkOption {
      type = t.str;
      default = "app";
    };
    testDatabase = lib.mkOption {
      type = t.str;
      default = "app_test";
    };
    portKey = lib.mkOption {
      type = t.str;
      default = "postgres";
    };
    dataDirName = lib.mkOption {
      type = t.str;
      default = "postgres";
    };
    extensions = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    extraConfig = lib.mkOption {
      type = t.lines;
      default = "";
    };
    envConfigs = lib.mkOption {
      type = t.attrsOf envConfigSpec;
      default = { };
    };
    migrations = {
      dir = lib.mkOption {
        type = t.str;
        default = "migrations";
      };
      command = lib.mkOption {
        type = t.lines;
        default = "";
      };
      sourceDatabase = lib.mkOption {
        type = t.nullOr t.str;
        default = null;
      };
    };
    sources = lib.mkOption {
      type = t.attrsOf sourceSpec;
      default = { };
    };
    sourceKeys = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    defaultSource = lib.mkOption {
      type = t.str;
      default = "";
    };
    probes = probeLib.probeOptions;
    contract = contractSchema.mkContractOption "Typed PostgreSQL public contract.";
    implementation = contractSchema.mkImplementationOption "Private PostgreSQL runtime implementation.";
  };

  config.nixfied.services.postgres.contract = {
    version = 1;
    service = "postgres";
    summary = "PostgreSQL service management API";
    details = "Public service contract for managing PostgreSQL across dev/prod/test/ci.";
    ownerFile = "nixfied/modules/services/postgres.nix";
    artifacts = {
      portKey = cfg.portKey;
      portVar = contractSchema.mkPortVarName cfg.portKey;
      serviceDir = serviceDir;
      dataDir = serviceDir;
      logFile = "${serviceDir}/postgres.log";
      pidFile = "${serviceDir}/postmaster.pid";
      defaultDatabase = cfg.database;
      testDatabase = cfg.testDatabase;
    };
    runtimePrimitives = contractSchema.mkRuntimePrimitivesV1 config.nixfied.runtime;
    operations =
      (operationContractBuilder {
        displayName = "PostgreSQL";
        extraOperations = {
          init = {
            runtimeOp = "init-leaf";
            preOps = [ "preflight-init" ];
            summary = "Initialize PostgreSQL data directory";
            details = "Initializes PGDATA and writes environment-specific PostgreSQL configuration.";
          };

          preflight-init = {
            runtimeOp = "preflight-init";
            summary = "Validate PostgreSQL init preconditions";
            details = "Checks deterministic blockers before PostgreSQL initialization for the current slot and environment.";
            exposeApp = false;
          };

          start = {
            runtimeOp = "start-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Start PostgreSQL server";
            details = "Starts PostgreSQL for the current slot and environment.";
          };

          full-start = {
            runtimeOp = "full-start-leaf";
            preOps = [ "start" ];
            summary = "Init, start, and set up PostgreSQL";
            details = "Performs init/start/setup-db in one operation.";
          };

          full-start-test = {
            runtimeOp = "full-start-test-leaf";
            preOps = [ "start" ];
            summary = "Init/start/setup for test database";
            details = "Performs init/start/setup-db using the configured test database.";
          };

          setup-db = {
            runtimeOp = "setup-db";
            summary = "Create and configure database";
            details = "Creates the configured database and required extensions.";
          };

          ready-test = {
            runtimeOp = "ready-test";
            summary = "Wait for PostgreSQL test-database readiness";
            details = "Checks PostgreSQL and the configured test database accept local SQL queries.";
          };

          list-instances = {
            runtimeOp = "list-instances";
            summary = "List PostgreSQL instances";
            details = "Lists PostgreSQL instances managed by Nixfied.";
          };

          backup = {
            runtimeOp = "backup";
            summary = "Create PostgreSQL backup";
            details = "Creates a backup for the current slot and environment.";
            usage = [ "nix run .#svc::postgres::backup -- <args>" ];
          };

          restore = {
            runtimeOp = "restore";
            summary = "Restore PostgreSQL backup";
            details = "Restores PostgreSQL data from a selected backup.";
            usage = [ "nix run .#svc::postgres::restore -- <backup-path>" ];
          };

          list-backups = {
            runtimeOp = "list-backups";
            summary = "List PostgreSQL backups";
            details = "Lists backups for the current slot and environment.";
          };

          verify-backup = {
            runtimeOp = "verify-backup";
            summary = "Verify PostgreSQL backup";
            details = "Verifies backup archive integrity.";
            usage = [ "nix run .#svc::postgres::verify-backup -- <backup-path>" ];
          };

          cleanup-backups = {
            runtimeOp = "cleanup-backups";
            summary = "Prune old PostgreSQL backups";
            details = "Removes old backups while keeping the requested number of newest snapshots.";
            usage = [ "nix run .#svc::postgres::cleanup-backups -- <keep-count>" ];
          };

          test-migrations = {
            runtimeOp = "test-migrations";
            summary = "Test PostgreSQL migrations";
            details = "Runs migrations against a temporary copy of the source database.";
          };

          ensure-migration-tested = {
            runtimeOp = "ensure-migration-tested";
            summary = "Ensure migrations were tested";
            details = "Fails when migration hashes were not previously tested.";
            exposeApp = false;
          };

          check-port = {
            runtimeOp = "check-port";
            summary = "Check PostgreSQL port usage";
            details = "Checks whether a port is already in use.";
            usage = [ "nix run .#svc::postgres::check-port -- <port>" ];
          };

          kill-port = {
            runtimeOp = "kill-port";
            summary = "Kill processes bound to a port";
            details = "Stops processes listening on a given port.";
            usage = [ "nix run .#svc::postgres::kill-port -- <port>" ];
          };

          shell = {
            runtimeOp = "shell";
            summary = "Open PostgreSQL shell";
            details = "Opens psql connected to the configured slot/environment database.";
            usage = [ "nix run .#svc::postgres::shell -- <psql-args>" ];
          };
        };
      })
      // contractSchema.mkObservabilityOperations {
        service = "postgres";
        summaryName = "PostgreSQL";
      };
  };

  config.nixfied.services.postgres.implementation = {
    version = 1;
    module = ./runtime/postgres/default.nix;
  };
}
