{
  pkgs,
  checks,
}:
let
  lib = pkgs.lib;
  listUtils = import ../../nixfied/framework/core/list-utils.nix;
  shardCatalog = import ./framework-test-shards.nix;
  shardNames = shardCatalog.order;
  selfCheckName = "framework-test-shard-validation";
  checkNames = builtins.sort builtins.lessThan (builtins.attrNames checks);
  allShardCheckNames = builtins.concatLists (
    builtins.map (name: shardCatalog.checks.${name} or [ ]) shardNames
  );
  uniqueShardCheckNames = listUtils.uniquePreserveOrder allShardCheckNames;
  checkMetadata = (builtins.mapAttrs (_: drv: drv.passthru.nixfied or { }) checks) // {
    "${selfCheckName}" = {
      layer = "migration";
    };
  };

  duplicates = builtins.filter (
    name: builtins.length (builtins.filter (candidate: candidate == name) allShardCheckNames) > 1
  ) uniqueShardCheckNames;
  missingChecks = builtins.filter (name: !(builtins.elem name uniqueShardCheckNames)) checkNames;
  unknownChecks = builtins.filter (
    name: !(builtins.elem name (checkNames ++ [ selfCheckName ]))
  ) uniqueShardCheckNames;

  wrongCompileLayer = builtins.filter (
    name: (checkMetadata.${name}.layer or "") != "compile"
  ) shardCatalog.checks.compile;
  wrongManifestLayer = builtins.filter (
    name: (checkMetadata.${name}.layer or "") != "manifest"
  ) shardCatalog.checks.manifest;
  wrongKernelLayer = builtins.filter (
    name: (checkMetadata.${name}.layer or "") != "kernel"
  ) shardCatalog.checks.kernel;
  wrongAdapterLayer = builtins.filter (
    name: (checkMetadata.${name}.layer or "") != "adapter"
  ) shardCatalog.checks.adapters;
  wrongE2eLayer = builtins.filter (
    name: (checkMetadata.${name}.layer or "") != "e2e"
  ) shardCatalog.checks.e2e;
  wrongMigrationLayer = builtins.filter (
    name: (checkMetadata.${name}.layer or "") != "migration"
  ) shardCatalog.checks.migration;

  renderList = values: lib.concatStringsSep ", " values;
  failureMessages = builtins.filter (message: message != "") [
    (
      if duplicates == [ ] then
        ""
      else
        "duplicate framework::test shard assignments: ${renderList duplicates}"
    )
    (
      if missingChecks == [ ] then
        ""
      else
        "framework checks missing from shard catalog: ${renderList missingChecks}"
    )
    (
      if unknownChecks == [ ] then
        ""
      else
        "unknown framework::test shard checks: ${renderList unknownChecks}"
    )
    (
      if wrongCompileLayer == [ ] then
        ""
      else
        "compile shard includes non-compile checks: ${renderList wrongCompileLayer}"
    )
    (
      if wrongManifestLayer == [ ] then
        ""
      else
        "manifest shard includes non-manifest checks: ${renderList wrongManifestLayer}"
    )
    (
      if wrongKernelLayer == [ ] then
        ""
      else
        "kernel shard includes non-kernel checks: ${renderList wrongKernelLayer}"
    )
    (
      if wrongAdapterLayer == [ ] then
        ""
      else
        "adapters shard includes non-adapter checks: ${renderList wrongAdapterLayer}"
    )
    (
      if wrongE2eLayer == [ ] then
        ""
      else
        "e2e shard includes non-e2e checks: ${renderList wrongE2eLayer}"
    )
    (
      if wrongMigrationLayer == [ ] then
        ""
      else
        "migration shard includes non-migration checks: ${renderList wrongMigrationLayer}"
    )
  ];
in
assert
  shardCatalog.profiles.ci == [
    "compile"
    "manifest"
    "kernel"
    "adapters"
    "migration"
  ];
assert shardCatalog.profiles.full == shardCatalog.order;
assert failureMessages == [ ];
pkgs.runCommand "framework-test-shard-validation" { } ''
  echo "OK: framework::test shard catalog matches registered checks and ownership layers" > "$out"
''
