{
  pkgs,
  model,
}:
let
  requiredAdapterFeatureIds = builtins.sort builtins.lessThan (
    builtins.filter (
      featureId:
      let
        feature = model.features.${featureId};
      in
      (feature.coverageRequired or false) && (feature.coverageLayer or "") == "adapter"
    ) (builtins.attrNames (model.features or { }))
  );
  assertFeature =
    feature: expected:
    assert feature.kind == "runtime";
    assert feature.summary == expected.summary;
    assert feature.ownerFiles == expected.ownerFiles;
    assert feature.modelPaths == expected.modelPaths;
    assert feature.defaults == expected.defaults;
    assert feature.surfaces == expected.surfaces;
    true;
  outputPrefix = model.features."runtime.output.prefix-contract";
  serviceHooks = model.features."runtime.service-hooks";
in
assert
  requiredAdapterFeatureIds == [
    "runtime.output.prefix-contract"
    "runtime.service-hooks"
  ];
assert assertFeature outputPrefix {
  summary = "Stable ASCII log prefix contract";
  ownerFiles = [
    "nixfied/framework/runtime/helpers/helpers.nix"
    "nixfied/project/tasks.nix"
  ];
  modelPaths = [ ];
  defaults = {
    prefixes = [
      "INFO:"
      "WARN:"
      "ERROR:"
      "OK:"
      "SKIP:"
    ];
  };
  surfaces = [
    {
      kind = "cli";
      name = "all-user-facing-output";
    }
  ];
};
assert assertFeature serviceHooks {
  summary = "Generated service hook env vars and service operation apps";
  ownerFiles = [
    "nixfied/framework/core/mkServiceRuntimeSurfaces.nix"
    "nixfied/framework/core/service-api.nix"
    "nixfied/framework/runtime/env-sandbox.nix"
  ];
  modelPaths = [ "services" ];
  defaults = {
    hookPrefix = "SVC_";
    appPrefix = "svc::";
  };
  surfaces = [
    {
      kind = "dispatcher";
      name = "run-task/run-workflow";
    }
    {
      kind = "app";
      name = "svc::<service>::<op>";
    }
  ];
};
pkgs.runCommand "feature-adapter-proof" { } ''
  echo "OK: adapter-layer runtime feature inventory is stable" > "$out"
''
