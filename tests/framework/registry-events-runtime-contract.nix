{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/registry/events.nix;
  locksSource = builtins.readFile ../../nixfied/framework/runtime/registry/events-locks.nix;
  snapshotSource = builtins.readFile ../../nixfied/framework/runtime/registry/events-snapshot.nix;
  appendSource = builtins.readFile ../../nixfied/framework/runtime/registry/events-append.nix;
in
assert pkgs.lib.hasInfix "registryLocksShell = import ./events-locks.nix" source;
assert pkgs.lib.hasInfix "registrySnapshotShell = import ./events-snapshot.nix" source;
assert pkgs.lib.hasInfix "registryAppendShell = import ./events-append.nix" source;
assert pkgs.lib.hasInfix "\${registryLocksShell}" source;
assert pkgs.lib.hasInfix "\${registrySnapshotShell}" source;
assert pkgs.lib.hasInfix "\${registryAppendShell}" source;
assert (!pkgs.lib.hasInfix "registry_lock_acquire_shared() {" source);
assert (!pkgs.lib.hasInfix "registry_events_snapshot() {" source);
assert (!pkgs.lib.hasInfix "registry_append_event() {" source);
assert pkgs.lib.hasInfix "registry_lock_acquire_shared() {" locksSource;
assert pkgs.lib.hasInfix "registry_events_file() {" locksSource;
assert pkgs.lib.hasInfix "registry_events_index_file() {" locksSource;
assert pkgs.lib.hasInfix "\${pkgs.procps}/bin/ps" locksSource;
assert pkgs.lib.hasInfix "flock -w" locksSource;
assert pkgs.lib.hasInfix "registry_events_snapshot() {" snapshotSource;
assert pkgs.lib.hasInfix "registry_events_index_snapshot() {" snapshotSource;
assert pkgs.lib.hasInfix "registry_snapshot_cleanup() {" snapshotSource;
assert pkgs.lib.hasInfix "registry_next_seq() {" appendSource;
assert pkgs.lib.hasInfix "registry_append_event() {" appendSource;
assert pkgs.lib.hasInfix "registryEventValidator = import ../../contracts/mkValidator.nix"
  appendSource;
assert pkgs.lib.hasInfix "contractRef = \"runtime.registryEvent\";" appendSource;
assert pkgs.lib.hasInfix "events_index_file" appendSource;
assert pkgs.lib.hasInfix "detail_reason" appendSource;
assert pkgs.lib.hasInfix "detail_exit_code" appendSource;
assert !(pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" appendSource);
assert pkgs.lib.hasInfix "json_quote_string \"$REGISTRY_EVENT_KIND\"" appendSource;
assert pkgs.lib.hasInfix "\"attemptId\":%s" appendSource;
pkgs.runCommand "registry-events-runtime-contract" { } ''
  echo "OK: registry runtime helpers are split, stable, and jq-free on append" > "$out"
''
