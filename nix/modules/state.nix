{ lib, ... }:
let
  inherit (lib) types;
  vocabulary = (import ../meta/default.nix { inherit lib; }).vocabularyMap;
  inherit (import ../meta/options.nix { inherit lib; }) mkOption;
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
      type = types.enum vocabulary."enum CleanupPolicy".members;
      default = "delete-on-clean";
      description = "Whether ordinary `clean` may delete owned state or must require explicit purge.";
    };

    persistence = mkOption {
      type = types.enum vocabulary."enum PersistencePolicy".members;
      default = "run-scoped";
      description = "Whether slot state is eligible for ordinary cleanup or treated as persistent data requiring explicit purge.";
    };
  };
}
