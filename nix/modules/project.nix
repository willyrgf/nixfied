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
    defaultText = lib.literalExpression "system";
    description = "Target system shared by the compiled model, runtime, and every declared closure.";
  };

  # The adopter-owned public verb surface (VERB-1): an explicit list of task
  # ids that become flake apps (`.#check` -> `runtime run --task check`).
  # Which tasks form the public surface is a *choice*, not a derivable fact —
  # explicit export is what keeps imported adapter tasks from silently
  # becoming public apps. The control namespace (`run`, `ps`, `down`, `clean`,
  # `model-check`) is framework-reserved and can never collide.
  options.nixfied.surface.verbs = mkOption {
    type = types.listOf types.nonEmptyStr;
    default = [ ];
    description = "Task ids exported as project flake apps; framework control names are reserved.";
  };

  options.nixfied.slotPolicy = {
    min = mkOption {
      type = types.int;
      default = 0;
      description = "Lowest slot number accepted by runtime commands.";
    };

    default = mkOption {
      type = types.int;
      default = 0;
      description = "Slot selected when a runtime command receives no explicit `--slot`.";
    };

    max = mkOption {
      type = types.int;
      default = 0;
      description = "Highest slot number accepted by runtime commands.";
    };
  };
}
