{ pkgs }:
let
  confSource = builtins.readFile ../../nixfied/project/conf.nix;
in
assert !(pkgs.lib.hasInfix "../.framework/" confSource);
assert !(pkgs.lib.hasInfix "nixfied/.framework/" confSource);
pkgs.runCommand "project-config-boundary" { } ''
  echo "OK: project config avoids direct framework path references" > "$out"
''
