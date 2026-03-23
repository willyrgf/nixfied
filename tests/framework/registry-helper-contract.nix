{ pkgs }:
let
  runRegistry = import ../../nixfied/framework/runtime/helpers/run-registry.nix { inherit pkgs; };
  replay = import ../../nixfied/framework/runtime/registry/replay.nix { inherit pkgs; };
  runRegistrySource = builtins.readFile ../../nixfied/framework/runtime/helpers/run-registry.nix;
  replaySource = builtins.readFile ../../nixfied/framework/runtime/registry/replay.nix;
in
assert pkgs.lib.hasInfix "write_meta_json() {" runRegistrySource;
assert pkgs.lib.hasInfix "write_meta_fields() {" runRegistrySource;
assert pkgs.lib.hasInfix "load_meta_fields() {" runRegistrySource;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" runRegistrySource);
assert pkgs.lib.hasInfix "registry_events_index_snapshot" replaySource;
assert pkgs.lib.hasInfix "declare -A replay_state=()" replaySource;
assert pkgs.lib.hasInfix "json_quote_string" replaySource;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" replaySource);
pkgs.runCommand "registry-helper-contract" { } ''
  ${pkgs.bash}/bin/bash -n ${runRegistry.runRegistryStart}
  ${pkgs.bash}/bin/bash -n ${replay.mkReplayTool { }}/bin/nixfied-registry-replay
  echo "OK: registry helper tools are jq-free" > "$out"
''
