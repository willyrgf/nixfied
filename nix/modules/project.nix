{ lib, system, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.project = {
    projectId = mkOption {
      type = types.strMatching "[A-Za-z0-9][A-Za-z0-9._-]*";
      description = "Stable project identifier.";
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
    description = "Nix target system for the model and runtime closures.";
  };

  options.nixfied.slotPolicy = {
    min = mkOption {
      type = types.int;
      default = 0;
      description = "Minimum supported slot.";
    };

    default = mkOption {
      type = types.int;
      default = 0;
      description = "Default supported slot.";
    };

    max = mkOption {
      type = types.int;
      default = 0;
      description = "Maximum supported slot.";
    };
  };
}
