# Base project configuration
{
  pkgs ? null,
}:

rec {
  project = {
    name = "Nixfied Project";
    id = "nixfied-project";
    description = "Reusable Nix development framework";
    envVar = "PROJECT_ENV";
    slotVar = "NIX_ENV";
  };

  # Environment definitions and port offsets
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

  # Slot behavior for port calculations
  slots = {
    max = 9;
    stride = 1;
    default = 0;
  };

  # Port roles (keys become <KEY>_PORT in slot scripts)
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

  # Base runtime directory. project/module.nix resolves the final default to a
  # workspace-scoped path for the current checkout.
  directories = {
    base = "/tmp/nixfied-runtime/${project.id}/runtime";
  };

  logging = {
    level = "info";
    output = "stdout";
  };

  tooling = {
    runtimePackages = [
      pkgs.coreutils
      pkgs.gnused
    ];
    devShellPackages = [ ];
    devShellHook = ''
      echo "Nix framework dev shell ready."
    '';
    envFile = {
      enable = true;
      strict = false;
      allow = [ ];
    };
  };

  discovery = {
    enable = true;
    strict = true;
    refreshArg = "--refresh-discovery";
    requiredDocs = [
      "README.md"
      "AGENTS.md"
      "CLAUDE.md"
      "docs/ARCHITECTURE.md"
      "REDESIGN.md"
    ];
  };

  install = {
    deps = "";
  };

  supervisor = {
    enable = true;
    services = { };
  };

  ephemeral = {
    enable = false;
    # Copy strategy for ephemeral source setup:
    # - git-files: tracked + non-ignored files via git ls-files.
    # - static-excludes: rsync with excludePatterns.
    copyMode = "git-files";
    excludePatterns = [
      ".git"
      "node_modules"
      ".next"
      "dist"
      ".turbo"
      ".cache"
      "result"
      "result-*"
      "*.log"
      "test-results"
      "coverage"
    ];
    extraDirs = [ ];
    # Preserve failed roots for debugging and prune deterministically.
    keepFailures = true;
    maxFailedRoots = 8;
    maxFailedRootAgeHours = 72;
    # Pre-copy disk guards; set > 0 to enforce.
    maxCopyBytes = 0;
    minFreeBytesAfterCopy = 0;
  };

  process = {
    # project/module.nix resolves the final default to a workspace-scoped path
    # for the current checkout.
    registryRoot = "/tmp/nixfied-runtime/${project.id}/registry";
  };

  services = {
    postgres = {
      enable = false;
      ports = {
        primary = "postgres";
      };
      database = "app";
      testDatabase = "app_test";
      extensions = [ ];
      dataDirName = "postgres";
      extraConfig = "";
      envConfigs = {
        dev = { };
        prod = { };
        test = { };
      };
      migrations = {
        dir = "migrations";
        command = "";
        sourceDatabase = null;
      };
      sources =
        if pkgs != null then
          {
            nixpkgs.package = pkgs.postgresql_16;
          }
        else
          { };
      defaultSource = if pkgs != null then "nixpkgs" else "";
    };

    nginx = {
      enable = false;
      ports = {
        http = "http";
        https = "https";
      };
      dataDirName = "nginx";
      sources =
        if pkgs != null && pkgs ? nginx then
          {
            nixpkgs.package = pkgs.nginx;
          }
        else
          { };
      defaultSource = if pkgs != null && pkgs ? nginx then "nixpkgs" else "";
    };

    minio = {
      enable = false;
      ports = {
        api = "minioApi";
        console = "minioConsole";
      };
      dataDirName = "minio";
      rootUser = "minioadmin";
      rootPassword = "minioadmin";
      browser = true;
      sources =
        if pkgs != null then
          {
            nixpkgs = {
              package = pkgs.minio;
              clientPackage = pkgs.minio-client;
            };
          }
        else
          { };
      defaultSource = if pkgs != null then "nixpkgs" else "";
    };

    reth = {
      enable = false;
      ports = {
        http = "rethHttp";
        ws = "rethWs";
        auth = "rethAuth";
      };
      dataDirName = "reth";
      network = "local";
      devMode = true;
      extraArgs = [ ];
      sources =
        if pkgs != null && pkgs ? reth then
          {
            nixpkgs.package = pkgs.reth;
          }
        else
          { };
      defaultSource = if pkgs != null && pkgs ? reth then "nixpkgs" else "";
    };

    helios = {
      enable = false;
      ports = {
        rpc = "heliosRpc";
        executionRpc = "rethHttp";
      };
      dataDirName = "helios";
      network = "local";
      executionRpcUrl = "";
      consensusRpcUrl = "";
      checkpoint = "";
      extraArgs = [ ];
      sources =
        if pkgs != null then
          {
            pinned.package = pkgs.callPackage ./sources/helios-pinned.nix { };
          }
          // (
            if pkgs ? helios then
              {
                nixpkgs.package = pkgs.helios;
              }
            else
              { }
          )
        else
          { };
      sourceKinds =
        if pkgs != null then
          {
            pinned = "real";
          }
          // (
            if pkgs ? helios then
              {
                nixpkgs = "real";
              }
            else
              { }
          )
        else
          { };
      defaultSource = if pkgs != null then "pinned" else "";
      readiness = {
        profile = "fast";
        requireNotSyncing = false;
        disallowSourceKinds = [ ];
      };
    };
  };

  # Isolation test runner configuration (nix run .#test-isolation)
  isolation = {
    enable = true;
    slots = [
      5
      7
      8
      9
    ];
    envs = [ ];
    validationInterval = 10;
    maxRuntime = 300;
    startupWait = 30;
    logsDir = "/tmp/${project.id}-isolation";
    keepLogsOnSuccess = false;
    keepLogsOnFailure = true;
    maxParallel = 12;
    useDeps = false;
    run = {
      kind = "runApp";
      app = "ci";
      args = [ "--summary" ];
    };
    validate = {
      kind = "runApp";
      app = "validate-env";
    };
    runEnv = { };
    setupActions = [ ];
    cleanupActions = [ ];
  };

  # Framework test runner configuration (nix run .#framework::test)
  frameworkTest = {
    # Default shard parallelism used when --max-parallel-shards is not passed.
    # Supported values: "auto" or a positive integer.
    maxParallelShards = "auto";
  };

  packages = { };
}
