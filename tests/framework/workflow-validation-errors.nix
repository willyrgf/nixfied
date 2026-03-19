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
in
assert emptyUnitTask.success == false;
assert emptyPreRunTask.success == false;
assert emptyStageEntry.success == false;
assert dependencyCycle.success == false;
assert pkgs.lib.hasInfix "unit '\${unitName}' has an empty taskId" compilerSource;
assert pkgs.lib.hasInfix "\${phaseName}.tasks references an empty task id" compilerSource;
assert pkgs.lib.hasInfix "workflow stage entries must not be empty" compilerSource;
assert pkgs.lib.hasInfix "workflow '\${workflowId}' has a dependency cycle" compilerSource;
pkgs.runCommand "workflow-validation-errors" { } ''
  echo "OK: workflow validation rejects empty task refs and dependency cycles" > "$out"
''
