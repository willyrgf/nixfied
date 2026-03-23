{ pkgs }:
let
  machineOutputSource = builtins.readFile ../../nixfied/framework/core/mkMachineOutputPrograms.nix;
  migratedSources = [
    (builtins.readFile ../../nixfied/framework/runtime/common-runtime.nix)
    (builtins.readFile ../../nixfied/framework/runtime/executor-runtime.nix)
    (builtins.readFile ../../nixfied/framework/runtime/orchestrator-runtime.nix)
    (builtins.readFile ../../nixfied/framework/runtime/helpers/runtime-events.nix)
    (builtins.readFile ../../nixfied/framework/runtime/helpers/summary.nix)
    (builtins.readFile ../../nixfied/framework/runtime/workflow-modes.nix)
  ];
  deletedHelperPaths = [
    ../../nixfied/framework/core/machine-output-validate.py
    ../../nixfied/framework/core/introspection-query.py
  ];
in
assert builtins.all (path: !builtins.pathExists path) deletedHelperPaths;
assert builtins.all (source: !pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source) migratedSources;
assert pkgs.lib.hasInfix "NIXFIED_MACHINE_OUTPUT_FILE=\"$payload_file\"" machineOutputSource;
assert pkgs.lib.hasInfix "did not write machine payload to declared file" machineOutputSource;
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >>\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "<\"$target_stdout\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "stdout-filter" machineOutputSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: contract migration guards enforce deleted helpers, explicit machine channels, and no semantic jq" > "$out"
''
