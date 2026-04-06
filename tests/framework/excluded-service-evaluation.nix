{
  pkgs,
  registry,
}:
let
  inherit (pkgs) lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    inherit (pkgs) system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  normalizeSourceKeys = sources: builtins.sort builtins.lessThan (builtins.attrNames sources);
  normalizePostgresEnvConfigs = lib.mapAttrs (
    _: envCfg: {
      extraConfig = envCfg.extraConfig or "";
    }
  );

  throwingProjectModule = {
    imports = [
      (import ../../nixfied/project/services.nix {
        inherit pkgs;
        conf = {
          services = {
            postgres = { };
            nginx = { };
            minio = { };
            reth = { };
            helios = {
              dataDirName = throw "helios evaluated unexpectedly";
            };
          };
        };
        inherit
          normalizeSourceKeys
          normalizePostgresEnvConfigs
          ;
      })
    ];
  };

  compiledThrowingWithoutExclusion = builtins.tryEval (frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ throwingProjectModule ];
      extraModules = [ ];
      localOverrides = [ ];
    }).model.serviceCatalog."service.helios".config.dataDirName;

  compiledThrowingWithExclusion = builtins.tryEval (frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ throwingProjectModule ];
      extraModules = [ ];
      localOverrides = [
        (
          _:
          {
            nixfied.graph.excludedServices = [ "helios" ];
          }
        )
      ];
    }).model.serviceCatalog;

  excludedTaskId = "task.test.excluded.helios";
  hardTaskId = "task.test.hard.needs.helios";
  softTaskId = "task.test.soft.needs.helios";
  controlTaskId = "task.test.control";
  unitLocalTaskId = "task.test.unit.local";
  afterLocalTaskId = "task.test.after.local";
  workflowId = "workflow.test.excluded";

  graphTestModule = {
    nixfied.tasks = {
      "test.excluded.helios" = {
        id = excludedTaskId;
        summary = "Excluded helios task";
        description = "Should be pruned when helios is excluded.";
        requirements.services = [ "helios" ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "excluded-helios-ran"
        '';
      };

      "test.hard.needs.helios" = {
        id = hardTaskId;
        summary = "Hard depends on excluded helios task";
        description = "Should be pruned because deps.needs points at a pruned task.";
        deps.needs = [ excludedTaskId ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "hard-needs-ran"
        '';
      };

      "test.soft.needs.helios" = {
        id = softTaskId;
        summary = "Soft depends on excluded helios task";
        description = "Should survive and drop the soft dependency inside workflows.";
        deps.softNeeds = [ excludedTaskId ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "soft-needs-ran"
        '';
      };

      "test.control" = {
        id = controlTaskId;
        summary = "Control task";
        description = "Should remain executable.";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "control-ran"
        '';
      };

      "test.unit.local" = {
        id = unitLocalTaskId;
        summary = "Workflow-local exclusion task";
        description = "Should survive globally and only be pruned from the workflow unit.";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "unit-local-ran"
        '';
      };

      "test.after.local" = {
        id = afterLocalTaskId;
        summary = "Depends on a workflow-local exclusion unit";
        description = "Should survive globally and be pruned from the workflow when its unit dependency disappears.";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "after-local-ran"
        '';
      };
    };

    nixfied.workflows."test.excluded" = {
      id = workflowId;
      summary = "Excluded service workflow";
      description = "Exercises task and workflow pruning for excluded services.";
      units = {
        "excluded.task" = {
          taskId = excludedTaskId;
        };

        "hard.task" = {
          taskId = hardTaskId;
        };

        "soft.task" = {
          taskId = softTaskId;
        };

        "control.task" = {
          taskId = controlTaskId;
        };

        "local.unit" = {
          taskId = unitLocalTaskId;
          requirements.services = [ "helios" ];
        };

        "after.local" = {
          taskId = afterLocalTaskId;
          needs = [ "local.unit" ];
        };
      };

      preRun.tasks = [
        excludedTaskId
        controlTaskId
      ];

      postRun.tasks = [
        excludedTaskId
        controlTaskId
      ];
    };
  };

  compiledExcluded = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ graphTestModule ];
    localOverrides = [
      (
        _:
        {
          nixfied.graph.excludedServices = [ "helios" ];
        }
      )
    ];
  };

  excludedWorkflow = compiledExcluded.model.workflows.${workflowId};
  workflowPlanNames = map (unit: unit.name) excludedWorkflow.plan;
  expectedDocsLine = "- ${workflowId}: control.task -> soft.task";
  featureLines = compiledExcluded.model.views.features.lines;

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    inherit (compiledExcluded) model;
    inherit (compiledExcluded) services;
    projectRoot = ../..;
  };
in
assert !compiledThrowingWithoutExclusion.success;
assert compiledThrowingWithExclusion.success;
assert !(builtins.hasAttr "service.helios" compiledThrowingWithExclusion.value);
assert !(builtins.hasAttr "service.helios" compiledExcluded.model.serviceCatalog);
assert !(builtins.hasAttr "service.helios" compiledExcluded.model.features);
assert !(builtins.hasAttr excludedTaskId compiledExcluded.model.tasks);
assert !(builtins.hasAttr hardTaskId compiledExcluded.model.tasks);
assert builtins.hasAttr softTaskId compiledExcluded.model.tasks;
assert builtins.hasAttr controlTaskId compiledExcluded.model.tasks;
assert builtins.hasAttr unitLocalTaskId compiledExcluded.model.tasks;
assert builtins.hasAttr afterLocalTaskId compiledExcluded.model.tasks;
assert !(builtins.hasAttr "excluded.task" excludedWorkflow.units);
assert !(builtins.hasAttr "hard.task" excludedWorkflow.units);
assert builtins.hasAttr "soft.task" excludedWorkflow.units;
assert builtins.hasAttr "control.task" excludedWorkflow.units;
assert !(builtins.hasAttr "local.unit" excludedWorkflow.units);
assert !(builtins.hasAttr "after.local" excludedWorkflow.units);
assert excludedWorkflow.preRun.tasks == [ controlTaskId ];
assert excludedWorkflow.postRun.tasks == [ controlTaskId ];
assert excludedWorkflow.units."soft.task".needs == [ ];
assert
  excludedWorkflow.stages == [
    [
      "control.task"
      "soft.task"
    ]
  ];
assert
  workflowPlanNames == [
    "control.task"
    "soft.task"
  ];
assert builtins.elem expectedDocsLine compiledExcluded.model.views.docs.lines;
assert !(builtins.any (line: lib.hasInfix "service.helios" line) featureLines);
pkgs.runCommand "excluded-service-evaluation" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  "$EXECUTOR" run-task "${excludedTaskId}" > "$TMPDIR/excluded-task.out" 2>&1
  excluded_rc="$?"
  set -e
  if [ "$excluded_rc" -eq 0 ]; then
    echo "expected excluded task to be unknown at runtime"
    cat "$TMPDIR/excluded-task.out"
    exit 1
  fi
  require_contains "$TMPDIR/excluded-task.out" "ERROR: unknown task '${excludedTaskId}'"

  "$EXECUTOR" run-workflow "${workflowId}" > "$TMPDIR/workflow.out" 2>&1
  require_contains "$TMPDIR/workflow.out" "control-ran"
  require_contains "$TMPDIR/workflow.out" "soft-needs-ran"
  require_not_contains "$TMPDIR/workflow.out" "excluded-helios-ran"
  require_not_contains "$TMPDIR/workflow.out" "hard-needs-ran"
  require_not_contains "$TMPDIR/workflow.out" "unit-local-ran"
  require_not_contains "$TMPDIR/workflow.out" "after-local-ran"

  echo "OK: excluded services are not evaluated and are pruned from compiled graphs" > "$out"
''
