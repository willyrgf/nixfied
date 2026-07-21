{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.codebases.main = {
    logicalRoot = mkOption {
      type = types.nonEmptyStr;
      default = ".";
      description = "Logical root within the main codebase's declared source identity.";
    };

    sourceMode = mkOption {
      type = types.enum [
        "live-workspace"
        "snapshot"
        "flake-input"
      ];
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
      type = types.enum [
        "allow"
        "warn"
        "reject"
      ];
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
