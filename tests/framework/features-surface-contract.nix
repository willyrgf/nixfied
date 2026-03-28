{
  pkgs,
  model,
  apps,
}:
let
  featureIds = builtins.sort builtins.lessThan (builtins.attrNames (model.features or { }));
  expectedLines = [
    "${model.identity.projectName} features (model-generated)"
    model.identity.description
    ""
    "Features:"
  ]
  ++ map (
    featureId:
    let
      feature = model.features.${featureId};
      coverageSuffix = if feature.coverageRequired or false then " coverage=required" else "";
    in
    "  ${featureId} [${feature.kind}] - ${feature.summary}${coverageSuffix}"
  ) featureIds;
  expectedText = builtins.concatStringsSep "\n" expectedLines + "\n";
  expectedFile = pkgs.writeText "expected-features-surface.txt" expectedText;
in
pkgs.runCommand "features-surface-contract" { } ''
  set -euo pipefail

  FEATURES=${apps.features.program}
  EXPECTED=${expectedFile}

  "$FEATURES" > "$TMPDIR/features.out"

  if ! ${pkgs.diffutils}/bin/diff -u "$EXPECTED" "$TMPDIR/features.out" > "$TMPDIR/features.diff"; then
    cat "$TMPDIR/features.diff"
    echo "ERROR: features app output drifted from compiled feature inventory" >&2
    exit 1
  fi

  echo "OK: features app exports the compiled feature inventory" > "$out"
''
