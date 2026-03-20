{
  pkgs,
  packages,
  apps,
}:
assert builtins.hasAttr "nix-checks" packages;
assert builtins.hasAttr "help" apps;
assert builtins.hasAttr "docs" apps;
assert builtins.hasAttr "features" apps;
assert builtins.hasAttr "run-task" apps;
assert builtins.hasAttr "run-workflow" apps;
assert builtins.hasAttr "run-workflow-parallel" apps;
assert builtins.hasAttr "runs" apps;
assert builtins.hasAttr "stop-run" apps;
assert builtins.hasAttr "stop-all-runs" apps;
assert !(builtins.hasAttr "registry::replay" apps);
assert builtins.any (appName: pkgs.lib.hasPrefix "task::" appName) (builtins.attrNames apps);
assert builtins.any (appName: pkgs.lib.hasPrefix "svc::" appName) (builtins.attrNames apps);
pkgs.runCommand "package-output-contract" { } ''
  set -euo pipefail

  ${packages."nix-checks"}/bin/nix-checks --help > "$out"
  ${pkgs.gnugrep}/bin/grep -Fq "Usage: nix-checks" "$out"
  ${pkgs.gnugrep}/bin/grep -Fq -- "--mode <quick|full>" "$out"
''
