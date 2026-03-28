{
  pkgs,
  checks,
}:
let
  lib = pkgs.lib;
  listUtils = import ../../nixfied/framework/core/list-utils.nix;
  shardCatalog = import ./framework-test-shards.nix;
  shardNames = shardCatalog.order;
  profileNames = builtins.sort builtins.lessThan (builtins.attrNames shardCatalog.profiles);
  selfCheckName = "framework-test-layout-validation";
  expectedProfileNames = [
    "ci"
    "feature-proof"
    "full"
  ];
  checkNames = builtins.sort builtins.lessThan (builtins.attrNames checks);
  allShardCheckNames = builtins.concatLists (
    builtins.map (name: shardCatalog.checks.${name} or [ ]) shardNames
  );
  uniqueShardCheckNames = listUtils.uniquePreserveOrder allShardCheckNames;
  checkMetadata = (builtins.mapAttrs (_: drv: drv.passthru.nixfied or { }) checks) // {
    "${selfCheckName}" = {
      layer = "compile";
      canonical = false;
      covers = [ ];
    };
  };

  duplicates = builtins.filter (
    name: builtins.length (builtins.filter (candidate: candidate == name) allShardCheckNames) > 1
  ) uniqueShardCheckNames;
  missingChecks = builtins.filter (name: !(builtins.elem name uniqueShardCheckNames)) checkNames;
  unknownChecks = builtins.filter (
    name: !(builtins.elem name (checkNames ++ [ selfCheckName ]))
  ) uniqueShardCheckNames;

  profileCheckNames = listUtils.uniquePreserveOrder (
    builtins.concatLists (builtins.map (name: shardCatalog.profiles.${name} or [ ]) profileNames)
  );
  unknownProfileChecks = builtins.filter (
    name: !(builtins.elem name (checkNames ++ [ selfCheckName ]))
  ) profileCheckNames;
  duplicateProfileChecks = builtins.concatLists (
    builtins.map (
      profileName:
      let
        values = shardCatalog.profiles.${profileName} or [ ];
        uniqueValues = listUtils.uniquePreserveOrder values;
      in
      builtins.filter (
        name: builtins.length (builtins.filter (candidate: candidate == name) values) > 1
      ) uniqueValues
    ) profileNames
  );

  coveredCheckNames = listUtils.uniquePreserveOrder (
    builtins.filter (name: (checkMetadata.${name}.covers or [ ]) != [ ]) (
      checkNames ++ [ selfCheckName ]
    )
  );
  canonicalCoveredCheckNames = listUtils.uniquePreserveOrder (
    builtins.filter (
      name:
      let
        metadata = checkMetadata.${name};
      in
      (metadata.canonical or false) && (metadata.covers or [ ]) != [ ]
    ) (checkNames ++ [ selfCheckName ])
  );

  featureProofChecks = shardCatalog.profiles."feature-proof" or [ ];
  featureProofMissingChecks = builtins.filter (
    name: !(builtins.elem name featureProofChecks)
  ) coveredCheckNames;
  featureProofNonCoveredChecks = builtins.filter (
    name: !builtins.elem name coveredCheckNames
  ) featureProofChecks;

  expectedCiChecks = listUtils.uniquePreserveOrder (
    canonicalCoveredCheckNames
    ++ shardCatalog.checks.compile
    ++ shardCatalog.checks.manifest
    ++ shardCatalog.checks.kernel
    ++ shardCatalog.checks.adapters
    ++ shardCatalog.checks.migration
  );
  actualCiChecks = builtins.sort builtins.lessThan (shardCatalog.profiles.ci or [ ]);
  expectedCiChecksSorted = builtins.sort builtins.lessThan expectedCiChecks;

  expectedFullChecks = builtins.sort builtins.lessThan uniqueShardCheckNames;
  actualFullChecks = builtins.sort builtins.lessThan (shardCatalog.profiles.full or [ ]);

  renderList = values: lib.concatStringsSep ", " values;

  failureMessages = builtins.filter (message: message != "") [
    (
      if
        shardNames == [
          "compile"
          "manifest"
          "kernel"
          "adapters"
          "e2e"
          "migration"
        ]
      then
        ""
      else
        "unexpected framework::test shard order: ${renderList shardNames}"
    )
    (
      if !(builtins.elem "services" shardNames) then
        ""
      else
        "framework::test must not expose a services shard"
    )
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
      if profileNames == expectedProfileNames then
        ""
      else
        "unexpected framework::test profile names: ${renderList profileNames}"
    )
    (
      if duplicateProfileChecks == [ ] then
        ""
      else
        "duplicate checks in framework::test profiles: ${renderList duplicateProfileChecks}"
    )
    (
      if unknownProfileChecks == [ ] then
        ""
      else
        "unknown checks in framework::test profiles: ${renderList unknownProfileChecks}"
    )
    (
      if featureProofMissingChecks == [ ] then
        ""
      else
        "feature-proof profile is missing covered checks: ${renderList featureProofMissingChecks}"
    )
    (
      if featureProofNonCoveredChecks == [ ] then
        ""
      else
        "feature-proof profile includes non-feature checks: ${renderList featureProofNonCoveredChecks}"
    )
    (
      if actualCiChecks == expectedCiChecksSorted then
        ""
      else
        "ci profile drifted from canonical feature proofs plus compile/manifest/kernel/adapters/migration shards"
    )
    (
      if actualFullChecks == expectedFullChecks then
        ""
      else
        "full profile drifted from the registered shard check set"
    )
  ];
in
assert failureMessages == [ ];
pkgs.runCommand "framework-test-layout-validation" { } ''
  echo "OK: framework::test shard and profile layout is complete and feature-proof aware" > "$out"
''
