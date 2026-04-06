{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };

  multiTaskId = "task.test.requirements.multi";
  singleTaskId = "task.test.requirements.single";
  workflowTaskId = "task.test.requirements.workflow";
  controlTaskId = "task.test.requirements.control";
  workflowId = "workflow.test.requirements";

  requirementsModule = {
    nixfied.tasks = {
      "test.requirements.multi" = {
        id = multiTaskId;
        summary = "Task with multiple service requirements";
        description = "Used to validate runtime skip against any required service.";
        requirements.services = [
          "postgres"
          "helios"
        ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "multi-task-ran"
        '';
      };

      "test.requirements.single" = {
        id = singleTaskId;
        summary = "Task with a single service requirement";
        description = "Used to validate runtime skip for single-service requirements.";
        requirements.services = [ "postgres" ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "single-task-ran"
        '';
      };

      "test.requirements.workflow" = {
        id = workflowTaskId;
        summary = "Workflow task with task-level service requirements";
        description = "Unit-level requirements should union with these requirements.";
        requirements.services = [ "postgres" ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "workflow-task-ran"
        '';
      };

      "test.requirements.control" = {
        id = controlTaskId;
        summary = "Workflow control task";
        description = "Should continue running when requirement-gated units are skipped.";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "control-task-ran"
        '';
      };
    };

    nixfied.workflows."test.requirements" = {
      id = workflowId;
      summary = "Workflow requirements contract";
      description = "Exercises unit-level service requirements and runtime skip.";
      units = {
        "control.unit" = {
          taskId = controlTaskId;
        };

        "required.unit" = {
          taskId = workflowTaskId;
          needs = [ "control.unit" ];
          requirements.services = [ "helios" ];
        };
      };
    };
  };

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ requirementsModule ];
    localOverrides = [ ];
  };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ requirementsModule ];
    localOverrides = [ ];
  };

  compiledExcluded = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ requirementsModule ];
    localOverrides = [
      (
        _: {
          nixfied.graph.excludedServices = [ "helios" ];
        }
      )
    ];
  };

  invalidTaskServiceName = builtins.tryEval (
    builtins.deepSeq (frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        {
          nixfied.tasks."test.invalid.service-name" = {
            id = "task.test.invalid.service-name";
            serviceName = "postgres";
            runner.command = ''
              set -euo pipefail
              printf '%s\n' "invalid-service-name"
            '';
          };
        }
      ];
      localOverrides = [ ];
    }).model.tasks true
  );

  invalidWorkflowServiceName = builtins.tryEval (
    builtins.deepSeq (frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        {
          nixfied.tasks."test.invalid.workflow.base" = {
            id = "task.test.invalid.workflow.base";
            runner.command = ''
              set -euo pipefail
              printf '%s\n' "invalid-workflow-base"
            '';
          };

          nixfied.workflows."test.invalid.service-name" = {
            id = "workflow.test.invalid.service-name";
            units."bad.unit" = {
              taskId = "task.test.invalid.workflow.base";
              serviceName = "postgres";
            };
          };
        }
      ];
      localOverrides = [ ];
    }).model.workflows true
  );

  invalidWorkflowRequirement = builtins.tryEval (
    builtins.deepSeq (frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        {
          nixfied.tasks."test.invalid.base" = {
            id = "task.test.invalid.base";
            runner.command = ''
              set -euo pipefail
              printf '%s\n' "invalid-base"
            '';
          };

          nixfied.workflows."test.invalid.requirements" = {
            id = "workflow.test.invalid.requirements";
            units."bad.unit" = {
              taskId = "task.test.invalid.base";
              requirements.services = [ "search" ];
            };
          };
        }
      ];
      localOverrides = [ ];
    }).model.workflows true
  );

  workflowUnit = compiled.model.workflows.${workflowId}.units."required.unit";

  runtimeDeps = runtimeFixture.runtimeMaterialization {
    inherit (compiled) model services serviceDefinitions;
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    inherit (compiled) model services;
    projectRoot = ../..;
    inherit (runtimeDeps) serviceDispatcherProgram;
    runtimeBin = runtimeDeps.serviceDispatcherProgram;
  };
in
assert
  compiled.model.tasks.${multiTaskId}.requirements.services == [
    "postgres"
    "helios"
  ];
assert compiled.model.tasks.${singleTaskId}.requirements.services == [ "postgres" ];
assert !(compiled.model.tasks.${singleTaskId} ? serviceName);
assert
  workflowUnit.requirements.services == [
    "postgres"
    "helios"
  ];
assert !(workflowUnit ? serviceName);
assert !(builtins.hasAttr multiTaskId compiledExcluded.model.tasks);
assert builtins.hasAttr workflowTaskId compiledExcluded.model.tasks;
assert !(builtins.hasAttr "required.unit" compiledExcluded.model.workflows.${workflowId}.units);
assert builtins.hasAttr "control.unit" compiledExcluded.model.workflows.${workflowId}.units;
assert !invalidTaskServiceName.success;
assert !invalidWorkflowServiceName.success;
assert !invalidWorkflowRequirement.success;
pkgs.runCommand "service-requirements-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"
  RUN_WORKFLOW_APP="${frameworkOutputs.apps."run-workflow".program}"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  "$EXECUTOR" run-task "${multiTaskId}" --exclude-services helios > "$TMPDIR/multi-task.out" 2>&1
  multi_rc="$?"
  set -e
  if [ "$multi_rc" -ne 0 ]; then
    echo "expected multi-requirement task skip to exit 0, got $multi_rc"
    cat "$TMPDIR/multi-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/multi-task.out" "SKIP: task '${multiTaskId}' is skipped because service 'helios' is excluded"
  require_not_contains "$TMPDIR/multi-task.out" "multi-task-ran"

  set +e
  "$EXECUTOR" run-task "${singleTaskId}" --exclude-services postgres > "$TMPDIR/single-task.out" 2>&1
  single_rc="$?"
  set -e
  if [ "$single_rc" -ne 0 ]; then
    echo "expected single-requirement task skip to exit 0, got $single_rc"
    cat "$TMPDIR/single-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/single-task.out" "SKIP: task '${singleTaskId}' is skipped because service 'postgres' is excluded"
  require_not_contains "$TMPDIR/single-task.out" "single-task-ran"

  "$EXECUTOR" run-workflow "${workflowId}" --exclude-services helios > "$TMPDIR/workflow.out" 2>&1
  require_contains "$TMPDIR/workflow.out" "control-task-ran"
  require_not_contains "$TMPDIR/workflow.out" "workflow-task-ran"

  set +e
  "$RUN_TASK_APP" "${multiTaskId}" --exclude-services helios > "$TMPDIR/public-multi-task.out" 2>&1
  public_multi_rc="$?"
  set -e
  if [ "$public_multi_rc" -ne 0 ]; then
    echo "expected public multi-requirement task skip to exit 0, got $public_multi_rc"
    cat "$TMPDIR/public-multi-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/public-multi-task.out" "SKIP: task '${multiTaskId}' is skipped because service 'helios' is excluded"
  require_not_contains "$TMPDIR/public-multi-task.out" "multi-task-ran"

  set +e
  "$RUN_TASK_APP" "${singleTaskId}" --exclude-services postgres > "$TMPDIR/public-single-task.out" 2>&1
  public_single_rc="$?"
  set -e
  if [ "$public_single_rc" -ne 0 ]; then
    echo "expected public single-requirement task skip to exit 0, got $public_single_rc"
    cat "$TMPDIR/public-single-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/public-single-task.out" "SKIP: task '${singleTaskId}' is skipped because service 'postgres' is excluded"
  require_not_contains "$TMPDIR/public-single-task.out" "single-task-ran"

  "$RUN_WORKFLOW_APP" "${workflowId}" --exclude-services helios > "$TMPDIR/public-workflow.out" 2>&1
  require_contains "$TMPDIR/public-workflow.out" "control-task-ran"
  require_not_contains "$TMPDIR/public-workflow.out" "workflow-task-ran"

  echo "OK: service requirements validate, exclude, and skip correctly" > "$out"
''
