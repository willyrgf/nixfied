{ frameworkSelfhostPreset }:
let
  workflowProbePhases = {
    preRun.serviceSets = [
      {
        serviceSetId = "service-set.default";
        operation = "ready";
      }
    ];
    postRun = {
      serviceSets = [
        {
          serviceSetId = "service-set.default";
          operation = "health";
        }
      ];
      alwaysRun = true;
    };
  };
in
{
  config = {
    nixfied.workflows = {
      ci-basic = {
        id = "workflow.ci.basic";
        summary = "Basic CI workflow";
        description = "Runs quality and tests.";
        mode = "ci";
        maxWorkers = 4;
        units = {
          quality = {
            taskId = "task.ci.quality";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          tests = {
            taskId = "task.ci.tests";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [ ];
        preRun = workflowProbePhases.preRun;
        postRun = workflowProbePhases.postRun;
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = true;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral.enable = true;
        };
      };

      ci-app = {
        id = "workflow.ci.app";
        summary = "App CI workflow";
        description = "Basic workflow plus quick system checks.";
        mode = "ci";
        maxWorkers = 4;
        units = {
          quality = {
            taskId = "task.ci.quality";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          tests = {
            taskId = "task.ci.tests";
            needs = [ "quality" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          system-quick = {
            taskId = "task.ci.system-quick";
            needs = [ "tests" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ "API_KEY" ];
          };
        };
        stages = [ ];
        preRun = workflowProbePhases.preRun;
        postRun = workflowProbePhases.postRun;
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = true;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral.enable = true;
        };
      };

      ci-env = {
        id = "workflow.ci.env";
        summary = "Environment CI workflow";
        description = "App workflow plus nginx proxy checks.";
        mode = "ci";
        maxWorkers = 4;
        units = {
          quality = {
            taskId = "task.ci.quality";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          tests = {
            taskId = "task.ci.tests";
            needs = [ "quality" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          system-quick = {
            taskId = "task.ci.system-quick";
            needs = [ "tests" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ "API_KEY" ];
          };
          nginx-proxy = {
            taskId = "task.ci.nginx-proxy";
            needs = [ "system-quick" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [ ];
        preRun = workflowProbePhases.preRun;
        postRun = workflowProbePhases.postRun;
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = true;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral.enable = true;
        };
      };

      ci-full = {
        id = "workflow.ci.full";
        summary = "Full CI workflow";
        description = "Runs quality/tests then system checks as deterministic stages.";
        mode = "ci";
        maxWorkers = 2;
        units = { };
        stages = [
          [
            "task.ci.quality"
            "task.ci.tests"
          ]
          [
            "task.ci.system-quick"
            "task.ci.nginx-proxy"
          ]
        ];
        preRun = workflowProbePhases.preRun;
        postRun = workflowProbePhases.postRun;
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = true;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral.enable = true;
        };
      };

      test-isolation-probe = {
        id = "workflow.test.isolation.probe";
        summary = "Isolation probe workflow";
        description = "Runs a single lightweight unit so test-isolation can emit workflow summaries per cell.";
        mode = "custom";
        maxWorkers = 1;
        units = {
          probe = {
            taskId = "task.test.isolation.unit";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [ ];
        preRun = {
          tasks = [ ];
        };
        postRun = {
          tasks = [ ];
          alwaysRun = true;
        };
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = false;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral.enable = true;
        };
      };

      test-parallel-smoke = {
        id = "workflow.test.parallel.smoke";
        summary = "Parallel runner smoke workflow";
        description = "Validates worker cap, dependency gating, locks, and when behavior.";
        mode = "custom";
        maxWorkers = 2;
        units = {
          alpha = {
            taskId = "task.test.parallel.sleep-a";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ "NIXFIED_PARALLEL_SMOKE" ];
            };
            skipIfMissingEnv = [ ];
          };
          beta = {
            taskId = "task.test.parallel.sleep-b";
            needs = [ ];
            locks = [ "smoke-lock" ];
            when = {
              envEquals = {
                NIXFIED_PARALLEL_SMOKE = "1";
              };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          gamma = {
            taskId = "task.test.parallel.sleep-c";
            needs = [ "alpha" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          delta = {
            taskId = "task.test.parallel.sleep-d";
            needs = [ ];
            locks = [ "smoke-lock" ];
            when = {
              envEquals = { };
              envPresent = [ "NIXFIED_PARALLEL_SMOKE" ];
            };
            skipIfMissingEnv = [ ];
          };
          skip = {
            taskId = "task.test.parallel.skip";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ "NIXFIED_PARALLEL_SKIP" ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [ ];
        preRun = workflowProbePhases.preRun;
        postRun = workflowProbePhases.postRun;
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
        };
      };

      test-parallel-failfast = {
        id = "workflow.test.parallel.failfast";
        summary = "Parallel runner fail-fast workflow";
        description = "Validates fail-fast cancellation of running and pending units.";
        mode = "custom";
        maxWorkers = 3;
        units = {
          fail = {
            taskId = "task.test.parallel.fail";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          slow-a = {
            taskId = "task.test.parallel.slow-a";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          slow-b = {
            taskId = "task.test.parallel.slow-b";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          after = {
            taskId = "task.test.parallel.sleep-c";
            needs = [ "slow-a" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [ ];
        preRun = workflowProbePhases.preRun;
        postRun = workflowProbePhases.postRun;
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
        };
      };
    }
    // frameworkSelfhostPreset.workflows;
  };
}
