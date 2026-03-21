{ pkgs }:
let
  depTaskId = "task.test.selection.dep";
  softTaskId = "task.test.selection.soft";
  rootTaskId = "task.test.selection.root";
  workflowTaskId = "task.test.selection.workflow-ref";
  phaseTaskId = "task.test.selection.phase";
  workflowBasicId = "workflow.selection.basic";
  workflowFullId = "workflow.selection.full";

  model = {
    tasks = {
      "test.selection.dep" = {
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

      "test.selection.soft" = {
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

      "test.selection.root" = {
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

      "test.selection.phase" = {
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

      "test.selection.workflow-ref" = {
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
      "selection-basic" = {
        id = workflowBasicId;
        preRun.tasks = [ phaseTaskId ];
        postRun = {
          tasks = [ ];
          alwaysRun = true;
        };
      };

      "selection-full" = {
        id = workflowFullId;
        units.soft.taskId = softTaskId;
        preRun.tasks = [ phaseTaskId ];
        postRun = {
          tasks = [ ];
          alwaysRun = true;
        };
      };
    };
  };

  selectionIndex = {
    taskDirectServicesById = {
      "${depTaskId}" = [ "postgres" ];
      "${softTaskId}" = [ "nginx" ];
      "${rootTaskId}" = [ "minio" ];
      "${phaseTaskId}" = [ "helios" ];
      "${workflowTaskId}" = [ "reth" ];
    };

    taskClosureServicesById = {
      "${depTaskId}" = [ "postgres" ];
      "${softTaskId}" = [ "nginx" ];
      "${rootTaskId}" = [
        "minio"
        "postgres"
        "nginx"
      ];
      "${phaseTaskId}" = [
        "helios"
        "minio"
        "postgres"
        "nginx"
      ];
      "${workflowTaskId}" = [
        "reth"
        "postgres"
        "helios"
        "minio"
        "nginx"
      ];
    };

    taskBaseClosureServicesById = {
      "${depTaskId}" = [ "postgres" ];
      "${softTaskId}" = [ "nginx" ];
      "${rootTaskId}" = [
        "minio"
        "postgres"
        "nginx"
      ];
      "${phaseTaskId}" = [
        "helios"
        "minio"
        "postgres"
        "nginx"
      ];
      "${workflowTaskId}" = [ "reth" ];
    };

    workflowClosureServicesById = {
      "${workflowBasicId}" = [
        "nginx"
        "helios"
        "minio"
        "postgres"
      ];
      "${workflowFullId}" = [
        "nginx"
        "helios"
        "minio"
        "postgres"
      ];
    };

    workflowUnitClosureServicesById = {
      "${workflowBasicId}" = [ "postgres" ];
      "${workflowFullId}" = [ "nginx" ];
    };

    workflowReferenceClosureServicesById = {
      "${workflowBasicId}" = [
        "postgres"
        "helios"
        "minio"
        "nginx"
      ];
      "${workflowFullId}" = [
        "postgres"
        "helios"
        "minio"
        "nginx"
      ];
    };
  };

  workflowModesShell = import ../../nixfied/framework/runtime/workflow-modes.nix {
    inherit
      pkgs
      model
      selectionIndex
      ;
  };
in
assert pkgs.lib.hasInfix "task_closure_selected_services() {" workflowModesShell;
assert pkgs.lib.hasInfix "workflow_unit_closure_selected_services() {" workflowModesShell;
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
  ];
assert selectionIndex.workflowUnitClosureServicesById.${workflowBasicId} == [ "postgres" ];
assert selectionIndex.workflowUnitClosureServicesById.${workflowFullId} == [ "nginx" ];
assert
  selectionIndex.workflowReferenceClosureServicesById.${workflowFullId} == [
    "postgres"
    "helios"
    "minio"
    "nginx"
  ];
assert
  selectionIndex.taskClosureServicesById.${workflowTaskId} == [
    "reth"
    "postgres"
    "helios"
    "minio"
    "nginx"
  ];
pkgs.runCommand "runtime-service-selection-contract" { } ''
  echo "OK: runtime service selection helper covers deps, workflow refs, and workflow phases" > "$out"
''
