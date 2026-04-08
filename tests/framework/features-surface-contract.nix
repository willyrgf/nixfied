{
  pkgs,
  model,
}:
let
  features = model.features or { };
  featureIds = builtins.sort builtins.lessThan (builtins.attrNames features);
  allowedCoverageLayers = [
    "compile"
    "adapter"
    "e2e"
  ];
  missingStructure = builtins.filter (
    featureId:
    let
      feature = features.${featureId};
    in
    (feature.kind or "") == ""
    || (feature.summary or "") == ""
    || (feature.status or "") == ""
    || (feature.ownerFiles or [ ]) == [ ]
    || !(builtins.isList (feature.modelPaths or [ ]))
    || (feature.surfaces or [ ]) == [ ]
  ) featureIds;
  missingCoverageLayer = builtins.filter (
    featureId:
    let
      feature = features.${featureId};
    in
    (feature.coverageRequired or false)
    && !(builtins.elem (feature.coverageLayer or "") allowedCoverageLayers)
  ) featureIds;
in
assert featureIds != [ ];
assert missingStructure == [ ];
assert missingCoverageLayer == [ ];
pkgs.runCommand "features-surface-contract" { } ''
  echo "OK: feature metadata is structurally complete without relying on CLI snapshots" > "$out"
''
