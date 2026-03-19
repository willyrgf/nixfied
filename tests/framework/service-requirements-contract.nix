{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  multiTaskId = "task.test.requirements.multi";
  aliasTaskId = "task.test.requirements.alias";
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

      "test.requirements.alias" = {
        id = aliasTaskId;
        summary = "Task using deprecated serviceName alias";
        description = "Used to validate alias normalization into requirements.services.";
        serviceName = "postgres";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "alias-task-ran"
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

  compiledExcluded = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ requirementsModule ];
    localOverrides = [
      (
        { ... }:
        {
          nixfied.graph.excludedServices = [ "helios" ];
        }
      )
    ];
  };

  invalidTaskAlias = builtins.tryEval (
    builtins.deepSeq ((frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        {
          nixfied.tasks."test.invalid.alias" = {
            id = "task.test.invalid.alias";
            serviceName = "search";
            runner.command = ''
              set -euo pipefail
              printf '%s\n' "invalid-alias"
            '';
          };
        }
      ];
      localOverrides = [ ];
    }).model.tasks
    ) true
  );

  invalidWorkflowRequirement = builtins.tryEval (
    builtins.deepSeq ((frameworkLib.mkNixfied {
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
    }).model.workflows
    ) true
  );

  workflowUnit = compiled.model.workflows.${workflowId}.units."required.unit";

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = compiled.model;
    projectRoot = ../..;
  };
in
assert
  compiled.model.tasks.${multiTaskId}.requirements.services == [
    "postgres"
    "helios"
  ];
assert compiled.model.tasks.${aliasTaskId}.requirements.services == [ "postgres" ];
assert compiled.model.tasks.${aliasTaskId}.serviceName == "postgres";
assert
  workflowUnit.requirements.services == [
    "postgres"
    "helios"
  ];
assert !(builtins.hasAttr multiTaskId compiledExcluded.model.tasks);
assert builtins.hasAttr workflowTaskId compiledExcluded.model.tasks;
assert !(builtins.hasAttr "required.unit" compiledExcluded.model.workflows.${workflowId}.units);
assert builtins.hasAttr "control.unit" compiledExcluded.model.workflows.${workflowId}.units;
assert invalidTaskAlias.success == false;
assert invalidWorkflowRequirement.success == false;
pkgs.runCommand "service-requirements-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  SKIP_HELIOS=1 "$EXECUTOR" run-task "${multiTaskId}" > "$TMPDIR/multi-task.out" 2>&1
  multi_rc="$?"
  set -e
  if [ "$multi_rc" -ne 0 ]; then
    echo "expected multi-requirement task skip to exit 0, got $multi_rc"
    cat "$TMPDIR/multi-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/multi-task.out" "SKIP: task '${multiTaskId}' is skipped because service 'helios' has a skip flag enabled"
  require_not_contains "$TMPDIR/multi-task.out" "multi-task-ran"

  set +e
  SKIP_POSTGRES=1 "$EXECUTOR" run-task "${aliasTaskId}" > "$TMPDIR/alias-task.out" 2>&1
  alias_rc="$?"
  set -e
  if [ "$alias_rc" -ne 0 ]; then
    echo "expected deprecated alias task skip to exit 0, got $alias_rc"
    cat "$TMPDIR/alias-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/alias-task.out" "SKIP: task '${aliasTaskId}' is skipped because service 'postgres' has a skip flag enabled"
  require_not_contains "$TMPDIR/alias-task.out" "alias-task-ran"

  SKIP_HELIOS=1 "$EXECUTOR" run-workflow "${workflowId}" > "$TMPDIR/workflow.out" 2>&1
  require_contains "$TMPDIR/workflow.out" "control-task-ran"
  require_not_contains "$TMPDIR/workflow.out" "workflow-task-ran"

  echo "OK: service requirements normalize, validate, exclude, and skip correctly" > "$out"
''
