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
  ephemeralOwnerFiles = [
    "nixfied/project/conf.nix"
    "nixfied/project/runtime.nix"
    "nixfied/framework/runtime/ephemeral.nix"
  ];
  assertFeature =
    feature: expected:
    assert feature.kind == "runtime";
    assert feature.summary == expected.summary;
    assert feature.ownerFiles == expected.ownerFiles;
    assert feature.modelPaths == expected.modelPaths;
    assert feature.defaults == expected.defaults;
    true;
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
assert assertFeature sourceMaterialization {
  summary = "Ephemeral source materialization defaults";
  ownerFiles = ephemeralOwnerFiles;
  modelPaths = [ "runtime.ephemeral.copyMode" ];
  defaults = {
    copyMode = model.runtime.ephemeral.copyMode;
  };
};
assert assertFeature includeUntracked {
  summary = "Ephemeral worktree copy policy";
  ownerFiles = ephemeralOwnerFiles;
  modelPaths = [ "runtime.ephemeral.includeUntracked" ];
  defaults = {
    includeUntracked = model.runtime.ephemeral.includeUntracked;
  };
};
assert assertFeature envFileLoading {
  summary = "Ephemeral host env file loading policy";
  ownerFiles = ephemeralOwnerFiles;
  modelPaths = [
    "runtime.ephemeral.envFileMode"
    "runtime.ephemeral.envFilePath"
  ];
  defaults = {
    envFileMode = model.runtime.ephemeral.envFileMode;
    envFilePath = model.runtime.ephemeral.envFilePath;
  };
};
assert assertFeature registryIsolation {
  summary = "Run-scoped registry isolation";
  ownerFiles = [
    "nixfied/framework/runtime/orchestrator.nix"
    "nixfied/framework/runtime/env-sandbox.nix"
  ];
  modelPaths = [ "state.policy.registryRoot" ];
  defaults = {
    registryRoot = null;
  };
};
pkgs.runCommand "feature-e2e-proof" { } ''
  echo "OK: end-to-end runtime feature inventory is stable" > "$out"
''
