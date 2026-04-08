{
  lib,
  pkgs,
  conf,
}:
let
  ownerFile = "proof-workspace/seed/nixfied/project/services.nix";

  mkShimPackage =
    name:
    pkgs.writeShellScriptBin "proof-${name}-shim" ''
      set -euo pipefail
      echo "INFO: proof ${name} shim package"
    '';

  mkServiceOpProbe =
    serviceName: runtimeOp:
    {
      kind = "exec";
      command = ''
        if [ -z "''${NIXFIED_RUNTIME_BIN:-}" ]; then
          echo "ERROR: NIXFIED_RUNTIME_BIN is required"
          exit 1
        fi
        "$NIXFIED_RUNTIME_BIN" run-service ${lib.escapeShellArg serviceName} ${lib.escapeShellArg runtimeOp} >/dev/null
      '';
    };

  mkCommonService =
    {
      serviceName,
      displayName,
      endpoints,
      dataDirName ? serviceName,
      sourceKinds ? { },
      extraConfig ? { },
      extraOps ? { },
      source ? { },
      requireClientPackage ? false,
      artifacts ? { },
    }:
    let
      serviceDir = "\${NIXFIED_SERVICE_ROOT}/${dataDirName}";
      sourceRecord =
        {
          package = mkShimPackage serviceName;
        }
        // lib.optionalAttrs requireClientPackage {
          clientPackage = mkShimPackage "${serviceName}-client";
        }
        // source;
    in
    {
      enable = true;
      summary = "${displayName} service management API";
      details = "Public service contract for managing ${displayName} across dev/prod/test/ci.";
      inherit ownerFile;
      profiles = [
        "dev"
        "prod"
        "test"
        "ci"
      ];
      inherit dataDirName endpoints sourceKinds;
      sources.shim = sourceRecord;
      defaultSource = "shim";
      requiredSourceArtifacts =
        [ "package" ] ++ lib.optionals requireClientPackage [ "clientPackage" ];
      artifacts =
        {
          inherit serviceDir;
          dataDir = "${serviceDir}/data";
          logFile = "${serviceDir}/logs/${serviceName}.log";
          pidFile = "${serviceDir}/run/${serviceName}.pid";
        }
        // artifacts;
      lifecycle = {
        preStart = {
          runtimeOp = "pre-start";
          summary = "Prepare ${displayName} startup";
          details = "Runs deterministic setup and validation before starting ${displayName}.";
          exposeApp = false;
        };
        start = {
          runtimeOp = "start-leaf";
          preOps = [
            "init"
            "check-config"
            "preflight-start"
          ];
          summary = "Start ${displayName}";
          details = "Starts ${displayName} for the current slot/environment.";
        };
        status = {
          runtimeOp = "status";
          summary = "Show ${displayName} status";
          details = "Prints ${displayName} status for the current slot/environment.";
        };
        preStop = {
          runtimeOp = "pre-stop";
          summary = "Prepare ${displayName} shutdown";
          details = "Runs deterministic shutdown preparation for ${displayName}.";
          exposeApp = false;
        };
        stop = {
          runtimeOp = "stop";
          summary = "Stop ${displayName}";
          details = "Stops ${displayName} for the current slot/environment.";
        };
      };
      checks = {
        health = {
          runtimeOp = "health";
          summary = "Run ${displayName} health check";
          details = "Checks ${displayName} health for the current slot/environment.";
          steps = [ (mkServiceOpProbe serviceName "health") ];
        };
        ready = {
          runtimeOp = "ready";
          summary = "Wait for ${displayName} readiness";
          details = "Waits for ${displayName} to be ready for the current slot/environment.";
          steps = [ (mkServiceOpProbe serviceName "ready") ];
        };
      };
      extraOps =
        {
          init = {
            runtimeOp = "init";
            summary = "Initialize ${displayName} runtime directories";
            details = "Creates ${displayName} runtime directories for the current slot/environment.";
          };
          "preflight-start" = {
            runtimeOp = "preflight-start";
            summary = "Validate ${displayName} start preconditions";
            details = "Checks deterministic blockers before ${displayName} startup.";
            exposeApp = false;
          };
          "check-config" = {
            runtimeOp = "check-config";
            summary = "Validate ${displayName} configuration";
            details = "Validates ${displayName} configuration for the current slot/environment.";
          };
          restart = {
            runtimeOp = null;
            preOps = [
              "stop"
              "start"
            ];
            summary = "Restart ${displayName}";
            details = "Stops then starts ${displayName} for the current slot/environment.";
          };
          "full-start" = {
            runtimeOp = "full-start-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Init/check/start ${displayName}";
            details = "Performs init + check-config + start for ${displayName}.";
          };
          "full-start-test" = {
            runtimeOp = "full-start-test-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Init/check/start ${displayName} for test profile";
            details = "Performs init + check-config + start for ${displayName} (test profile).";
          };
        }
        // extraOps;
      implementation = {
        version = 1;
        module = ../local/service-shims/${serviceName}.nix;
      };
    }
    // extraConfig;

  postgresService = mkCommonService {
    serviceName = "postgres";
    displayName = "PostgreSQL";
    endpoints.primary = {
      protocol = "postgres";
      portKey = "postgres";
    };
    extraConfig = {
      database = "app";
      testDatabase = "app_test";
    };
    artifacts = {
      portVar = "POSTGRES_PORT";
      defaultDatabase = "app";
      testDatabase = "app_test";
    };
    extraOps = {
      "setup-db" = {
        runtimeOp = "setup-db";
        summary = "Create and configure database";
        details = "Creates the configured database and proof fixture markers.";
      };
      "ready-test" = {
        runtimeOp = "ready-test";
        summary = "Wait for PostgreSQL test-database readiness";
        details = "Checks PostgreSQL and the configured test database accept proof queries.";
      };
      "list-instances" = {
        runtimeOp = "list-instances";
        summary = "List PostgreSQL instances";
        details = "Lists PostgreSQL proof instances managed by Nixfied.";
      };
      backup = {
        runtimeOp = "backup";
        summary = "Create PostgreSQL backup";
        details = "Creates a deterministic proof backup for the current slot/environment.";
      };
      restore = {
        runtimeOp = "restore";
        summary = "Restore PostgreSQL backup";
        details = "Restores PostgreSQL proof data from a selected backup.";
      };
      "list-backups" = {
        runtimeOp = "list-backups";
        summary = "List PostgreSQL backups";
        details = "Lists PostgreSQL proof backups for the current slot/environment.";
      };
      "verify-backup" = {
        runtimeOp = "verify-backup";
        summary = "Verify PostgreSQL backup";
        details = "Verifies proof backup file presence.";
      };
      "cleanup-backups" = {
        runtimeOp = "cleanup-backups";
        summary = "Prune old PostgreSQL backups";
        details = "Removes old proof backups while keeping the requested number newest.";
      };
      "test-migrations" = {
        runtimeOp = "test-migrations";
        summary = "Test PostgreSQL migrations";
        details = "Runs proof migration checks against the configured source database marker.";
      };
      "ensure-migration-tested" = {
        runtimeOp = "ensure-migration-tested";
        summary = "Ensure migrations were tested";
        details = "Fails when proof migration markers were not previously tested.";
        exposeApp = false;
      };
      "check-port" = {
        runtimeOp = "check-port";
        summary = "Check PostgreSQL port usage";
        details = "Checks whether a proof port token is valid.";
      };
      "kill-port" = {
        runtimeOp = "kill-port";
        summary = "Release PostgreSQL port";
        details = "Clears proof PostgreSQL port ownership markers.";
      };
    };
  };

  nginxService = mkCommonService {
    serviceName = "nginx";
    displayName = "nginx";
    endpoints = {
      http = {
        protocol = "http";
        portKey = "http";
      };
      https = {
        protocol = "https";
        portKey = "https";
      };
    };
    artifacts = {
      httpPortVar = "HTTP_PORT";
      httpsPortVar = "HTTPS_PORT";
    };
    extraOps = {
      reload = {
        runtimeOp = "reload";
        summary = "Reload nginx configuration";
        details = "Validates and reloads proof nginx configuration.";
      };
      "list-instances" = {
        runtimeOp = "list-instances";
        summary = "List nginx instances";
        details = "Lists proof nginx instances managed by Nixfied.";
      };
      "site-proxy" = {
        runtimeOp = "site-proxy";
        summary = "Write proxy site configuration";
        details = "Writes a proxy site config and enables it.";
        exposeApp = false;
      };
      "site-static" = {
        runtimeOp = "site-static";
        summary = "Write static site configuration";
        details = "Writes a static site config and enables it.";
        exposeApp = false;
      };
      "site-add" = {
        runtimeOp = "site-add";
        summary = "Add proxy nginx site";
        details = "Adds a proof proxy site and enables it.";
      };
      "site-remove" = {
        runtimeOp = "site-remove";
        summary = "Remove nginx site";
        details = "Removes proof nginx site configuration.";
      };
      "site-list" = {
        runtimeOp = "site-list";
        summary = "List nginx sites";
        details = "Lists configured proof nginx sites.";
      };
      "site-enable" = {
        runtimeOp = "site-enable";
        summary = "Enable nginx site";
        details = "Enables an existing proof nginx site.";
      };
      "site-disable" = {
        runtimeOp = "site-disable";
        summary = "Disable nginx site";
        details = "Disables an existing proof nginx site.";
      };
      "cert-obtain" = {
        runtimeOp = "cert-obtain";
        summary = "Obtain SSL certificate";
        details = "Creates a proof certificate marker for a domain.";
      };
      "cert-renew" = {
        runtimeOp = "cert-renew";
        summary = "Renew SSL certificates";
        details = "Refreshes proof certificate markers.";
      };
      "cert-status" = {
        runtimeOp = "cert-status";
        summary = "Show SSL certificate status";
        details = "Prints proof certificate status for configured domains.";
      };
    };
  };

  minioService = mkCommonService {
    serviceName = "minio";
    displayName = "MinIO";
    endpoints = {
      api = {
        protocol = "http";
        portKey = "minioApi";
      };
      console = {
        protocol = "http";
        portKey = "minioConsole";
      };
    };
    requireClientPackage = true;
    extraConfig = {
      rootUser = "minioadmin";
      rootPassword = "minioadmin";
      browser = true;
    };
    artifacts = {
      apiPortVar = "MINIOAPI_PORT";
      consolePortVar = "MINIOCONSOLE_PORT";
    };
    extraOps = {
      "export-s3-env" = {
        runtimeOp = "export-s3-env";
        summary = "Export S3-compatible environment from MinIO";
        details = "Prints shell exports for AWS/S3 variables targeting the current proof MinIO endpoint.";
      };
      "bucket-ensure" = {
        runtimeOp = "bucket-ensure";
        summary = "Ensure MinIO bucket exists";
        details = "Creates a proof bucket when missing and succeeds when already present.";
      };
      "bucket-create" = {
        runtimeOp = "bucket-create";
        summary = "Create MinIO bucket";
        details = "Creates a bucket in the running proof MinIO instance.";
      };
      "bucket-delete" = {
        runtimeOp = "bucket-delete";
        summary = "Delete MinIO bucket";
        details = "Deletes a bucket from the running proof MinIO instance.";
      };
      "bucket-list" = {
        runtimeOp = "bucket-list";
        summary = "List MinIO buckets";
        details = "Lists buckets from the running proof MinIO instance.";
      };
      "policy-apply" = {
        runtimeOp = "policy-apply";
        summary = "Apply MinIO bucket policy";
        details = "Applies a JSON policy file to a proof MinIO bucket.";
      };
    };
  };

  rethService = mkCommonService {
    serviceName = "reth";
    displayName = "Reth";
    endpoints = {
      http = {
        protocol = "http";
        portKey = "rethHttp";
      };
      ws = {
        protocol = "http";
        portKey = "rethWs";
      };
      auth = {
        protocol = "http";
        portKey = "rethAuth";
      };
    };
    extraConfig = {
      network = "local";
      devMode = true;
    };
    artifacts = {
      httpPortVar = "RETHHTTP_PORT";
      wsPortVar = "RETHWS_PORT";
      authPortVar = "RETHAUTH_PORT";
      network = "local";
      devMode = true;
    };
  };

  heliosService = mkCommonService {
    serviceName = "helios";
    displayName = "Helios";
    endpoints = {
      rpc = {
        protocol = "http";
        portKey = "heliosRpc";
      };
      execution = {
        protocol = "http";
        portKey = "rethHttp";
      };
    };
    sourceKinds.shim = "shim";
    extraConfig = {
      network = "local";
      executionRpcUrl = "";
      consensusRpcUrl = "";
      checkpoint = "";
      readiness = {
        profile = "fast";
        requireNotSyncing = false;
        disallowSourceKinds = [ ];
      };
    };
    artifacts = {
      rpcPortVar = "HELIOSRPC_PORT";
      executionPortVar = "RETHHTTP_PORT";
      network = "local";
    };
  };
in
{
  config = {
    nixfied.services =
      (conf.services or { })
      // {
        postgres = postgresService;
        nginx = nginxService;
        minio = minioService;
        reth = rethService;
        helios = heliosService;
      };
  };
}
