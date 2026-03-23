{ pkgs }:
let
  lib = pkgs.lib;
  frameworkSources = builtins.filter (
    path: lib.hasSuffix ".nix" (toString path)
  ) (lib.filesystem.listFilesRecursive ../../nixfied/framework);
  jqAllowlist = map toString [
    ../../nixfied/framework/core/mkCoreSurfaces.nix
    ../../nixfied/framework/runtime/helpers/env-loader.nix
    ../../nixfied/framework/runtime/helpers/probe-commands.nix
    ../../nixfied/framework/runtime/helpers/probe-plan-runtime.nix
    ../../nixfied/framework/runtime/helpers/shell-contract.nix
    ../../nixfied/framework/runtime/services/helios/lifecycle.nix
    ../../nixfied/framework/runtime/services/supervisor/status.nix
  ];
  hasJqMarker =
    source:
    lib.any (marker: lib.hasInfix marker source) [
      "\${pkgs.jq}/bin/jq"
      "pkgs.jq"
      "jq = pkgs.jq"
    ];
  disallowedJqFiles = builtins.filter (
    path:
    let
      pathString = toString path;
      source = builtins.readFile path;
    in
    hasJqMarker source && !(builtins.elem pathString jqAllowlist)
  ) frameworkSources;
  machineOutputSource = builtins.readFile ../../nixfied/framework/core/mkMachineOutputPrograms.nix;
  deletedHelperPaths = [
    ../../nixfied/framework/core/machine-output-validate.py
    ../../nixfied/framework/core/introspection-query.py
  ];
in
assert builtins.all (path: !builtins.pathExists path) deletedHelperPaths;
assert disallowedJqFiles == [ ];
assert pkgs.lib.hasInfix "NIXFIED_MACHINE_OUTPUT_FILE=\"$payload_file\"" machineOutputSource;
assert pkgs.lib.hasInfix "did not write machine payload to declared file" machineOutputSource;
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >>\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "<\"$target_stdout\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "stdout-filter" machineOutputSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: contract migration guards enforce deleted helpers, explicit machine channels, and repo-wide jq allowlist policy" > "$out"
''
