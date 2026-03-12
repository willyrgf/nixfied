{
  conf,
  normalizeSourceKeys,
  normalizePostgresEnvConfigs,
}:
{
  config = {
    nixfied.services = {
      postgres = {
        enable = conf.services.postgres.enable or false;
        database = conf.services.postgres.database or "app";
        testDatabase = conf.services.postgres.testDatabase or "app_test";
        portKey = conf.services.postgres.ports.primary or "postgres";
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
        sourceKeys = normalizeSourceKeys (conf.services.postgres.sources or { });
        defaultSource = conf.services.postgres.defaultSource or "";
      };

      nginx = {
        enable = conf.services.nginx.enable or false;
        portKeyHttp = conf.services.nginx.ports.http or "http";
        portKeyHttps = conf.services.nginx.ports.https or "https";
        dataDirName = conf.services.nginx.dataDirName or "nginx";
        sources = conf.services.nginx.sources or { };
        sourceKeys = normalizeSourceKeys (conf.services.nginx.sources or { });
        defaultSource = conf.services.nginx.defaultSource or "";
      };

      minio = {
        enable = conf.services.minio.enable or false;
        portKeyApi = conf.services.minio.ports.api or "minioApi";
        portKeyConsole = conf.services.minio.ports.console or "minioConsole";
        dataDirName = conf.services.minio.dataDirName or "minio";
        rootUser = conf.services.minio.rootUser or "minioadmin";
        rootPassword = conf.services.minio.rootPassword or "minioadmin";
        browser = conf.services.minio.browser or true;
        sources = conf.services.minio.sources or { };
        sourceKeys = normalizeSourceKeys (conf.services.minio.sources or { });
        defaultSource = conf.services.minio.defaultSource or "";
      };

      reth = {
        enable = conf.services.reth.enable or false;
        portKeyHttp = conf.services.reth.ports.http or "rethHttp";
        portKeyWs = conf.services.reth.ports.ws or "rethWs";
        portKeyAuth = conf.services.reth.ports.auth or "rethAuth";
        dataDirName = conf.services.reth.dataDirName or "reth";
        network = conf.services.reth.network or "local";
        devMode = conf.services.reth.devMode or false;
        extraArgs = conf.services.reth.extraArgs or [ ];
        sources = conf.services.reth.sources or { };
        sourceKeys = normalizeSourceKeys (conf.services.reth.sources or { });
        defaultSource = conf.services.reth.defaultSource or "";
      };

      helios = {
        enable = conf.services.helios.enable or false;
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
        sourceKeys = normalizeSourceKeys (conf.services.helios.sources or { });
        defaultSource = conf.services.helios.defaultSource or "";
        sourceKinds = conf.services.helios.sourceKinds or { };
        readiness = {
          profile = conf.services.helios.readiness.profile or "fast";
          requireNotSyncing = conf.services.helios.readiness.requireNotSyncing or false;
          disallowSourceKinds = conf.services.helios.readiness.disallowSourceKinds or [ ];
        };
      };
    };
  };
}
