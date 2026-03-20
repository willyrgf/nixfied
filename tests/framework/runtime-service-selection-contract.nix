{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };

  depTaskId = "task.test.selection.dep";
  softTaskId = "task.test.selection.soft";
  rootTaskId = "task.test.selection.root";
  workflowTaskId = "task.test.selection.workflow-ref";
  phaseTaskId = "task.test.selection.phase";
  workflowBasicId = "workflow.selection.basic";
  workflowFullId = "workflow.selection.full";

  selectionModule = {
    nixfied.tasks = {
      "test.selection.dep" = {
        id = depTaskId;
        requirements.services = [ "postgres" ];
        runner.command = ''
          set -euo pipefail
          echo dep
        '';
      };

      "test.selection.soft" = {
        id = softTaskId;
        requirements.services = [ "nginx" ];
        runner.command = ''
          set -euo pipefail
          echo soft
        '';
      };

      "test.selection.root" = {
        id = rootTaskId;
        requirements.services = [ "minio" ];
        deps = {
          needs = [ depTaskId ];
          softNeeds = [ softTaskId ];
        };
        runner.command = ''
          set -euo pipefail
          echo root
        '';
      };

      "test.selection.phase" = {
        id = phaseTaskId;
        requirements.services = [ "helios" ];
        deps.needs = [ rootTaskId ];
        runner.command = ''
          set -euo pipefail
          echo phase
        '';
      };

      "test.selection.workflow-ref" = {
        id = workflowTaskId;
        requirements.services = [ "reth" ];
        runner = {
          type = "workflowRef";
          workflowId = workflowFullId;
        };
      };
    };

    nixfied.workflows = {
      "selection-basic" = {
        id = workflowBasicId;
        summary = "selection basic";
        units.dep.taskId = depTaskId;
        preRun.tasks = [ phaseTaskId ];
        postRun = {
          tasks = [ ];
          alwaysRun = true;
        };
      };

      "selection-full" = {
        id = workflowFullId;
        summary = "selection full";
        units.soft.taskId = softTaskId;
        preRun.tasks = [ phaseTaskId ];
        postRun = {
          tasks = [ ];
          alwaysRun = true;
        };
      };
    };
  };

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ selectionModule ];
    localOverrides = [ ];
  };

  selection = compiled.serviceSelection;
in
assert selection.taskDirectServicesById.${depTaskId} == [ "postgres" ];
assert
  selection.taskClosureServicesById.${rootTaskId} == [
    "minio"
    "postgres"
    "nginx"
  ];
assert selection.taskBaseClosureServicesById.${workflowTaskId} == [ "reth" ];
assert
  selection.workflowClosureServicesById.${workflowFullId} == [
    "nginx"
    "helios"
    "minio"
    "postgres"
  ];
assert
  selection.workflowReferenceClosureServicesById.${workflowFullId} == [
    "postgres"
    "helios"
    "minio"
    "nginx"
  ];
assert
  selection.taskClosureServicesById.${workflowTaskId} == [
    "reth"
    "postgres"
    "helios"
    "minio"
    "nginx"
  ];
pkgs.runCommand "runtime-service-selection-contract" { } ''
  echo "OK: runtime service selection helper covers deps, workflow refs, and workflow phases" > "$out"
''
