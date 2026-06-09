{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.codebases.main = {
    logicalRoot = mkOption {
      type = types.nonEmptyStr;
      default = ".";
      description = "Logical source root for the single live workspace codebase.";
    };

    sourceIdentity = mkOption {
      type = types.nonEmptyStr;
      default = "live";
      description = "live workspace source identity placeholder.";
    };

    dirtyPolicy = mkOption {
      type = types.enum [
        "allow"
        "warn"
        "reject"
      ];
      default = "warn";
      description = "dirty policy for the single live workspace codebase.";
    };

    admissionFingerprintPolicy = mkOption {
      type = types.nonEmptyStr;
      default = "live-fingerprint";
      description = "source fingerprint policy placeholder.";
    };
  };
}
