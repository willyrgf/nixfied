{ lib, ... }:
let
  inherit (lib) types;
  vocabulary = (import ../meta/default.nix { inherit lib; }).vocabularyMap;
  inherit (import ../meta/options.nix { inherit lib; }) mkOption;
in
{
  options.nixfied.codebases.main = {
    logicalRoot = mkOption {
      type = types.nonEmptyStr;
      default = ".";
      description = "Logical root within the main codebase's declared source identity.";
    };

    sourceMode = mkOption {
      type = types.enum vocabulary."enum SourceMode".members;
      default = "live-workspace";
      description = "Whether the main codebase resolves from the live workspace or an immutable Nix-store source.";
    };

    sourceIdentity = mkOption {
      type = types.either types.path types.nonEmptyStr;
      apply = toString;
      default = "live";
      description = "`live` for a live workspace, or the immutable Nix-store source root for snapshot and flake-input modes.";
    };

    dirtyPolicy = mkOption {
      type = types.enum vocabulary."enum DirtyPolicy".members;
      default = "warn";
      description = "Admission policy for uncommitted changes in a live workspace.";
    };

    admissionFingerprintPolicy = mkOption {
      type = types.nonEmptyStr;
      default = "live-fingerprint";
      description = "Source fingerprint policy recorded in the admission contract; currently `live-fingerprint`.";
    };
  };
}
