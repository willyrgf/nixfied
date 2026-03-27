{ pkgs }:
let
  depTaskId = "task.test.selection.dep";
  softTaskId = "task.test.selection.soft";
  rootTaskId = "task.test.selection.root";
  workflowTaskId = "task.test.selection.workflow-ref";
  phaseTaskId = "task.test.selection.phase";
  workflowBasicId = "workflow.selection.basic";
  workflowFullId = "workflow.selection.full";
  workflowPhaseServiceSet = {
    serviceSetId = "service-set.selection";
    serviceSetName = "selection";
    operation = "status";
    selectedServices = [ "reth" ];
  };

  model = {
    tasks = {
      "${depTaskId}" = {
        id = depTaskId;
        summary = "dep";
        requirements.services = [ "postgres" ];
        produces = { };
        scheduling = { };
        runtime = { };
        runner.command = ''
          set -euo pipefail
          echo dep
        '';
      };

      "${softTaskId}" = {
        id = softTaskId;
        summary = "soft";
        requirements.services = [ "nginx" ];
        produces = { };
        scheduling = { };
        runtime = { };
        runner.command = ''
          set -euo pipefail
          echo soft
        '';
      };

      "${rootTaskId}" = {
        id = rootTaskId;
        summary = "root";
        requirements.services = [ "minio" ];
        produces = { };
        scheduling = { };
        deps = {
          needs = [ depTaskId ];
          softNeeds = [ softTaskId ];
        };
        runtime = { };
        runner.command = ''
          set -euo pipefail
          echo root
        '';
      };

      "${phaseTaskId}" = {
        id = phaseTaskId;
        summary = "phase";
        requirements.services = [ "helios" ];
        produces = { };
        scheduling = { };
        deps.needs = [ rootTaskId ];
        runtime = { };
        runner.command = ''
          set -euo pipefail
          echo phase
        '';
      };

      "${workflowTaskId}" = {
        id = workflowTaskId;
        summary = "workflow ref";
        requirements.services = [ "reth" ];
        produces = { };
        scheduling = { };
        runtime = { };
        runner = {
          type = "workflowRef";
          workflowId = workflowFullId;
          command = "";
        };
      };
    };

    workflows = {
      "${workflowBasicId}" = {
        id = workflowBasicId;
        units.dep.taskId = depTaskId;
        preRun.tasks = [ phaseTaskId ];
        postRun = {
          tasks = [ ];
          alwaysRun = true;
        };
      };

      "${workflowFullId}" = {
        id = workflowFullId;
        units.soft.taskId = softTaskId;
        preRun = {
          tasks = [ phaseTaskId ];
          serviceSets = [ workflowPhaseServiceSet ];
        };
        postRun = {
          tasks = [ ];
          serviceSets = [ ];
          alwaysRun = true;
        };
      };
    };
  };

  selectionIndex =
    import ../../nixfied/compiler/compile-selection-index.nix
      {
        inherit (pkgs) lib;
      }
      {
        tasks = model.tasks;
        workflows = model.workflows;
        serviceCatalog = { };
      };

  runtimeSelectionIndex = import ../../nixfied/framework/runtime/service-selection.nix {
    inherit (pkgs) lib;
    inherit model;
  };
  runtimeMetadata =
    import ../../nixfied/compiler/compile-runtime-metadata.nix
      {
        inherit (pkgs) lib;
      }
      {
        tasks = model.tasks;
        workflows = model.workflows;
        serviceCatalog = { };
        apps = { };
        inherit selectionIndex;
      };
in
assert runtimeSelectionIndex.taskIds == selectionIndex.taskIds;
assert runtimeSelectionIndex.workflowIds == selectionIndex.workflowIds;
assert runtimeSelectionIndex.workflowFamilies == selectionIndex.workflowFamilies;
assert runtimeSelectionIndex.taskDirectServicesById == selectionIndex.taskDirectServicesById;
assert
  runtimeSelectionIndex.taskBaseClosureServicesById == selectionIndex.taskBaseClosureServicesById;
assert runtimeSelectionIndex.taskClosureServicesById == selectionIndex.taskClosureServicesById;
assert runtimeSelectionIndex.taskRunnerWorkflowIdById == selectionIndex.taskRunnerWorkflowIdById;
assert runtimeSelectionIndex.workflowModesByFamily == selectionIndex.workflowModesByFamily;
assert
  runtimeSelectionIndex.workflowUnitClosureServicesById
  == selectionIndex.workflowUnitClosureServicesById;
assert
  runtimeSelectionIndex.workflowClosureServicesById == selectionIndex.workflowClosureServicesById;
assert
  runtimeSelectionIndex.workflowReferenceClosureServicesById
  == selectionIndex.workflowReferenceClosureServicesById;
assert
  selectionIndex.taskIds == [
    depTaskId
    phaseTaskId
    rootTaskId
    softTaskId
    workflowTaskId
  ];
assert
  selectionIndex.workflowIds == [
    workflowBasicId
    workflowFullId
  ];
assert selectionIndex.taskDirectServicesById.${depTaskId} == [ "postgres" ];
assert
  selectionIndex.taskClosureServicesById.${rootTaskId} == [
    "minio"
    "postgres"
    "nginx"
  ];
assert selectionIndex.taskBaseClosureServicesById.${workflowTaskId} == [ "reth" ];
assert
  selectionIndex.workflowClosureServicesById.${workflowFullId} == [
    "nginx"
    "helios"
    "minio"
    "postgres"
    "reth"
  ];
assert selectionIndex.workflowUnitClosureServicesById.${workflowBasicId} == [ "postgres" ];
assert selectionIndex.workflowUnitClosureServicesById.${workflowFullId} == [ "nginx" ];
assert
  selectionIndex.workflowReferenceClosureServicesById.${workflowFullId} == [
    "postgres"
    "helios"
    "minio"
    "nginx"
    "reth"
  ];
assert
  selectionIndex.taskClosureServicesById.${workflowTaskId} == [
    "reth"
    "postgres"
    "helios"
    "minio"
    "nginx"
  ];
assert
  runtimeMetadata.tasks.${rootTaskId}.closureSelectedServices
  == selectionIndex.taskClosureServicesById.${rootTaskId};
assert
  runtimeMetadata.tasks.${workflowTaskId}.baseClosureSelectedServices
  == selectionIndex.taskBaseClosureServicesById.${workflowTaskId};
assert
  runtimeMetadata.workflows.${workflowFullId}.unitClosureSelectedServices
  == selectionIndex.workflowUnitClosureServicesById.${workflowFullId};
assert
  runtimeMetadata.workflows.${workflowFullId}.closureSelectedServices
  == selectionIndex.workflowClosureServicesById.${workflowFullId};
assert
  runtimeMetadata.workflows.${workflowFullId}.referenceClosureSelectedServices
  == selectionIndex.workflowReferenceClosureServicesById.${workflowFullId};
assert
  runtimeMetadata.workflows.${workflowFullId}.phases.preRun.serviceSets
  == [ workflowPhaseServiceSet ];
pkgs.runCommand "runtime-service-selection-contract" { } ''
  echo "OK: runtime service selection metadata covers deps, workflow refs, and workflow phases" > "$out"
''
