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
  outputPrefix = model.features."runtime.output.prefix-contract";
  serviceHooks = model.features."runtime.service-hooks";
in
assert
  requiredAdapterFeatureIds == [
    "runtime.output.prefix-contract"
    "runtime.service-hooks"
  ];
assert outputPrefix.kind == "runtime";
assert outputPrefix.summary == "Stable ASCII log prefix contract";
assert
  outputPrefix.ownerFiles == [
    "nixfied/framework/runtime/helpers/helpers.nix"
    "nixfied/project/tasks.nix"
  ];
assert outputPrefix.modelPaths == [ ];
assert
  outputPrefix.defaults.prefixes == [
    "INFO:"
    "WARN:"
    "ERROR:"
    "OK:"
    "SKIP:"
  ];
assert outputPrefix.coverageRequired == true;
assert outputPrefix.coverageLayer == "adapter";
assert
  outputPrefix.surfaces == [
    {
      kind = "cli";
      name = "all-user-facing-output";
    }
  ];
assert serviceHooks.kind == "runtime";
assert serviceHooks.summary == "Generated service hook env vars and service operation apps";
assert
  serviceHooks.ownerFiles == [
    "nixfied/framework/core/mkServiceRuntimeSurfaces.nix"
    "nixfied/framework/runtime/helpers/service-api.nix"
    "nixfied/framework/runtime/env-sandbox.nix"
  ];
assert serviceHooks.modelPaths == [ "services" ];
assert
  serviceHooks.defaults == {
    hookPrefix = "SVC_";
    appPrefix = "svc::";
  };
assert serviceHooks.coverageRequired == true;
assert serviceHooks.coverageLayer == "adapter";
assert
  serviceHooks.surfaces == [
    {
      kind = "dispatcher";
      name = "run-task/run-workflow";
    }
    {
      kind = "app";
      name = "svc::<service>::<op>";
    }
  ];
pkgs.runCommand "feature-adapter-proof" { } ''
  echo "OK: adapter-layer runtime feature inventory is stable" > "$out"
''
