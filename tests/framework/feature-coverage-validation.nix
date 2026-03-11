{
  pkgs,
  model,
  checks,
}:
let
  lib = pkgs.lib;
  safeRepoRoot = builtins.unsafeDiscardStringContext (builtins.toString ../..);

  featureIds = builtins.sort builtins.lessThan (builtins.attrNames (model.features or { }));

  requiredFeatureIds = builtins.filter (
    featureId:
    let
      feature = model.features.${featureId};
    in
    feature.coverageRequired or false
  ) featureIds;

  checkNames = builtins.sort builtins.lessThan (builtins.attrNames checks);

  unique =
    list:
    builtins.foldl' (acc: value: if builtins.elem value acc then acc else acc ++ [ value ]) [ ] list;

  checkMetadata = builtins.listToAttrs (
    builtins.map (name: {
      inherit name;
      value = checks.${name}.passthru.nixfied or { };
    }) checkNames
  );

  invalidMetadataChecks = builtins.filter (
    name:
    let
      metadata = checkMetadata.${name};
      kind = metadata.kind or "";
      covers = metadata.covers or [ ];
      ownerFiles = metadata.ownerFiles or [ ];
    in
    (
      !builtins.elem kind [
        "contract"
        "smoke"
      ]
    )
    || !builtins.isList covers
    || !builtins.isList ownerFiles
    || ownerFiles == [ ]
    || builtins.any (path: !builtins.pathExists "${safeRepoRoot}/${path}") ownerFiles
  ) checkNames;

  coveredFeatureIds = unique (
    builtins.concatLists (builtins.map (name: (checkMetadata.${name}.covers or [ ])) checkNames)
  );

  unknownCoveredFeatureIds = builtins.filter (
    featureId: !(builtins.elem featureId featureIds)
  ) coveredFeatureIds;

  missingRequiredFeatureIds = builtins.filter (
    featureId: !(builtins.elem featureId coveredFeatureIds)
  ) requiredFeatureIds;

  renderList = values: lib.concatStringsSep ", " values;

  invalidMetadataMessage =
    if invalidMetadataChecks == [ ] then
      ""
    else
      "invalid framework check metadata for: ${renderList invalidMetadataChecks}";

  unknownCoverageMessage =
    if unknownCoveredFeatureIds == [ ] then
      ""
    else
      "unknown covered feature ids: ${renderList unknownCoveredFeatureIds}";

  missingCoverageMessage =
    if missingRequiredFeatureIds == [ ] then
      ""
    else
      "missing required feature coverage for: ${renderList missingRequiredFeatureIds}";

  failureMessages = builtins.filter (message: message != "") [
    invalidMetadataMessage
    unknownCoverageMessage
    missingCoverageMessage
  ];
in
assert featureIds != [ ];
assert failureMessages == [ ];
pkgs.runCommand "feature-coverage-validation" { } ''
  echo "OK: framework feature coverage metadata is complete" > "$out"
''
