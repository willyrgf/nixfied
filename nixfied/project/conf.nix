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
  };

  # Base data directory for per-slot/per-env state
  directories = {
    base = "\${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}";
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
    excludePatterns = [
      ".git"
      "node_modules"
      ".next"
      "dist"
      ".turbo"
      ".cache"
      "*.log"
      "test-results"
      "coverage"
    ];
    extraDirs = [ ];
  };

  modules = {
    postgres = {
      enable = false;
      database = "app";
      testDatabase = "app_test";
      extensions = [ ];
      package = if pkgs != null then pkgs.postgresql_16 else null;
      portKey = "postgres";
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
    };
    nginx = {
      enable = false;
      portKeyHttp = "http";
      portKeyHttps = "https";
      dataDirName = "nginx";
    };
    minio = {
      enable = false;
      package = if pkgs != null then pkgs.minio else null;
      clientPackage = if pkgs != null then pkgs.minio-client else null;
      portKeyApi = "minioApi";
      portKeyConsole = "minioConsole";
      dataDirName = "minio";
      rootUser = "minioadmin";
      rootPassword = "minioadmin";
      browser = true;
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
    useDeps = false;
    runApp = "ci";
    runArgs = [ "--summary" ];
    runEnv = { };
    preInstall = "";
    runCommand = "nix run path:.#ci -- --summary";
    validateCommand = "";
    cleanup = "";
  };

  services = {
    names = [ ];
    sockets = { };
  };

  packages = { };
}
