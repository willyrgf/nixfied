{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.state = {
    markerIdentity = mkOption {
      type = types.nonEmptyStr;
      default = "nixfied-state";
      description = "Identity written into the slot marker and required before the runtime may adopt or clean its state.";
    };

    stateEpoch = mkOption {
      type = types.nonEmptyStr;
      default = "1";
      description = "Project-chosen state compatibility epoch; a mismatch must pass the declared cleanup policy before admission.";
    };

    cleanupPolicy = mkOption {
      type = types.enum [
        "delete-on-clean"
        "protected"
      ];
      default = "delete-on-clean";
      description = "Whether ordinary `clean` may delete owned state or must require explicit purge.";
    };

    persistence = mkOption {
      type = types.enum [
        "run-scoped"
        "persistent"
      ];
      default = "run-scoped";
      description = "Whether slot state is eligible for ordinary cleanup or treated as persistent data requiring explicit purge.";
    };
  };
}
