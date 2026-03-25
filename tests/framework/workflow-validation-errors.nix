{
  pkgs,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    system = pkgs.system;
  };
  compilerSource = builtins.readFile ../../nixfied/compiler/compile-workflows.nix;
  workflowModuleSource = builtins.readFile ../../nixfied/modules/workflows.nix;
  workflowSchemaSource = builtins.readFile ../../nixfied/schemas/workflow-contract.json;

  evalWorkflows =
    extraModule:
    builtins.tryEval (
      builtins.deepSeq ((frameworkLib.mkNixfied {
        projectRoot = ../..;
        projectModules = [ ../../nixfied/project/module.nix ];
        extraModules = [ extraModule ];
        localOverrides = [ ];
      }).model.workflows
      ) true
    );

  emptyUnitTask = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.empty-unit" = {
      id = "workflow.test.workflow.errors.empty-unit";
      units.main.taskId = "";
    };
  };

  emptyPreRunTask = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.empty-prerun" = {
      id = "workflow.test.workflow.errors.empty-prerun";
      units.main.taskId = "task.test.workflow.errors.base";
      preRun.tasks = [ "" ];
    };
  };

  emptyStageEntry = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.empty-stage" = {
      id = "workflow.test.workflow.errors.empty-stage";
      stages = [ [ "" ] ];
    };
  };

  dependencyCycle = evalWorkflows {
    nixfied.tasks = {
      "test.workflow.errors.a" = {
        id = "task.test.workflow.errors.a";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "a"
        '';
      };
      "test.workflow.errors.b" = {
        id = "task.test.workflow.errors.b";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "b"
        '';
      };
    };

    nixfied.workflows."test.workflow.errors.cycle" = {
      id = "workflow.test.workflow.errors.cycle";
      units = {
        a = {
          taskId = "task.test.workflow.errors.a";
          needs = [ "b" ];
        };
        b = {
          taskId = "task.test.workflow.errors.b";
          needs = [ "a" ];
        };
      };
    };
  };

  unknownPreRunServiceSet = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.unknown-service-set" = {
      id = "workflow.test.workflow.errors.unknown-service-set";
      units.main.taskId = "task.test.workflow.errors.base";
      preRun.serviceSets = [
        {
          serviceSetId = "missing";
        }
      ];
    };
  };

  unsupportedLockPolicy = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.unsupported-lock-policy" = {
      id = "workflow.test.workflow.errors.unsupported-lock-policy";
      units.main.taskId = "task.test.workflow.errors.base";
      execution.lockPolicy = "shared-aware";
    };
  };
in
assert emptyUnitTask.success == false;
assert emptyPreRunTask.success == false;
assert emptyStageEntry.success == false;
assert dependencyCycle.success == false;
assert unknownPreRunServiceSet.success == false;
assert unsupportedLockPolicy.success == false;
assert pkgs.lib.hasInfix "unit '\${unitName}' has an empty taskId" compilerSource;
assert pkgs.lib.hasInfix "\${phaseName}.tasks references an empty task id" compilerSource;
assert pkgs.lib.hasInfix "\${phaseName}.serviceSets references unknown service set" compilerSource;
assert pkgs.lib.hasInfix "workflow stage entries must not be empty" compilerSource;
assert pkgs.lib.hasInfix "workflow '\${workflowId}' has a dependency cycle" compilerSource;
assert pkgs.lib.hasInfix "\"exclusive\"" workflowModuleSource;
assert (!pkgs.lib.hasInfix "\"shared-aware\"" workflowModuleSource);
assert pkgs.lib.hasInfix "\"exclusive\"" workflowSchemaSource;
assert (!pkgs.lib.hasInfix "\"shared-aware\"" workflowSchemaSource);
pkgs.runCommand "workflow-validation-errors" { } ''
  echo "OK: workflow validation rejects invalid refs, cycles, and unsupported lock policy" > "$out"
''
