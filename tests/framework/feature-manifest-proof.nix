{
  pkgs,
  model,
}:
let
  requiredManifestFeatureIds = builtins.sort builtins.lessThan (
    builtins.filter (
      featureId:
      let
        feature = model.features.${featureId};
      in
      (feature.coverageRequired or false) && (feature.coverageLayer or "") == "manifest"
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
  appExecutionManifests = model.features."runtime.app-execution-manifests";
  serviceSetSurfaces = model.features."runtime.service-set-surfaces";
in
assert
  requiredManifestFeatureIds == [
    "runtime.app-execution-manifests"
    "runtime.service-set-surfaces"
  ];
assert assertFeature appExecutionManifests {
  summary = "App-scoped execution manifests for selected launchers and machine-output wrappers";
  ownerFiles = [
    "nixfied/compiler/compile-execution.nix"
    "nixfied/framework/core/materializeExecution.nix"
    "nixfied/framework/core/mkMachineOutputPrograms.nix"
  ];
  modelPaths = [ "apps" ];
  defaults = {
    manifestScope = "app";
    wrapperKinds = [
      "taskRef"
      "workflowRef"
      "machineOutput"
    ];
  };
  surfaces = [
    {
      kind = "app";
      name = "selected-app";
    }
    {
      kind = "execution";
      name = "app-manifest";
    }
  ];
};
assert assertFeature serviceSetSurfaces {
  summary = "Grouped service-set lifecycle, export, and workflow adapter surfaces";
  ownerFiles = [
    "nixfied/modules/service-sets.nix"
    "nixfied/framework/core/mkServiceSetPrograms.nix"
    "nixfied/framework/core/materializeExecution.nix"
    "nixfied/compiler/compile-service-sets.nix"
    "nixfied/compiler/compile-workflows.nix"
  ];
  modelPaths = [ "serviceSets" ];
  defaults = {
    appPrefix = "svcset::";
    operations = [
      "start"
      "stop"
      "status"
      "health"
      "ready"
      "export"
    ];
  };
  surfaces = [
    {
      kind = "app";
      name = "svcset::<service-set>::<operation>";
    }
    {
      kind = "workflow-phase";
      name = "preRun.serviceSets/postRun.serviceSets";
    }
  ];
};
pkgs.runCommand "feature-manifest-proof" { } ''
  echo "OK: manifest-layer runtime feature inventory is stable" > "$out"
''
