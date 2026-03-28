{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/presets/framework-test.nix;
  shardCatalog = import ./framework-test-shards.nix;
in
assert
  shardCatalog.order == [
    "compile"
    "manifest"
    "kernel"
    "adapters"
    "e2e"
    "migration"
  ];
assert builtins.length shardCatalog.checks.compile > 0;
assert builtins.length shardCatalog.checks.manifest > 0;
assert builtins.length shardCatalog.checks.kernel > 0;
assert builtins.length shardCatalog.checks.adapters > 0;
assert builtins.length shardCatalog.checks.e2e > 0;
assert builtins.length shardCatalog.checks.migration > 0;
assert builtins.length shardCatalog.profiles."feature-proof" > 0;
assert builtins.length shardCatalog.profiles.ci > 0;
assert builtins.length shardCatalog.profiles.full > 0;
assert pkgs.lib.hasInfix "framework-test-shards.nix" source;
assert pkgs.lib.hasInfix "PROFILE_FEATURE_PROOF_SHARDS" source;
assert pkgs.lib.hasInfix "PROFILE_CI_SHARDS" source;
assert pkgs.lib.hasInfix "PROFILE_FULL_SHARDS" source;
assert !(pkgs.lib.hasInfix "\"services\"" source);
assert !(pkgs.lib.hasInfix "\"flake-check\"" source);
assert !(pkgs.lib.hasInfix "\"launcher-pruning\"" source);
assert !(pkgs.lib.hasInfix "\"help\"" source);
assert !(pkgs.lib.hasInfix "\"workflow-ci\"" source);
assert !(pkgs.lib.hasInfix "\"isolation\"" source);
assert !(pkgs.lib.hasInfix "\"self-host\"" source);
assert !(pkgs.lib.hasInfix "nix flake check ." source);
assert !(pkgs.lib.hasInfix "workflow.ci.$MODE" source);
assert !(pkgs.lib.hasInfix "task.ops.test-isolation" source);
pkgs.runCommand "framework-test-coverage-contract" { } ''
  echo "OK: framework::test uses explicit feature-proof and layer profiles without the deleted services shard or old operational buckets" > "$out"
''
