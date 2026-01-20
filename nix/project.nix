# Project-specific configuration for the Nix framework.
# Edit this file to adapt the framework to your project.
{ pkgs ? null }:

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
    prod = { offset = 0; };
    dev = { offset = 10; };
    test = { offset = 20; };
  };

  # Port roles (keys become <KEY>_PORT in slot scripts)
  # Remove or add roles as needed for your project.
  ports = {
    backend = 3000;
    frontend = 3100;
    http = 8080;
    https = 8443;
    postgres = 5432;
  };

  # Base data directory for per-slot/per-env state
  directories = {
    base = "\${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}";
  };

  tooling = {
    # Packages added to PATH for nix run apps
    runtimePackages = [ ];
    # Packages available in nix develop
    devShellPackages = [ ];
    devShellHook = ''
      echo "Nix framework dev shell ready."
    '';
  };

  install = {
    # Optional dependency install script (used when a command sets useDeps = true)
    deps = "";
  };

  # Core commands exposed as nix run .#<name>
  commands = {
    dev = {
      description = "Start the dev workflow";
      env = { "${project.envVar}" = "dev"; };
      useDeps = true;
      script = ''
        echo "No dev command configured. Edit nix/project.nix."
        exit 1
      '';
    };
    test = {
      description = "Run tests";
      env = { "${project.envVar}" = "test"; };
      useDeps = true;
      script = ''
        echo "No test command configured. Edit nix/project.nix."
        exit 1
      '';
    };
    build = {
      description = "Build artifacts";
      env = { "${project.envVar}" = "prod"; };
      useDeps = true;
      script = ''
        echo "No build command configured. Edit nix/project.nix."
        exit 1
      '';
    };
    ci = {
      description = "Run the CI pipeline";
      env = { "${project.envVar}" = "test"; };
      useDeps = true;
      script = ''
        echo "No CI command configured. Edit nix/project.nix."
        exit 1
      '';
    };
    check = {
      description = "Run quality checks";
      env = { };
      useDeps = true;
      script = ''
        echo "No check command configured. Edit nix/project.nix."
        exit 1
      '';
    };
  };

  # Supervisor (process-compose) configuration
  supervisor = {
    enable = true;
    services = { };
  };

  # Optional modules
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
    };
    nginx = {
      enable = false;
      portKeyHttp = "http";
      portKeyHttps = "https";
      dataDirName = "nginx";
    };
    playwright = {
      enable = false;
    };
  };

  # Optional packages export (flake packages)
  packages = { };
}
