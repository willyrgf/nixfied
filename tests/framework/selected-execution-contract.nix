{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };

  baseOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };

  workflowRefOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.workflows.ci-full.launcher = {
          enable = true;
          appId = "ci-full-direct";
          summary = "Exact workflowRef execution contract";
          description = "Ensures direct workflowRef apps keep exact execution scope.";
          usage = [ "nix run .#ci-full-direct" ];
          ownerFile = "tests/framework/selected-execution-contract.nix";
        };
      }
    ];
    localOverrides = [ ];
  };

  sortKeys = attrs: builtins.sort builtins.lessThan (builtins.attrNames attrs);
  hasSubset = expected: actual: lib.all (value: builtins.elem value actual) expected;
  baseExecution = baseOutputs.model.compiled.execution;
  checkExecution = baseExecution.apps.byId.check;
  isolationExecution = baseExecution.apps.byId."test-isolation";
  workflowExecution = workflowRefOutputs.model.compiled.execution.apps.byId."ci-full-direct";
  checkProgram = builtins.readFile baseOutputs.apps.check.program;
  workflowProgram = builtins.readFile workflowRefOutputs.apps."ci-full-direct".program;
in
assert baseExecution.schema.kind == "nixfied-execution";
assert baseExecution.schema.version == 1;
assert !(baseOutputs.model.compiled ? runtimeMetadata);
assert baseOutputs.model.compiled.execution ? runtimeMetadata;
assert lib.hasInfix "run-selected-app.nix" checkProgram;
assert !(lib.hasInfix "run-selected-app.nix" workflowProgram);
assert checkExecution.taskId == "task.check";
assert checkExecution.workflowId == null;
assert checkExecution.taskIds == [ "task.check" ];
assert checkExecution.workflowIds == [ ];
assert !(checkExecution.model.compiled ? runtimeMetadata);
assert sortKeys checkExecution.model.tasks == [ "task.check" ];
assert sortKeys checkExecution.model.workflows == [ ];
assert sortKeys checkExecution.model.serviceCatalog == [ ];
assert checkExecution.model.compiled.execution.runtimeMetadata.tasks."task.check".runner.type == "derivation";
assert builtins.elem "task.ops.test-isolation" (sortKeys isolationExecution.model.tasks);
assert builtins.elem "task.ops.validate-env" (sortKeys isolationExecution.model.tasks);
assert builtins.elem "task.test.isolation.probe" (sortKeys isolationExecution.model.tasks);
assert builtins.elem "task.test.isolation.unit" (sortKeys isolationExecution.model.tasks);
assert !(builtins.elem "task.ci" (sortKeys isolationExecution.model.tasks));
assert hasSubset [ "workflow.test.isolation.probe" ] (sortKeys isolationExecution.model.workflows);
assert
  isolationExecution.model.compiled.execution.runtimeMetadata.workflows."workflow.test.isolation.probe".family
  == "test";
assert workflowExecution.taskId == null;
assert workflowExecution.workflowId == "workflow.ci.full";
assert workflowExecution.taskIds != [ ];
assert workflowExecution.workflowIds == [ "workflow.ci.full" ];
assert sortKeys workflowExecution.model.workflows == [ "workflow.ci.full" ];
assert
  workflowExecution.model.compiled.execution.runtimeMetadata.workflows."workflow.ci.full".family
  == "ci";
pkgs.runCommand "selected-execution-contract" { } ''
  echo "OK: selected execution stays narrowed for taskRef and workflowRef apps" > "$out"
''
