# Test fixture: enable optional modules
{
  pkgs ? null,
}:

rec {
  project = {
    name = "Nixfied Test Project";
    id = "nixfied-test-framework";
    description = "Framework test fixture";
    envVar = "PROJECT_ENV";
    slotVar = "NIX_ENV";
  };

  envs = {
    prod = {
      offset = 0;
    };
    dev = {
      offset = 10;
    };
    test = {
      offset = 20;
    };
  };

  ports = {
    backend = 3000;
    frontend = 3100;
    http = 8080;
    https = 8443;
    postgres = 5432;
    minioApi = 9000;
    minioConsole = 9001;
    rethHttp = 8545;
    rethWs = 8546;
    rethAuth = 8551;
    heliosRpc = 8547;
  };

  directories = {
    base = "\${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}";
  };

  tooling = {
    runtimePackages = [
      pkgs.coreutils
      pkgs.gnused
      pkgs.gnugrep
      pkgs.netcat
      pkgs.python3
    ];
    devShellPackages = [ ];
    devShellHook = "";
  };

  install = {
    deps = "";
  };

  supervisor = {
    enable = true;
    services = { };
  };

  modules = {
    postgres = {
      enable = true;
      database = "app";
      testDatabase = "app_test";
      extensions = [ ];
      package = if pkgs != null then pkgs.postgresql_16 else null;
      portKey = "postgres";
      dataDirName = "postgres";
      extraConfig = "";
    };
    nginx = {
      enable = true;
      portKeyHttp = "http";
      portKeyHttps = "https";
      dataDirName = "nginx";
    };
    minio = {
      enable = true;
      package = if pkgs != null then pkgs.minio else null;
      clientPackage = if pkgs != null then pkgs.minio-client else null;
      portKeyApi = "minioApi";
      portKeyConsole = "minioConsole";
      dataDirName = "minio";
      rootUser = "minioadmin";
      rootPassword = "minioadmin";
      browser = true;
    };
    reth = {
      enable = true;
      portKeyHttp = "rethHttp";
      portKeyWs = "rethWs";
      portKeyAuth = "rethAuth";
      dataDirName = "reth";
      network = "local";
      devMode = true;
      # Use an ephemeral p2p listener in test fixtures to avoid collisions with local nodes.
      extraArgs = [
        "--port"
        "0"
      ];
    };
    helios = {
      enable = true;
      portKeyRpc = "heliosRpc";
      dataDirName = "helios";
      network = "local";
      executionRpcPortKey = "rethHttp";
      executionRpcUrl = "";
      consensusRpcUrl = "";
      checkpoint = "";
      extraArgs = [ ];
    };
  };

  packages = { };
}
