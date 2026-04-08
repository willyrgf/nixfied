{
  pkgs,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };

  evalWorkflows =
    extraModule:
    builtins.tryEval (
      builtins.deepSeq (frameworkLib.mkNixfied {
        projectRoot = ../..;
        projectModules = [ ../../nixfied/project/module.nix ];
        extraModules = [ extraModule ];
        localOverrides = [ ];
      }).model.workflows true
    );

  validUnitTask = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.valid-unit" = {
      id = "workflow.test.workflow.errors.valid-unit";
      units.main.taskId = "task.test.workflow.errors.base";
    };
  };

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

  validPreRunTask = evalWorkflows {
    nixfied.tasks = {
      "test.workflow.errors.base" = {
        id = "task.test.workflow.errors.base";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "base"
        '';
      };
      "test.workflow.errors.pre" = {
        id = "task.test.workflow.errors.pre";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "pre"
        '';
      };
    };

    nixfied.workflows."test.workflow.errors.valid-prerun" = {
      id = "workflow.test.workflow.errors.valid-prerun";
      units.main.taskId = "task.test.workflow.errors.base";
      preRun.tasks = [ "task.test.workflow.errors.pre" ];
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

  validStageEntry = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.valid-stage" = {
      id = "workflow.test.workflow.errors.valid-stage";
      stages = [ [ "task.test.workflow.errors.base" ] ];
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

  validDependencyGraph = evalWorkflows {
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

    nixfied.workflows."test.workflow.errors.valid-cycle" = {
      id = "workflow.test.workflow.errors.valid-cycle";
      units = {
        a = {
          taskId = "task.test.workflow.errors.a";
          needs = [ ];
        };
        b = {
          taskId = "task.test.workflow.errors.b";
          needs = [ "a" ];
        };
      };
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

  validPreRunServiceSet = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.valid-service-set" = {
      id = "workflow.test.workflow.errors.valid-service-set";
      units.main.taskId = "task.test.workflow.errors.base";
      preRun.serviceSets = [
        {
          serviceSetId = "service-set.default";
          operation = "status";
        }
      ];
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
          operation = "status";
        }
      ];
    };
  };

  validLockPolicy = evalWorkflows {
    nixfied.tasks."test.workflow.errors.base" = {
      id = "task.test.workflow.errors.base";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "base"
      '';
    };

    nixfied.workflows."test.workflow.errors.valid-lock-policy" = {
      id = "workflow.test.workflow.errors.valid-lock-policy";
      units.main.taskId = "task.test.workflow.errors.base";
      execution.lockPolicy = "exclusive";
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
assert validUnitTask.success;
assert !emptyUnitTask.success;
assert validPreRunTask.success;
assert !emptyPreRunTask.success;
assert validStageEntry.success;
assert !emptyStageEntry.success;
assert validDependencyGraph.success;
assert !dependencyCycle.success;
assert validPreRunServiceSet.success;
assert !unknownPreRunServiceSet.success;
assert validLockPolicy.success;
assert !unsupportedLockPolicy.success;
pkgs.runCommand "workflow-validation-errors" { } ''
  echo "OK: workflow validation distinguishes valid local shapes from invalid refs, cycles, and lock policies" > "$out"
''
