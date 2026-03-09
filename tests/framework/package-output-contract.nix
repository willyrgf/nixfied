{
  pkgs,
  packages,
}:
assert builtins.hasAttr "nix-checks" packages;
pkgs.runCommand "package-output-contract" { } ''
  set -euo pipefail

  ${packages."nix-checks"}/bin/nix-checks --help > "$out"
  ${pkgs.gnugrep}/bin/grep -Fq "Usage: nix-checks" "$out"
  ${pkgs.gnugrep}/bin/grep -Fq -- "--mode <quick|full>" "$out"
''
