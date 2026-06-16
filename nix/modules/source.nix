{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.codebases.main = {
    logicalRoot = mkOption {
      type = types.nonEmptyStr;
      default = ".";
      description = "Logical source root for the single main codebase.";
    };

    sourceMode = mkOption {
      type = types.enum [
        "live-workspace"
        "snapshot"
        "flake-input"
      ];
      default = "live-workspace";
      description = "Source mode for the single main codebase.";
    };

    sourceIdentity = mkOption {
      type = types.either types.path types.nonEmptyStr;
      apply = toString;
      default = "live";
      description = "Live identity placeholder, or immutable source store root for snapshot/flake-input.";
    };

    dirtyPolicy = mkOption {
      type = types.enum [
        "allow"
        "warn"
        "reject"
      ];
      default = "warn";
      description = "Dirty policy for the single main codebase.";
    };

    admissionFingerprintPolicy = mkOption {
      type = types.nonEmptyStr;
      default = "live-fingerprint";
      description = "source fingerprint policy placeholder.";
    };
  };
}
