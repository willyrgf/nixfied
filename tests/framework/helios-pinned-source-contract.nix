{ pkgs }:
let
  conf = import ../../nixfied/project/conf.nix { inherit pkgs; };
  helios = conf.services.helios;
in
assert helios.sources ? pinned;
assert helios.sourceKinds ? pinned;
assert helios.defaultSource == "pinned";
pkgs.runCommand "helios-pinned-source-contract" { } ''
  echo "OK: helios default source is pinned" > "$out"
''
