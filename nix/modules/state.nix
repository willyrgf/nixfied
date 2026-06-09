{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.state = {
    markerIdentity = mkOption {
      type = types.nonEmptyStr;
      default = "nixfied-state";
      description = "Marker identity written into state ownership metadata.";
    };

    stateEpoch = mkOption {
      type = types.nonEmptyStr;
      default = "1";
      description = "state epoch.";
    };

    cleanupPolicy = mkOption {
      type = types.enum [
        "delete-on-clean"
        "protected"
      ];
      default = "delete-on-clean";
      description = "state cleanup policy.";
    };

    persistence = mkOption {
      type = types.enum [
        "run-scoped"
        "persistent"
      ];
      default = "run-scoped";
      description = "state persistence policy.";
    };
  };
}
