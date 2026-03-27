{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };

  baseCompiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };

  serviceSetOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ./service-set-enabled-module.nix ];
    localOverrides = [ ];
  };

  workflowFixtureOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.tasks."test.runtime-manifest.fixture" = {
          id = "task.test.runtime-manifest.fixture";
          summary = "Runtime manifest fixture task";
          description = "Minimal task used to assert workflowRef manifest narrowing.";
          runner.command = ''
            set -euo pipefail
            echo fixture
          '';
        };

        nixfied.workflows."test.runtime-manifest.fixture" = {
          id = "workflow.test.runtime-manifest.fixture";
          summary = "Runtime manifest fixture workflow";
          description = "Minimal workflow used to assert direct workflowRef manifest fixtures.";
          units.main.taskId = "task.test.runtime-manifest.fixture";
          launcher = {
            enable = true;
            appId = "runtime-manifest-fixture";
            summary = "Runtime manifest fixture launcher";
            description = "Launcher used only by runtime-manifest fixture tests.";
            usage = [ "nix run .#runtime-manifest-fixture" ];
            ownerFile = "tests/framework/runtime-manifest-fixture-contract.nix";
          };
        };
      }
    ];
    localOverrides = [ ];
  };

  manifestIds = builtins.attrNames baseCompiled.model.compiled.runtimeManifests.byApp;
  checkManifest = baseCompiled.model.compiled.runtimeManifests.byApp.check;
  isolationManifest = baseCompiled.model.compiled.runtimeManifests.byApp.test-isolation;
  serviceSetManifest =
    serviceSetOutputs.model.compiled.runtimeManifests.byServiceSet."service-set.default";
  workflowFixtureManifest =
    workflowFixtureOutputs.model.compiled.runtimeManifests.byApp."runtime-manifest-fixture";

  sortKeys = attrs: builtins.sort builtins.lessThan (builtins.attrNames attrs);
  hasSubset = expected: actual: lib.all (value: builtins.elem value actual) expected;
in
assert
  baseCompiled.model.compiled.runtimeManifests.catalog.schema.kind
  == "nixfied-runtime-manifest-catalog";
assert baseCompiled.model.compiled.runtimeManifests.catalog.schema.version == 1;
assert baseCompiled.model.compiled.runtimeManifests.catalog.appIds == manifestIds;
assert
  baseCompiled.model.compiled.runtimeManifests.catalog.manifestCount == builtins.length manifestIds;
assert checkManifest.id == "check";
assert checkManifest.taskId == "task.check";
assert checkManifest.workflowId == null;
assert checkManifest.taskIds == [ "task.check" ];
assert checkManifest.workflowIds == [ ];
assert checkManifest.selectedServices == [ ];
assert checkManifest.model.schema.kind == "nixfied-execution-manifest";
assert checkManifest.model.compiled.runtimeMetadata.schema.kind == "nixfied-runtime-metadata";
assert sortKeys checkManifest.model.tasks == [ "task.check" ];
assert sortKeys checkManifest.model.workflows == [ ];
assert sortKeys checkManifest.model.serviceCatalog == [ ];
assert checkManifest.model.compiled.runtimeMetadata.tasks."task.check".runner.type == "derivation";
assert isolationManifest.id == "test-isolation";
assert isolationManifest.taskId == "task.ops.test-isolation";
assert isolationManifest.workflowId == null;
assert hasSubset [
  "task.ops.test-isolation"
  "task.ops.validate-env"
  "task.test.isolation.probe"
  "task.test.isolation.unit"
] (sortKeys isolationManifest.model.tasks);
assert hasSubset [ "workflow.test.isolation.probe" ] (sortKeys isolationManifest.model.workflows);
assert
  isolationManifest.model.compiled.runtimeMetadata.workflows."workflow.test.isolation.probe".family
  == "test";
assert serviceSetManifest.id == "service-set.default";
assert serviceSetManifest.name == "default";
assert
  serviceSetManifest.requiredServices == [
    "minio"
    "postgres"
  ];
assert serviceSetManifest.optionalServices == [ ];
assert
  serviceSetManifest.allServices == [
    "minio"
    "postgres"
  ];
assert serviceSetManifest.defaultOperation == "health";
assert workflowFixtureManifest.id == "runtime-manifest-fixture";
assert workflowFixtureManifest.taskId == null;
assert workflowFixtureManifest.workflowId == "workflow.test.runtime-manifest.fixture";
assert workflowFixtureManifest.taskIds == [ "task.test.runtime-manifest.fixture" ];
assert workflowFixtureManifest.workflowIds == [ "workflow.test.runtime-manifest.fixture" ];
assert workflowFixtureManifest.selectedServices == [ ];
assert sortKeys workflowFixtureManifest.model.tasks == [ "task.test.runtime-manifest.fixture" ];
assert
  sortKeys workflowFixtureManifest.model.workflows == [ "workflow.test.runtime-manifest.fixture" ];
pkgs.runCommand "runtime-manifest-fixture-contract" { } ''
  echo "OK: runtime manifest fixtures prove canonical app and service-set manifest invariants" > "$out"
''
