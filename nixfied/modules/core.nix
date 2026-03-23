{ lib, ... }:
let
  t = lib.types;
  serviceConfigLib = import ../framework/core/service-config.nix { inherit lib; };
in
{
  imports = [
    ./apps.nix
    ./service-sets.nix
    ./state.nix
    ./runtime.nix
    ./tasks.nix
    ./workflows.nix
    ./operations.nix
    ./services/postgres.nix
    ./services/nginx.nix
    ./services/minio.nix
    ./services/reth.nix
    ./services/helios.nix
  ];

  options.nixfied = {
    identity = {
      projectId = lib.mkOption {
        type = t.str;
        default = "nixfied-project";
        description = "Stable project identity used in model and state paths.";
      };

      projectName = lib.mkOption {
        type = t.str;
        default = "Nixfied Project";
      };

      description = lib.mkOption {
        type = t.str;
        default = "Model-driven Nixfied project";
      };
    };

    tooling = {
      runtimePackages = lib.mkOption {
        type = t.listOf t.package;
        default = [ ];
      };

      devShellPackages = lib.mkOption {
        type = t.listOf t.package;
        default = [ ];
      };

      devShellHook = lib.mkOption {
        type = t.lines;
        default = ''
          echo "INFO: nixfied dev shell ready"
        '';
      };
    };

    packages = lib.mkOption {
      type = t.attrsOf t.package;
      default = { };
    };

    legacyLocal = {
      apps = lib.mkOption {
        type = t.attrsOf t.anything;
        default = { };
        description = "Legacy local extension apps surfaced from nixfied/local/default.nix.";
      };

      packages = lib.mkOption {
        type = t.attrsOf t.package;
        default = { };
        description = "Legacy local extension packages surfaced from nixfied/local/default.nix.";
      };

      devShells = lib.mkOption {
        type = t.attrsOf t.package;
        default = { };
        description = "Legacy local extension dev shells surfaced from nixfied/local/default.nix.";
      };
    };

    graph = {
      excludedServices = lib.mkOption {
        type = t.listOf (t.enum serviceConfigLib.supportedServiceNames);
        default = [ ];
        description = "Pure graph-time service exclusions applied before project service projection.";
      };
    };
  };
}
