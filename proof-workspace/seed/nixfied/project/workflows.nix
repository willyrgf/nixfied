_:
let
  serviceWorkflowPhases = {
    preRun = {
      tasks = [ "task.test.workflow.service.ready" ];
      serviceSets = [
        {
          serviceSetId = "service-set.default";
          operation = "start";
        }
      ];
    };
    postRun = {
      tasks = [ ];
      serviceSets = [
        {
          serviceSetId = "service-set.default";
          operation = "health";
        }
      ];
      alwaysRun = true;
    };
  };

  noPhaseTasks = {
    preRun = {
      tasks = [ ];
    };
    postRun = {
      tasks = [ ];
      alwaysRun = true;
    };
  };
in
{
  config = {
    nixfied.workflows = {
      "test-isolation-probe" = {
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
        inherit (noPhaseTasks) preRun;
        inherit (noPhaseTasks) postRun;
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

      "test-service-phase-probe" = {
        id = "workflow.test.service.phase.probe";
        summary = "Workflow service phase proof";
        description = "Proves workflow preRun/postRun service-set phases over enabled proof services.";
        mode = "custom";
        maxWorkers = 1;
        units = {
          marker = {
            taskId = "task.test.workflow.marker";
            needs = [ ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ "PROOF_WORKFLOW_PHASE_LOG_FILE" ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [ ];
        inherit (serviceWorkflowPhases) preRun;
        inherit (serviceWorkflowPhases) postRun;
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
        };
      };

      "test-parallel-smoke" = {
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
        inherit (noPhaseTasks) preRun;
        inherit (noPhaseTasks) postRun;
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

      "test-parallel-failfast" = {
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
        inherit (noPhaseTasks) preRun;
        inherit (noPhaseTasks) postRun;
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
    };
  };
}
