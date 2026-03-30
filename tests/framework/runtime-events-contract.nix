{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/runtime-events-programs.nix;
in
assert pkgs.lib.hasInfix "import ./registry/events.nix" source;
assert pkgs.lib.hasInfix "registry_append_event \"$REGISTRY_ROOT\"" source;
assert pkgs.lib.hasInfix "nixfied-kernel event-detail render" source;
assert pkgs.lib.hasInfix "nixfied-kernel event-state derive" source;
assert pkgs.lib.hasInfix "nixfied-kernel service-policy runtime-event" source;
assert pkgs.lib.hasInfix "nixfied-kernel registry runtime-status" source;
assert pkgs.lib.hasInfix "import ./helpers/kernel-export-runtime.nix" source;
assert pkgs.lib.hasInfix "import ../core/runtime-event-policy.nix" source;
assert !(pkgs.lib.hasInfix "import ./service-policy.nix" source);
assert !(pkgs.lib.hasInfix "projectMeta = project.project or" source);
assert !(pkgs.lib.hasInfix "baseDirExpr =" source);
assert !(pkgs.lib.hasInfix "artifactsRootExpr =" source);
assert !(pkgs.lib.hasInfix "ephemeralPrefix =" source);
assert (!pkgs.lib.hasInfix "registry_events_snapshot \"$REGISTRY_ROOT\"" source);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
assert (!pkgs.lib.hasInfix "events.jsonl" source);
assert (!pkgs.lib.hasInfix "snapshot.json" source);
pkgs.runCommand "runtime-events-contract" { } ''
  echo "OK: runtime event programs stay on the registry append edge and consume core-owned event policy" > "$out"
''
