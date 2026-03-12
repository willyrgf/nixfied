{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/helpers/runtime-events.nix;
in
assert pkgs.lib.hasInfix "import ../../../registry/events.nix" source;
assert pkgs.lib.hasInfix "registry_append_event \"$REGISTRY_ROOT\"" source;
assert pkgs.lib.hasInfix "registry_events_snapshot \"$REGISTRY_ROOT\"" source;
assert pkgs.lib.hasInfix "REGISTRY_ROOT_DEFAULT=" source;
assert (!pkgs.lib.hasInfix "events.jsonl" source);
assert (!pkgs.lib.hasInfix "snapshot.json" source);
assert (!pkgs.lib.hasInfix "process-stop" source);
pkgs.runCommand "runtime-events-contract" { } ''
  echo "OK: runtime lifecycle events use the shared NDJSON registry" > "$out"
''
