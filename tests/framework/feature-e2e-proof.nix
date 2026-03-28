{
  pkgs,
  model,
}:
let
  requiredE2eFeatureIds = builtins.sort builtins.lessThan (
    builtins.filter (
      featureId:
      let
        feature = model.features.${featureId};
      in
      (feature.coverageRequired or false) && (feature.coverageLayer or "") == "e2e"
    ) (builtins.attrNames (model.features or { }))
  );
  envFileLoading = model.features."runtime.ephemeral.env-file-loading";
  includeUntracked = model.features."runtime.ephemeral.include-untracked";
  registryIsolation = model.features."runtime.registry.isolation";
  sourceMaterialization = model.features."runtime.ephemeral.source-materialization";
in
assert
  requiredE2eFeatureIds == [
    "runtime.ephemeral.env-file-loading"
    "runtime.ephemeral.include-untracked"
    "runtime.ephemeral.source-materialization"
    "runtime.registry.isolation"
  ];
assert sourceMaterialization.kind == "runtime";
assert sourceMaterialization.summary == "Ephemeral source materialization defaults";
assert
  sourceMaterialization.ownerFiles == [
    "nixfied/project/conf.nix"
    "nixfied/project/runtime.nix"
    "nixfied/framework/runtime/ephemeral.nix"
  ];
assert sourceMaterialization.modelPaths == [ "runtime.ephemeral.copyMode" ];
assert
  sourceMaterialization.defaults == {
    copyMode = model.runtime.ephemeral.copyMode;
  };
assert sourceMaterialization.coverageRequired == true;
assert sourceMaterialization.coverageLayer == "e2e";
assert includeUntracked.kind == "runtime";
assert includeUntracked.summary == "Ephemeral worktree copy policy";
assert
  includeUntracked.ownerFiles == [
    "nixfied/project/conf.nix"
    "nixfied/project/runtime.nix"
    "nixfied/framework/runtime/ephemeral.nix"
  ];
assert includeUntracked.modelPaths == [ "runtime.ephemeral.includeUntracked" ];
assert
  includeUntracked.defaults == {
    includeUntracked = model.runtime.ephemeral.includeUntracked;
  };
assert includeUntracked.coverageRequired == true;
assert includeUntracked.coverageLayer == "e2e";
assert envFileLoading.kind == "runtime";
assert envFileLoading.summary == "Ephemeral host env file loading policy";
assert
  envFileLoading.ownerFiles == [
    "nixfied/project/conf.nix"
    "nixfied/project/runtime.nix"
    "nixfied/framework/runtime/ephemeral.nix"
  ];
assert
  envFileLoading.modelPaths == [
    "runtime.ephemeral.envFileMode"
    "runtime.ephemeral.envFilePath"
  ];
assert
  envFileLoading.defaults == {
    envFileMode = model.runtime.ephemeral.envFileMode;
    envFilePath = model.runtime.ephemeral.envFilePath;
  };
assert envFileLoading.coverageRequired == true;
assert envFileLoading.coverageLayer == "e2e";
assert registryIsolation.kind == "runtime";
assert registryIsolation.summary == "Run-scoped registry isolation";
assert
  registryIsolation.ownerFiles == [
    "nixfied/framework/runtime/orchestrator.nix"
    "nixfied/framework/runtime/env-sandbox.nix"
  ];
assert registryIsolation.modelPaths == [ "state.policy.registryRoot" ];
assert
  registryIsolation.defaults == {
    registryRoot = null;
  };
assert registryIsolation.coverageRequired == true;
assert registryIsolation.coverageLayer == "e2e";
pkgs.runCommand "feature-e2e-proof" { } ''
  echo "OK: end-to-end runtime feature inventory is stable" > "$out"
''
