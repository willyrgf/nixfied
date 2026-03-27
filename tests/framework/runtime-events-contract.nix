{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/helpers/runtime-events.nix;
in
assert pkgs.lib.hasInfix "import ../registry/events.nix" source;
assert pkgs.lib.hasInfix "registry_append_event \"$REGISTRY_ROOT\"" source;
assert pkgs.lib.hasInfix "nixfied-kernel event-detail render" source;
assert pkgs.lib.hasInfix "nixfied-kernel event-state derive" source;
assert pkgs.lib.hasInfix "nixfied-kernel service-policy runtime-event" source;
assert pkgs.lib.hasInfix "nixfied-kernel registry runtime-status" source;
assert pkgs.lib.hasInfix "import ./kernel-export-runtime.nix" source;
assert !(pkgs.lib.hasInfix "import ./service-policy.nix" source);
assert (!pkgs.lib.hasInfix "registry_events_snapshot \"$REGISTRY_ROOT\"" source);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
assert (!pkgs.lib.hasInfix "events.jsonl" source);
assert (!pkgs.lib.hasInfix "snapshot.json" source);
pkgs.runCommand "runtime-events-contract" { } ''
  echo "OK: runtime event helpers stay on the registry append edge and delegate semantics to the kernel" > "$out"
''
