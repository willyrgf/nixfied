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
  appExecutionManifests = model.features."runtime.app-execution-manifests";
  serviceSetSurfaces = model.features."runtime.service-set-surfaces";
in
assert
  requiredManifestFeatureIds == [
    "runtime.app-execution-manifests"
    "runtime.service-set-surfaces"
  ];
assert appExecutionManifests.kind == "runtime";
assert
  appExecutionManifests.summary
  == "App-scoped execution manifests for selected launchers and machine-output wrappers";
assert
  appExecutionManifests.ownerFiles == [
    "nixfied/compiler/compile-execution.nix"
    "nixfied/framework/core/materializeExecution.nix"
    "nixfied/framework/core/mkMachineOutputPrograms.nix"
  ];
assert appExecutionManifests.modelPaths == [ "apps" ];
assert
  appExecutionManifests.defaults == {
    manifestScope = "app";
    wrapperKinds = [
      "taskRef"
      "workflowRef"
      "machineOutput"
    ];
  };
assert appExecutionManifests.coverageRequired == true;
assert appExecutionManifests.coverageLayer == "manifest";
assert
  appExecutionManifests.surfaces == [
    {
      kind = "app";
      name = "selected-app";
    }
    {
      kind = "execution";
      name = "app-manifest";
    }
  ];
assert serviceSetSurfaces.kind == "runtime";
assert
  serviceSetSurfaces.summary
  == "Grouped service-set lifecycle, export, and workflow adapter surfaces";
assert
  serviceSetSurfaces.ownerFiles == [
    "nixfied/modules/service-sets.nix"
    "nixfied/framework/core/mkServiceSetPrograms.nix"
    "nixfied/framework/core/materializeExecution.nix"
    "nixfied/compiler/compile-service-sets.nix"
    "nixfied/compiler/compile-workflows.nix"
  ];
assert serviceSetSurfaces.modelPaths == [ "serviceSets" ];
assert
  serviceSetSurfaces.defaults == {
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
assert serviceSetSurfaces.coverageRequired == true;
assert serviceSetSurfaces.coverageLayer == "manifest";
assert
  serviceSetSurfaces.surfaces == [
    {
      kind = "app";
      name = "svcset::<service-set>::<operation>";
    }
    {
      kind = "workflow-phase";
      name = "preRun.serviceSets/postRun.serviceSets";
    }
  ];
pkgs.runCommand "feature-manifest-proof" { } ''
  echo "OK: manifest-layer runtime feature inventory is stable" > "$out"
''
