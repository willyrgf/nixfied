{ lib, system, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.project = {
    projectId = mkOption {
      type = types.strMatching "[A-Za-z0-9][A-Za-z0-9._-]*";
      description = "Stable M0 project identifier.";
    };

    name = mkOption {
      type = types.nonEmptyStr;
      description = "Human-readable project name.";
    };
  };

  options.nixfied.target.system = mkOption {
    type = types.enum [
      "aarch64-darwin"
      "aarch64-linux"
      "x86_64-darwin"
      "x86_64-linux"
    ];
    default = system;
    description = "Nix target system for the M0 model and runtime closures.";
  };

  options.nixfied.environments.dev = mkOption {
    type = types.submodule {
      options = {
        services = mkOption {
          type = types.listOf types.str;
          default = [ "synthetic" ];
          description = "M0 services in the dev environment.";
        };

        tasks = mkOption {
          type = types.listOf types.str;
          default = [ "smoke" ];
          description = "M0 tasks in the dev environment.";
        };
      };
    };
    default = { };
    description = "The single M0 environment.";
  };

  options.nixfied.slotPolicy = {
    min = mkOption {
      type = types.int;
      default = 0;
      description = "Minimum supported M0 slot.";
    };

    default = mkOption {
      type = types.int;
      default = 0;
      description = "Default supported M0 slot.";
    };

    max = mkOption {
      type = types.int;
      default = 0;
      description = "Maximum supported M0 slot.";
    };
  };
}
