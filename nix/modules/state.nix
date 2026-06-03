{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.state = {
    markerIdentity = mkOption {
      type = types.nonEmptyStr;
      default = "nixfied-m0";
      description = "Marker identity written into M0 state ownership metadata.";
    };

    stateEpoch = mkOption {
      type = types.nonEmptyStr;
      default = "m0";
      description = "M0 state epoch.";
    };

    cleanupPolicy = mkOption {
      type = types.enum [
        "delete-on-clean"
        "protected"
      ];
      default = "delete-on-clean";
      description = "M0 state cleanup policy.";
    };

    persistence = mkOption {
      type = types.enum [
        "run-scoped"
        "persistent"
      ];
      default = "run-scoped";
      description = "M0 state persistence policy.";
    };
  };
}
