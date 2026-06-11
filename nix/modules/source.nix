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
      # "reject" is deliberately absent: the runtime cannot prove live-workspace
      # cleanliness yet, so admission would refuse every model that carries it.
      # See the deferred list in AGENTS.md.
      type = types.enum [
        "allow"
        "warn"
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
