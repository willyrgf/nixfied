{ lib, ... }:
let
  inherit (lib) types;
  vocabulary = (import ../meta/manifest-structure.nix { inherit lib; }).vocabularyMap;
  inherit (import ../meta/options.nix { inherit lib; }) mkOption;
in
{
  options.nixfied.state = {
    markerIdentity = mkOption {
      type = types.nonEmptyStr;
      default = "nixfied-state";
      description = "Identity written into the slot marker and required before the runtime may adopt or clean its state.";
    };

    persistence = mkOption {
      type = types.enum vocabulary."enum PersistencePolicy".members;
      default = "run-scoped";
      description = "The sole application-data retention policy: `run-scoped` data is deleted after safe session teardown and by ordinary `clean`; `persistent` data survives sessions and ordinary `clean`, and only explicit purge deletes it.";
    };
  };
}
