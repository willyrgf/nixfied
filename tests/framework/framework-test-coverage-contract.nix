{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/presets/framework-test.nix;
in
assert pkgs.lib.hasInfix "\"services\"" source;
assert pkgs.lib.hasInfix "shard_services() {" source;
assert pkgs.lib.hasInfix "nix build --no-link \\" source;
assert pkgs.lib.hasInfix "service-surface-catalog-contract" source;
assert pkgs.lib.hasInfix "runtime-controls-no-service-materialization-smoke" source;
assert pkgs.lib.hasInfix "run-id-semantic-inputs-contract" source;
assert pkgs.lib.hasInfix "selected-source-only-resolution-smoke" source;
assert pkgs.lib.hasInfix "managed-service-lifecycle-contract" source;
assert pkgs.lib.hasInfix "service-lifecycle-matrix-smoke" source;
assert pkgs.lib.hasInfix "ready-health-matrix-smoke" source;
assert pkgs.lib.hasInfix "ready-health-shutdown-smoke" source;
assert pkgs.lib.hasInfix "ready-helios-sync-gate-smoke" source;
assert pkgs.lib.hasInfix "supervisor-lifecycle-smoke" source;
assert pkgs.lib.hasInfix "supervisor-runtime-contract" source;
pkgs.runCommand "framework-test-coverage-contract" { } ''
  echo "OK: framework::test covers split-era and service lifecycle shards" > "$out"
''
