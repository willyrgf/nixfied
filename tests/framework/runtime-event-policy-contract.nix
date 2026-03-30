{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/core/runtime-event-policy.nix;
in
assert pkgs.lib.hasInfix "registryRoot =" source;
assert pkgs.lib.hasInfix "baseDirExpr =" source;
assert pkgs.lib.hasInfix "artifactsRootExpr =" source;
assert pkgs.lib.hasInfix "ephemeralPrefix =" source;
assert pkgs.lib.hasInfix "runtime_index_segment()" source;
assert pkgs.lib.hasInfix "service_events_index_file_for()" source;
assert pkgs.lib.hasInfix "slot_events_index_file_for()" source;
assert !(pkgs.lib.hasInfix "pkgs.writeShellScript" source);
assert !(pkgs.lib.hasInfix "nixfied-kernel" source);
assert !(pkgs.lib.hasInfix "registry_append_event" source);
assert !(pkgs.lib.hasInfix "registry runtime-status" source);
pkgs.runCommand "runtime-event-policy-contract" { } ''
  echo "OK: runtime event policy is core-owned data and naming logic without runtime shell or registry IO" > "$out"
''
