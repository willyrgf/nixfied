{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/helpers/runtime-events.nix;
in
assert pkgs.lib.hasInfix "import ../registry/events.nix" source;
assert pkgs.lib.hasInfix "registry_append_event \"$REGISTRY_ROOT\"" source;
assert pkgs.lib.hasInfix "NIXFIED_ATTEMPT_ID:-" source;
assert pkgs.lib.hasInfix "REGISTRY_ROOT_DEFAULT=" source;
assert pkgs.lib.hasInfix "service_events_index_file_for()" source;
assert pkgs.lib.hasInfix "slot_events_index_file_for()" source;
assert pkgs.lib.hasInfix "REGISTRY_APPEND_LAST_EVENT_JSON" source;
assert pkgs.lib.hasInfix "nixfied-kernel event-detail render" source;
assert pkgs.lib.hasInfix "nixfied-kernel registry runtime-status" source;
assert !(pkgs.lib.hasInfix "service_status_file_for()" source);
assert !(pkgs.lib.hasInfix "slot_status_file_for()" source);
assert (!pkgs.lib.hasInfix "registry_events_snapshot \"$REGISTRY_ROOT\"" source);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
assert (!pkgs.lib.hasInfix "json_quote_string" source);
assert (!pkgs.lib.hasInfix "events.jsonl" source);
assert (!pkgs.lib.hasInfix "snapshot.json" source);
assert (!pkgs.lib.hasInfix "process-stop" source);
pkgs.runCommand "runtime-events-contract" { } ''
  echo "OK: runtime lifecycle events use the shared NDJSON registry" > "$out"
''
