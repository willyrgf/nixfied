{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  options.nixfied.codebases.main = {
    logicalRoot = mkOption {
      type = types.nonEmptyStr;
      default = ".";
      description = "Logical source root for the single M0 live workspace codebase.";
    };

    sourceIdentity = mkOption {
      type = types.nonEmptyStr;
      default = "live";
      description = "M0 live workspace source identity placeholder.";
    };

    dirtyPolicy = mkOption {
      type = types.enum [
        "allow"
        "warn"
        "reject"
      ];
      default = "warn";
      description = "M0 dirty policy for the single live workspace codebase.";
    };

    admissionFingerprintPolicy = mkOption {
      type = types.nonEmptyStr;
      default = "m0-placeholder";
      description = "M0 source fingerprint policy placeholder.";
    };
  };
}
