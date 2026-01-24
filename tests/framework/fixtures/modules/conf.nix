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
  };

  packages = { };
}
