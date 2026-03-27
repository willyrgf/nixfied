{
  pkgs,
  model,
  checks,
}:
let
  lib = pkgs.lib;
  listUtils = import ../../nixfied/framework/core/list-utils.nix;
  safeRepoRoot = builtins.unsafeDiscardStringContext (builtins.toString ../..);
  validLayers = [
    "compile"
    "manifest"
    "kernel"
    "adapter"
    "e2e"
    "migration"
  ];
  validProofKinds = [
    "contract"
    "fixture"
    "unit"
    "integration"
    "smoke"
    "guard"
  ];

  featureIds = builtins.sort builtins.lessThan (builtins.attrNames (model.features or { }));

  requiredFeatureIds = builtins.filter (
    featureId:
    let
      feature = model.features.${featureId};
    in
    feature.coverageRequired or false
  ) featureIds;

  checkNames = builtins.sort builtins.lessThan (builtins.attrNames checks);

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
      layer = metadata.layer or "";
      proofKind = metadata.proofKind or "";
      canonical = metadata.canonical or false;
      covers = metadata.covers or [ ];
      ownerFiles = metadata.ownerFiles or [ ];
    in
    (
      !builtins.elem kind [
        "contract"
        "smoke"
      ]
    )
    || !builtins.elem layer validLayers
    || !builtins.elem proofKind validProofKinds
    || !builtins.isBool canonical
    || !builtins.isList covers
    || (canonical && covers == [ ])
    || !builtins.isList ownerFiles
    || ownerFiles == [ ]
    || builtins.any (path: !builtins.pathExists "${safeRepoRoot}/${path}") ownerFiles
  ) checkNames;

  coveredFeatureIds = listUtils.uniquePreserveOrder (
    builtins.concatLists (builtins.map (name: (checkMetadata.${name}.covers or [ ])) checkNames)
  );

  unknownCoveredFeatureIds = builtins.filter (
    featureId: !(builtins.elem featureId featureIds)
  ) coveredFeatureIds;

  missingRequiredFeatureIds = builtins.filter (
    featureId: !(builtins.elem featureId coveredFeatureIds)
  ) requiredFeatureIds;
  expectedLayerForFeature = featureId: model.features.${featureId}.coverageLayer or "";
  unknownExpectedLayerFeatureIds = builtins.filter (
    featureId: expectedLayerForFeature featureId == ""
  ) requiredFeatureIds;
  canonicalCheckNamesForFeature =
    featureId:
    builtins.filter (
      name:
      let
        metadata = checkMetadata.${name};
      in
      (metadata.canonical or false) && builtins.elem featureId (metadata.covers or [ ])
    ) checkNames;
  missingCanonicalFeatureIds = builtins.filter (
    featureId: canonicalCheckNamesForFeature featureId == [ ]
  ) requiredFeatureIds;
  duplicateCanonicalFeatureIds = builtins.filter (
    featureId: builtins.length (canonicalCheckNamesForFeature featureId) > 1
  ) requiredFeatureIds;
  wrongCanonicalLayerFeatureIds = builtins.filter (
    featureId:
    let
      expectedLayer = expectedLayerForFeature featureId;
      canonicalChecks = canonicalCheckNamesForFeature featureId;
    in
    expectedLayer != ""
    && canonicalChecks != [ ]
    && builtins.length canonicalChecks == 1
    && (checkMetadata.${builtins.head canonicalChecks}.layer or "") != expectedLayer
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
  unknownExpectedLayerMessage =
    if unknownExpectedLayerFeatureIds == [ ] then
      ""
    else
      "required features missing expected test layer mapping: ${renderList unknownExpectedLayerFeatureIds}";
  missingCanonicalCoverageMessage =
    if missingCanonicalFeatureIds == [ ] then
      ""
    else
      "missing canonical feature coverage for: ${renderList missingCanonicalFeatureIds}";
  duplicateCanonicalCoverageMessage =
    if duplicateCanonicalFeatureIds == [ ] then
      ""
    else
      "multiple canonical proofs declared for: ${renderList duplicateCanonicalFeatureIds}";
  wrongCanonicalLayerMessage =
    if wrongCanonicalLayerFeatureIds == [ ] then
      ""
    else
      "canonical proofs declared at wrong layer for: ${renderList wrongCanonicalLayerFeatureIds}";

  failureMessages = builtins.filter (message: message != "") [
    invalidMetadataMessage
    unknownCoverageMessage
    missingCoverageMessage
    unknownExpectedLayerMessage
    missingCanonicalCoverageMessage
    duplicateCanonicalCoverageMessage
    wrongCanonicalLayerMessage
  ];
in
assert featureIds != [ ];
assert failureMessages == [ ];
pkgs.runCommand "feature-coverage-validation" { } ''
  echo "OK: framework feature coverage metadata is complete and canonicalized by layer" > "$out"
''
