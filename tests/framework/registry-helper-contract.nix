{ pkgs }:
let
  replay = import ../../nixfied/framework/runtime/registry/replay.nix { inherit pkgs; };
  replaySource = builtins.readFile ../../nixfied/framework/runtime/registry/replay.nix;
in
assert !(builtins.pathExists ../../nixfied/framework/runtime/helpers/run-registry.nix);
assert pkgs.lib.hasInfix "nixfied-kernel registry replay" replaySource;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" replaySource);
pkgs.runCommand "registry-helper-contract" { } ''
  ${pkgs.bash}/bin/bash -n ${replay.mkReplayTool { }}/bin/nixfied-registry-replay
  echo "OK: legacy run-registry helper is deleted and registry replay is kernel-owned" > "$out"
''
