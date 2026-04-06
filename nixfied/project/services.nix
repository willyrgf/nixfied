{
  pkgs,
  conf,
  normalizePostgresEnvConfigs,
}:
{
  lib,
  config,
  ...
}:
let
  contractSchema = import ../modules/services/contract-schema.nix { inherit lib; };
  runtimeDefaults = import ../framework/core/runtime-defaults.nix;

  servicesCfg = config.nixfied.services;
  postgresPkg = pkgs.postgresql_16 or pkgs.postgresql;
  localhost = runtimeDefaults.hosts.loopbackIp;

  mkPortVar = contractSchema.mkPortVarName;
  mkServiceDir = serviceName: contractSchema.mkServiceDirExpr servicesCfg.${serviceName}.dataDirName;

  mkCommonArtifacts =
    serviceName:
    let
      serviceDir = mkServiceDir serviceName;
    in
    {
      inherit serviceDir;
      dataDir = serviceDir;
    };

  mkInitOp = displayName: {
    runtimeOp = "init";
    summary = "Initialize ${displayName}";
    details = "Initializes ${displayName} runtime state for the current slot/environment.";
  };

  mkRestartOp = displayName: {
    runtimeOp = "restart";
    summary = "Restart ${displayName}";
    details = "Restarts ${displayName} for the current slot/environment.";
  };

  mkCheckConfigOp = displayName: {
    runtimeOp = "check-config";
    summary = "Validate ${displayName} configuration";
    details = "Validates ${displayName} configuration for the current slot/environment.";
  };

  mkFullStartOp =
    {
      runtimeOp,
      summary,
      details,
      ...
    }:
    {
      inherit
        runtimeOp
        summary
        details
        ;
    };

  mkFixtureRef =
    {
      argumentFields ? [ ],
      defaults ? { },
      description ? "",
      script,
    }:
    {
      inherit
        argumentFields
        defaults
        description
        script
        ;
    };

  mkFixtureInvocation =
    {
      operation,
      argumentFields ? [ ],
      defaults ? { },
      description ? "",
    }:
    {
      inherit
        operation
        argumentFields
        defaults
        description
        ;
    };

  heliosSourceKindCase = builtins.concatStringsSep "\n" (
    lib.mapAttrsToList (sourceName: sourceKind: ''
      ${lib.escapeShellArg sourceName})
        source_kind=${lib.escapeShellArg sourceKind}
        ;;
    '') (servicesCfg.helios.sourceKinds or { })
  );

  heliosDisallowedKinds = lib.unique (
    (servicesCfg.helios.readiness.disallowSourceKinds or [ ])
    ++ lib.optionals ((servicesCfg.helios.readiness.profile or "fast") == "strict") [
      "shim"
      "unknown"
    ]
  );

  heliosDisallowedKindCase = builtins.concatStringsSep "\n" (
    map (kind: ''
      ${lib.escapeShellArg kind})
        echo "${servicesCfg.helios.displayName} not ready port=$NIXFIED_PROBE_RPC_PORT source=$NIXFIED_PROBE_SOURCE source_kind=$source_kind profile=$profile (source kind disallowed)"
        exit 1
        ;;
    '') heliosDisallowedKinds
  );
in
{
  config = {
    nixfied.services.postgres = {
      enable = conf.services.postgres.enable or false;
      displayName = "PostgreSQL";
      summary = "PostgreSQL service management API";
      details = "Public service contract for managing PostgreSQL across dev/prod/test/ci.";
      ownerFile = "nixfied/project/services.nix";

      portKey = conf.services.postgres.ports.primary or "postgres";
      database = conf.services.postgres.database or "app";
      testDatabase = conf.services.postgres.testDatabase or "app_test";
      dataDirName = conf.services.postgres.dataDirName or "postgres";
      extensions = conf.services.postgres.extensions or [ ];
      extraConfig = conf.services.postgres.extraConfig or "";
      envConfigs = normalizePostgresEnvConfigs (conf.services.postgres.envConfigs or { });
      migrations = {
        dir = conf.services.postgres.migrations.dir or "migrations";
        command = conf.services.postgres.migrations.command or "";
        sourceDatabase = conf.services.postgres.migrations.sourceDatabase or null;
      };
      sources = conf.services.postgres.sources or { };
      defaultSource = conf.services.postgres.defaultSource or "";
      requiredSourceArtifacts = [ "package" ];

      endpoints.primary = {
        protocol = "postgres";
        inherit (servicesCfg.postgres) portKey;
      };

      artifacts =
        let
          inherit (servicesCfg.postgres)
            portKey
            testDatabase
            ;
          defaultDatabase = servicesCfg.postgres.database;
        in
        mkCommonArtifacts "postgres" // {
          inherit portKey;
          portVar = mkPortVar portKey;
          logFile = "${mkServiceDir "postgres"}/postgres.log";
          pidFile = "${mkServiceDir "postgres"}/postmaster.pid";
          inherit defaultDatabase testDatabase;
        };

      lifecycle = {
        preStart = {
          summary = "Prepare PostgreSQL startup";
          details = "Runs PostgreSQL initialization and deterministic configuration validation before startup.";
        };
        start = {
          summary = "Start PostgreSQL server";
          details = "Starts PostgreSQL for the current slot/environment.";
        };
        status = {
          summary = "Show PostgreSQL status";
          details = "Prints PostgreSQL status for the current slot/environment.";
        };
        preStop = {
          summary = "Prepare PostgreSQL shutdown";
          details = "Runs deterministic shutdown preparation for PostgreSQL.";
        };
        stop = {
          summary = "Stop PostgreSQL server";
          details = "Stops PostgreSQL for the current slot/environment.";
        };
      };

      checks = {
        health.steps = lib.mkDefault [
          {
            kind = "exec";
            command = ''
              ${postgresPkg}/bin/pg_isready -U postgres -h ${localhost} -p "$NIXFIED_PROBE_PRIMARY_PORT" -q
            '';
          }
        ];
        ready.steps = lib.mkDefault [
          {
            kind = "exec";
            command = ''
              ${postgresPkg}/bin/pg_isready -U postgres -h ${localhost} -p "$NIXFIED_PROBE_PRIMARY_PORT" -q &&
              ${postgresPkg}/bin/psql -h ${localhost} -p "$NIXFIED_PROBE_PRIMARY_PORT" -U postgres -d ${lib.escapeShellArg servicesCfg.postgres.database} -Atqc 'select 1;' >/dev/null
            '';
          }
        ];
      };

      extraOps = {
        init = mkInitOp "PostgreSQL";
        restart = mkRestartOp "PostgreSQL";
        "check-config" = mkCheckConfigOp "PostgreSQL";

        "preflight-init" = {
          runtimeOp = "preflight-init";
          summary = "Validate PostgreSQL init preconditions";
          details = "Checks deterministic blockers before PostgreSQL initialization for the current slot/environment.";
          exposeApp = false;
        };

        "setup-db" = {
          runtimeOp = "setup-db";
          summary = "Create and configure PostgreSQL database";
          details = "Creates the configured database and required extensions.";
        };

        "full-start" = mkFullStartOp {
          runtimeOp = "full-start";
          displayName = "PostgreSQL";
          summary = "Init, start, and set up PostgreSQL";
          details = "Performs init/start/setup-db in one operation.";
        };

        "full-start-test" = mkFullStartOp {
          runtimeOp = "full-start-test";
          displayName = "PostgreSQL";
          summary = "Init/start/setup PostgreSQL test database";
          details = "Performs init/start/setup-db using the configured test database.";
        };

        "ready-test" = {
          runtimeOp = "ready-test";
          summary = "Wait for PostgreSQL test-database readiness";
          details = "Checks PostgreSQL and the configured test database accept local SQL queries.";
        };

        "list-instances" = {
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

        "list-backups" = {
          runtimeOp = "list-backups";
          summary = "List PostgreSQL backups";
          details = "Lists backups for the current slot and environment.";
        };

        "verify-backup" = {
          runtimeOp = "verify-backup";
          summary = "Verify PostgreSQL backup";
          details = "Verifies backup archive integrity.";
          usage = [ "nix run .#svc::postgres::verify-backup -- <backup-path>" ];
        };

        "cleanup-backups" = {
          runtimeOp = "cleanup-backups";
          summary = "Prune old PostgreSQL backups";
          details = "Removes old backups while keeping the requested number of newest snapshots.";
          usage = [ "nix run .#svc::postgres::cleanup-backups -- <keep-count>" ];
        };

        "test-migrations" = {
          runtimeOp = "test-migrations";
          summary = "Test PostgreSQL migrations";
          details = "Runs migrations against a temporary copy of the source database.";
        };

        "ensure-migration-tested" = {
          runtimeOp = "ensure-migration-tested";
          summary = "Ensure migrations were tested";
          details = "Fails when migration hashes were not previously tested.";
          exposeApp = false;
        };

        "check-port" = {
          runtimeOp = "check-port";
          summary = "Check PostgreSQL port usage";
          details = "Checks whether a port is already in use.";
          usage = [ "nix run .#svc::postgres::check-port -- <port>" ];
        };

        "kill-port" = {
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

      fixture.refs.url = mkFixtureRef {
        argumentFields = [ "database" ];
        defaults.database = servicesCfg.postgres.testDatabase;
        description = "Resolve a local PostgreSQL connection URL for the active slot/environment.";
        script = ''
          port_var=${lib.escapeShellArg (mkPortVar servicesCfg.postgres.portKey)}
          port_value="''${!port_var:?}"
          printf 'postgresql://postgres:postgres@${localhost}:%s/%s' "$port_value" "$database"
        '';
      };

      implementation = {
        version = 1;
        module = ../modules/services/runtime/postgres/default.nix;
      };
    };

    nixfied.services.nginx = {
      enable = conf.services.nginx.enable or false;
      displayName = "nginx";
      summary = "nginx service management API";
      details = "Public service contract for managing nginx across dev/prod/test/ci.";
      ownerFile = "nixfied/project/services.nix";

      portKeyHttp = conf.services.nginx.ports.http or "http";
      portKeyHttps = conf.services.nginx.ports.https or "https";
      dataDirName = conf.services.nginx.dataDirName or "nginx";
      sources = conf.services.nginx.sources or { };
      defaultSource = conf.services.nginx.defaultSource or "";
      requiredSourceArtifacts = [ "package" ];

      endpoints = {
        http = {
          protocol = "http";
          portKey = servicesCfg.nginx.portKeyHttp;
        };
        https = {
          protocol = "https";
          portKey = servicesCfg.nginx.portKeyHttps;
        };
      };

      artifacts = mkCommonArtifacts "nginx" // {
        httpPortVar = mkPortVar servicesCfg.nginx.portKeyHttp;
        httpsPortVar = mkPortVar servicesCfg.nginx.portKeyHttps;
        logFile = "${mkServiceDir "nginx"}/logs/error.log";
        pidFile = "${mkServiceDir "nginx"}/run/nginx.pid";
      };

      checks = {
        health.steps = lib.mkDefault [
          {
            kind = "tcp";
            endpoint = "http";
          }
          {
            kind = "tcp";
            endpoint = "https";
          }
        ];
        ready.steps = lib.mkDefault [
          {
            kind = "tcp";
            endpoint = "http";
          }
          {
            kind = "tcp";
            endpoint = "https";
          }
        ];
      };

      extraOps = {
        init = mkInitOp "nginx";
        restart = mkRestartOp "nginx";
        "check-config" = mkCheckConfigOp "nginx";

        reload = {
          runtimeOp = "reload";
          summary = "Reload nginx configuration";
          details = "Tests and reloads nginx configuration.";
        };

        "full-start" = mkFullStartOp {
          runtimeOp = "full-start";
          displayName = "nginx";
          summary = "Init/check/start nginx";
          details = "Performs init, configuration validation, and startup for nginx.";
        };

        "full-start-test" = mkFullStartOp {
          runtimeOp = "full-start-test";
          displayName = "nginx";
          summary = "Init/check/start nginx for test profile";
          details = "Performs init, configuration validation, and startup for nginx using the test profile.";
        };

        "list-instances" = {
          runtimeOp = "list-instances";
          summary = "List nginx instances";
          details = "Lists nginx instances managed by Nixfied.";
        };

        "site-proxy" = {
          runtimeOp = "site-proxy";
          summary = "Write proxy site configuration";
          details = "Writes a proxy site config and enables it.";
          exposeApp = false;
          usage = [ "nix run .#svc::nginx::site-proxy -- <domain> <upstream-host> <upstream-port>" ];
        };

        "site-static" = {
          runtimeOp = "site-static";
          summary = "Write static site configuration";
          details = "Writes a static site config and enables it.";
          exposeApp = false;
          usage = [ "nix run .#svc::nginx::site-static -- <domain> <site-root>" ];
        };

        "site-add" = {
          runtimeOp = "site-add";
          summary = "Add proxy nginx site";
          details = "Adds a proxy site and enables it.";
          usage = [ "nix run .#svc::nginx::site-add -- <domain> <upstream-host> <upstream-port>" ];
        };

        "site-remove" = {
          runtimeOp = "site-remove";
          summary = "Remove nginx site";
          details = "Removes nginx site configuration.";
          usage = [ "nix run .#svc::nginx::site-remove -- <domain>" ];
        };

        "site-list" = {
          runtimeOp = "site-list";
          summary = "List nginx sites";
          details = "Lists configured nginx sites.";
        };

        "site-enable" = {
          runtimeOp = "site-enable";
          summary = "Enable nginx site";
          details = "Enables an existing nginx site.";
          usage = [ "nix run .#svc::nginx::site-enable -- <domain>" ];
        };

        "site-disable" = {
          runtimeOp = "site-disable";
          summary = "Disable nginx site";
          details = "Disables an existing nginx site.";
          usage = [ "nix run .#svc::nginx::site-disable -- <domain>" ];
        };

        "cert-obtain" = {
          runtimeOp = "cert-obtain";
          summary = "Obtain SSL certificate";
          details = "Obtains a Let's Encrypt certificate for a domain.";
          usage = [ "nix run .#svc::nginx::cert-obtain -- <domain> <email> [--staging]" ];
        };

        "cert-renew" = {
          runtimeOp = "cert-renew";
          summary = "Renew SSL certificates";
          details = "Renews certificates for configured sites.";
        };

        "cert-status" = {
          runtimeOp = "cert-status";
          summary = "Show SSL certificate status";
          details = "Prints certificate status for configured domains.";
        };
      };

      implementation = {
        version = 1;
        module = ../modules/services/runtime/nginx/default.nix;
      };
    };

    nixfied.services.minio = {
      enable = conf.services.minio.enable or false;
      displayName = "MinIO";
      summary = "MinIO service management API";
      details = "Public service contract for managing MinIO across dev/prod/test/ci.";
      ownerFile = "nixfied/project/services.nix";
      profiles = [
        "dev"
        "prod"
        "test"
        "ci"
      ];

      portKeyApi = conf.services.minio.ports.api or "minioApi";
      portKeyConsole = conf.services.minio.ports.console or "minioConsole";
      dataDirName = conf.services.minio.dataDirName or "minio";
      rootUser = conf.services.minio.rootUser or "minioadmin";
      rootPassword = conf.services.minio.rootPassword or "minioadmin";
      browser = conf.services.minio.browser or true;
      sources = conf.services.minio.sources or { };
      defaultSource = conf.services.minio.defaultSource or "";
      requiredSourceArtifacts = [
        "package"
        "clientPackage"
      ];

      endpoints = {
        api = {
          protocol = "http";
          portKey = servicesCfg.minio.portKeyApi;
        };
        console = {
          protocol = "http";
          portKey = servicesCfg.minio.portKeyConsole;
        };
      };

      artifacts = mkCommonArtifacts "minio" // {
        apiPortVar = mkPortVar servicesCfg.minio.portKeyApi;
        consolePortVar = mkPortVar servicesCfg.minio.portKeyConsole;
        logFile = "${mkServiceDir "minio"}/logs/minio.log";
        pidFile = "${mkServiceDir "minio"}/run/minio.pid";
      };

      checks = {
        health.steps = lib.mkDefault [
          {
            kind = "tcp";
            endpoint = "api";
          }
          {
            kind = "tcp";
            endpoint = "console";
          }
        ];
        ready.steps = lib.mkDefault [
          {
            kind = "tcp";
            endpoint = "api";
          }
          {
            kind = "tcp";
            endpoint = "console";
          }
        ];
      };

      extraOps = {
        init = mkInitOp "MinIO";
        restart = mkRestartOp "MinIO";
        "check-config" = mkCheckConfigOp "MinIO";

        "full-start" = mkFullStartOp {
          runtimeOp = "full-start";
          displayName = "MinIO";
          summary = "Init/check/start MinIO";
          details = "Performs init, configuration validation, and startup for MinIO.";
        };

        "full-start-test" = mkFullStartOp {
          runtimeOp = "full-start-test";
          displayName = "MinIO";
          summary = "Init/check/start MinIO for test profile";
          details = "Performs init, configuration validation, and startup for MinIO using the test profile.";
        };

        "export-s3-env" = {
          runtimeOp = "export-s3-env";
          summary = "Export S3-compatible environment from MinIO";
          details = "Prints shell exports for AWS/S3 variables targeting the current MinIO endpoint.";
          usage = [ "eval \"$(nix run .#svc::minio::export-s3-env -- <bucket> <prefix> <region>)\"" ];
        };

        "bucket-ensure" = {
          runtimeOp = "bucket-ensure";
          summary = "Ensure MinIO bucket exists";
          details = "Creates bucket when missing and succeeds when already present.";
          usage = [ "nix run .#svc::minio::bucket-ensure -- <bucket>" ];
        };

        "bucket-create" = {
          runtimeOp = "bucket-create";
          summary = "Create MinIO bucket";
          details = "Creates a bucket in the running MinIO instance.";
          usage = [ "nix run .#svc::minio::bucket-create -- <bucket>" ];
        };

        "bucket-delete" = {
          runtimeOp = "bucket-delete";
          summary = "Delete MinIO bucket";
          details = "Deletes a bucket from the running MinIO instance.";
          usage = [ "nix run .#svc::minio::bucket-delete -- <bucket>" ];
        };

        "bucket-list" = {
          runtimeOp = "bucket-list";
          summary = "List MinIO buckets";
          details = "Lists buckets from the running MinIO instance.";
        };

        "policy-apply" = {
          runtimeOp = "policy-apply";
          summary = "Apply MinIO bucket policy";
          details = "Applies a JSON policy file to a MinIO bucket.";
          usage = [ "nix run .#svc::minio::policy-apply -- <bucket> <policy-file>" ];
        };
      };

      fixture = {
        refs = {
          endpoint = mkFixtureRef {
            description = "Resolve the MinIO API endpoint URL for the active slot/environment.";
            script = ''
              port_var=${lib.escapeShellArg (mkPortVar servicesCfg.minio.portKeyApi)}
              port_value="''${!port_var:?}"
              printf 'http://${localhost}:%s' "$port_value"
            '';
          };
          bucket = mkFixtureRef {
            description = "Read the current MinIO fixture bucket token.";
            script = ''
              printf '%s' "''${MINIO_BUCKET:-}"
            '';
          };
          region = mkFixtureRef {
            description = "Read the current MinIO fixture region token.";
            script = ''
              printf '%s' "''${MINIO_REGION:-us-east-1}"
            '';
          };
          prefix = mkFixtureRef {
            description = "Read the current MinIO fixture prefix token.";
            script = ''
              printf '%s' "''${MINIO_PREFIX:-}"
            '';
          };
        };

        exports.s3 = mkFixtureInvocation {
          operation = "export-s3-env";
          argumentFields = [
            "bucket"
            "prefix"
            "region"
          ];
          defaults.region = "us-east-1";
          description = "Materialize AWS/S3 environment variables from the running MinIO service.";
        };

        bootstrap.bucket = mkFixtureInvocation {
          operation = "bucket-ensure";
          argumentFields = [ "name" ];
          description = "Ensure a MinIO bucket exists for fixture setup.";
        };
      };

      implementation = {
        version = 1;
        module = ../modules/services/runtime/minio/default.nix;
      };
    };

    nixfied.services.reth = {
      enable = conf.services.reth.enable or false;
      displayName = "Reth";
      summary = "Reth service management API";
      details = "Public service contract for managing Reth across dev/prod/test/ci.";
      ownerFile = "nixfied/project/services.nix";

      portKeyHttp = conf.services.reth.ports.http or "rethHttp";
      portKeyWs = conf.services.reth.ports.ws or "rethWs";
      portKeyAuth = conf.services.reth.ports.auth or "rethAuth";
      dataDirName = conf.services.reth.dataDirName or "reth";
      network = conf.services.reth.network or "local";
      devMode = conf.services.reth.devMode or false;
      extraArgs = conf.services.reth.extraArgs or [ ];
      sources = conf.services.reth.sources or { };
      defaultSource = conf.services.reth.defaultSource or "";
      requiredSourceArtifacts = [ "package" ];

      endpoints = {
        http = {
          protocol = "http";
          portKey = servicesCfg.reth.portKeyHttp;
        };
        ws = {
          protocol = "ws";
          portKey = servicesCfg.reth.portKeyWs;
        };
        auth = {
          protocol = "http";
          portKey = servicesCfg.reth.portKeyAuth;
        };
      };

      artifacts = mkCommonArtifacts "reth" // {
        httpPortVar = mkPortVar servicesCfg.reth.portKeyHttp;
        wsPortVar = mkPortVar servicesCfg.reth.portKeyWs;
        authPortVar = mkPortVar servicesCfg.reth.portKeyAuth;
        logFile = "${mkServiceDir "reth"}/logs/reth.log";
        pidFile = "${mkServiceDir "reth"}/run/reth.pid";
        inherit (servicesCfg.reth)
          network
          devMode
          ;
      };

      checks = {
        health.steps = lib.mkDefault [
          {
            kind = "jsonrpc";
            endpoint = "http";
            method = "web3_clientVersion";
          }
          {
            kind = "tcp";
            endpoint = "ws";
          }
          {
            kind = "tcp";
            endpoint = "auth";
          }
        ];
        ready.steps = lib.mkDefault [
          {
            kind = "jsonrpc";
            endpoint = "http";
            method = "eth_chainId";
          }
          {
            kind = "tcp";
            endpoint = "ws";
          }
          {
            kind = "tcp";
            endpoint = "auth";
          }
        ];
      };

      extraOps = {
        init = mkInitOp "Reth";
        restart = mkRestartOp "Reth";
        "check-config" = mkCheckConfigOp "Reth";

        "full-start" = mkFullStartOp {
          runtimeOp = "full-start";
          displayName = "Reth";
          summary = "Init/check/start Reth";
          details = "Performs init, configuration validation, and startup for Reth.";
        };

        "full-start-test" = mkFullStartOp {
          runtimeOp = "full-start-test";
          displayName = "Reth";
          summary = "Init/check/start Reth for test profile";
          details = "Performs init, configuration validation, and startup for Reth using the test profile.";
        };
      };

      fixture.refs.httpUrl = mkFixtureRef {
        description = "Resolve the Reth HTTP RPC URL for the active slot/environment.";
        script = ''
          port_var=${lib.escapeShellArg (mkPortVar servicesCfg.reth.portKeyHttp)}
          port_value="''${!port_var:?}"
          printf 'http://${localhost}:%s' "$port_value"
        '';
      };

      implementation = {
        version = 1;
        module = ../modules/services/runtime/reth/default.nix;
      };
    };

    nixfied.services.helios = {
      enable = conf.services.helios.enable or false;
      displayName = "Helios";
      summary = "Helios service management API";
      details = "Public service contract for managing Helios across dev/prod/test/ci.";
      ownerFile = "nixfied/project/services.nix";

      portKeyRpc = conf.services.helios.ports.rpc or "heliosRpc";
      executionRpcPortKey = conf.services.helios.ports.executionRpc or "rethHttp";
      dataDirName = conf.services.helios.dataDirName or "helios";
      network = conf.services.helios.network or "local";
      executionRpcUrl = conf.services.helios.executionRpcUrl or "";
      consensusRpcUrl = conf.services.helios.consensusRpcUrl or "";
      defaultConsensusRpcUrl =
        conf.services.helios.defaultConsensusRpcUrl or "https://www.lightclientdata.org";
      checkpoint = conf.services.helios.checkpoint or "";
      extraArgs = conf.services.helios.extraArgs or [ ];
      sources = conf.services.helios.sources or { };
      sourceKinds = conf.services.helios.sourceKinds or { };
      defaultSource = conf.services.helios.defaultSource or "";
      readiness = conf.services.helios.readiness or { };
      requiredSourceArtifacts = [ "package" ];

      endpoints = {
        rpc = {
          protocol = "http";
          portKey = servicesCfg.helios.portKeyRpc;
        };
        execution = {
          protocol = "http";
          portKey = servicesCfg.helios.executionRpcPortKey;
        };
      };

      artifacts = mkCommonArtifacts "helios" // {
        rpcPortVar = mkPortVar servicesCfg.helios.portKeyRpc;
        executionPortVar = mkPortVar servicesCfg.helios.executionRpcPortKey;
        logFile = "${mkServiceDir "helios"}/logs/helios.log";
        pidFile = "${mkServiceDir "helios"}/run/helios.pid";
        inherit (servicesCfg.helios) network;
      };

      checks = {
        health.steps = lib.mkDefault [
          {
            kind = "jsonrpc";
            endpoint = "rpc";
            method = "eth_chainId";
          }
          {
            kind = "jsonrpc";
            endpoint = "execution";
            method = "web3_clientVersion";
            label = "helios execution";
          }
        ];

        ready = {
          wait = {
            enabled = lib.mkDefault true;
            timeoutEnvVar = lib.mkDefault "HELIOS_READY_TIMEOUT_SECS";
            intervalEnvVar = lib.mkDefault "HELIOS_READY_INTERVAL_SECS";
          };
          steps = lib.mkDefault [
            {
              kind = "exec";
              command = ''
                source_kind=unknown
                case "$NIXFIED_PROBE_SOURCE" in
                ${heliosSourceKindCase}
                esac

                profile=${lib.escapeShellArg (servicesCfg.helios.readiness.profile or "fast")}

                case "$source_kind" in
                ${heliosDisallowedKindCase}
                esac

                rpc_url="http://${localhost}:$NIXFIED_PROBE_RPC_PORT"

                block_payload="$(${pkgs.curl}/bin/curl -fsS --max-time 2 \
                  -H 'content-type: application/json' \
                  --data '{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]}' \
                  "$rpc_url")"
                block_number="$(printf '%s' "$block_payload" | ${pkgs.python3}/bin/python3 -c 'import json,sys; payload=json.load(sys.stdin); result=payload.get("result"); print(result if isinstance(result, str) and result.startswith("0x") else "")')"

                if [ -z "$block_number" ]; then
                  echo "Helios not ready port=$NIXFIED_PROBE_RPC_PORT source=$NIXFIED_PROBE_SOURCE source_kind=$source_kind (invalid eth_blockNumber result)"
                  exit 1
                fi

                echo "OK: helios ready port=$NIXFIED_PROBE_RPC_PORT block_number=$block_number"

                if [ "${
                  if
                    (servicesCfg.helios.readiness.requireNotSyncing or false)
                    || ((servicesCfg.helios.readiness.profile or "fast") == "strict")
                  then
                    "1"
                  else
                    "0"
                }" = "1" ]; then
                  syncing_payload="$(${pkgs.curl}/bin/curl -fsS --max-time 2 \
                    -H 'content-type: application/json' \
                    --data '{"jsonrpc":"2.0","id":1,"method":"eth_syncing","params":[]}' \
                    "$rpc_url")"
                  syncing_result="$(printf '%s' "$syncing_payload" | ${pkgs.python3}/bin/python3 -c 'import json,sys; payload=json.load(sys.stdin); import json as _j; print(_j.dumps(payload.get("result"), separators=(",", ":")))')"
                  if [ "$syncing_result" != "false" ]; then
                    echo "Helios not ready port=$NIXFIED_PROBE_RPC_PORT source=$NIXFIED_PROBE_SOURCE source_kind=$source_kind profile=$profile (eth_syncing=$syncing_result)"
                    exit 1
                  fi
                  echo "OK: helios sync status ready port=$NIXFIED_PROBE_RPC_PORT"
                fi
              '';
            }
            {
              kind = "jsonrpc";
              endpoint = "execution";
              method = "eth_chainId";
              label = "helios execution";
            }
          ];
        };
      };

      extraOps = {
        init = mkInitOp "Helios";
        restart = mkRestartOp "Helios";
        "check-config" = mkCheckConfigOp "Helios";

        "full-start" = mkFullStartOp {
          runtimeOp = "full-start";
          displayName = "Helios";
          summary = "Init/check/start Helios";
          details = "Performs init, configuration validation, and startup for Helios.";
        };

        "full-start-test" = mkFullStartOp {
          runtimeOp = "full-start-test";
          displayName = "Helios";
          summary = "Init/check/start Helios for test profile";
          details = "Performs init, configuration validation, and startup for Helios using the test profile.";
        };
      };

      fixture.refs.rpcUrl = mkFixtureRef {
        description = "Resolve the Helios RPC URL for the active slot/environment.";
        script = ''
          port_var=${lib.escapeShellArg (mkPortVar servicesCfg.helios.portKeyRpc)}
          port_value="''${!port_var:?}"
          printf 'http://${localhost}:%s' "$port_value"
        '';
      };

      implementation = {
        version = 1;
        module = ../modules/services/runtime/helios/default.nix;
      };
    };
  };
}
