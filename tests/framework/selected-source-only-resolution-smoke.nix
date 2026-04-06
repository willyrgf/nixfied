{ pkgs }:
let
  serviceConfigLib = import ../../nixfied/framework/core/service-config.nix {
    inherit (pkgs) lib;
    inherit pkgs;
  };

  heliosConfig = serviceConfigLib.normalizeServiceConfig {
    name = "helios";
    config = {
      sourceKeys = [
        "safe"
        "poison"
      ];
      defaultSource = "safe";
      sources = {
        safe.package = pkgs.writeShellScript "selected-source-safe" ''
          exit 0
        '';
        poison.packageFactory = ./poison-package.nix;
      };
    };
  };
in
assert heliosConfig.package != null;
assert heliosConfig.resolved.selectedSource == "safe";
assert heliosConfig.sources.safe.package != null;
assert heliosConfig.sources.poison == { };
pkgs.runCommand "selected-source-only-resolution-smoke" { } ''
  echo "OK: service normalization resolves only the selected source package" > "$out"
''
