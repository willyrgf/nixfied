{ project, ... }:

{
  commands.ci = {
    description = "CI DSL fixture";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    script = ''
      echo "CI DSL fixture command (overridden by DSL app)."
    '';
  };

  ci = {
    enable = true;
    defaultMode = "basic";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    setupActions = [ ];
    teardownActions = [
      {
        kind = "artifactTouch";
        artifact = "teardown.ok";
      }
    ];
    artifacts = {
      dir = ".ci-artifacts";
      keepOnFailure = true;
      keepOnSuccess = true;
    };
    modes = {
      basic = {
        steps = [
          "runs"
          "skip-missing"
          "when-false"
          "runs-second"
        ];
      };
      failure = {
        steps = [ "fail-with-cleanup" ];
      };
      sequential-legacy = {
        steps = [
          "seq-one"
          "seq-two"
        ];
      };
      staged = {
        parallel = {
          maxWorkers = 2;
        };
        stages = [
          [
            "parallel-one"
            "parallel-two"
          ]
          [ "stage-barrier" ]
        ];
      };
      locked = {
        parallel = {
          maxWorkers = 2;
        };
        stages = [
          [
            "lock-one"
            "lock-two"
          ]
        ];
      };
      worker-cap = {
        parallel = {
          maxWorkers = 2;
        };
        stages = [
          [
            "cap-one"
            "cap-two"
            "cap-three"
          ]
        ];
      };
      cancel = {
        parallel = {
          maxWorkers = 3;
        };
        stages = [
          [
            "cancel-fail"
            "cancel-long"
          ]
          [ "cancel-tail" ]
        ];
      };
    };
    steps = {
      runs = {
        description = "Basic step runs";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "runs.ok";
          }
        ];
      };
      skip-missing = {
        description = "Skipped when env var missing";
        skipIfMissing = [ "CI_MISSING" ];
        actions = [
          {
            kind = "artifactTouch";
            artifact = "skip-missing.ok";
          }
        ];
      };
      when-false = {
        description = "Skipped when condition false";
        when = {
          envEquals = {
            "${project.envVar}" = "dev";
          };
        };
        actions = [
          {
            kind = "artifactTouch";
            artifact = "when.ok";
          }
        ];
      };
      runs-second = {
        description = "Second step runs";
        fixtures = {
          env = {
            FIXTURE_STEP_ENV = "from-fixture";
          };
        };
        actions = [
          {
            kind = "assertEnvEquals";
            name = "FIXTURE_STEP_ENV";
            value = "from-fixture";
          }
          {
            kind = "artifactTouch";
            artifact = "runs-second.ok";
          }
        ];
      };
      fail-with-cleanup = {
        description = "Cleanup runs on failure";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "fail.ran";
          }
          {
            kind = "fail";
            code = 1;
          }
        ];
        cleanupActions = [
          {
            kind = "artifactTouch";
            artifact = "fail.cleanup";
          }
        ];
      };
      seq-one = {
        description = "Legacy sequential first step";
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "1"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "seq-one.ok";
          }
        ];
      };
      seq-two = {
        description = "Legacy sequential second step";
        actions = [
          {
            kind = "exec";
            argv = [
              "sh"
              "-c"
              "[ -f \"$CI_ARTIFACTS_DIR/seq-one.ok\" ]"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "seq-two.ok";
          }
        ];
      };
      parallel-one = {
        description = "Parallel stage step one";
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "2"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "parallel-one.ok";
          }
        ];
      };
      parallel-two = {
        description = "Parallel stage step two";
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "2"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "parallel-two.ok";
          }
        ];
      };
      stage-barrier = {
        description = "Stage barrier validation step";
        actions = [
          {
            kind = "exec";
            argv = [
              "sh"
              "-c"
              "[ -f \"$CI_ARTIFACTS_DIR/parallel-one.ok\" ] && [ -f \"$CI_ARTIFACTS_DIR/parallel-two.ok\" ]"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "stage-barrier.ok";
          }
        ];
      };
      lock-one = {
        description = "Locked parallel step one";
        locks = [ "shared-lock" ];
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "1"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "lock-one.ok";
          }
        ];
      };
      lock-two = {
        description = "Locked parallel step two";
        locks = [ "shared-lock" ];
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "1"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "lock-two.ok";
          }
        ];
      };
      cap-one = {
        description = "Worker cap step one";
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "1"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "cap-one.ok";
          }
        ];
      };
      cap-two = {
        description = "Worker cap step two";
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "1"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "cap-two.ok";
          }
        ];
      };
      cap-three = {
        description = "Worker cap step three";
        actions = [
          {
            kind = "exec";
            argv = [
              "sleep"
              "1"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "cap-three.ok";
          }
        ];
      };
      cancel-fail = {
        description = "Step that fails to trigger cancellation";
        actions = [
          {
            kind = "exec";
            argv = [
              "sh"
              "-c"
              "sleep 1"
            ];
          }
          {
            kind = "fail";
            code = 7;
          }
        ];
        cleanupActions = [
          {
            kind = "artifactTouch";
            artifact = "cancel-fail.cleanup";
          }
        ];
      };
      cancel-long = {
        description = "Long-running step canceled by fail-fast";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "cancel-long.started";
          }
          {
            kind = "exec";
            argv = [
              "sleep"
              "30"
            ];
          }
          {
            kind = "artifactTouch";
            artifact = "cancel-long.done";
          }
        ];
        cleanupActions = [
          {
            kind = "artifactTouch";
            artifact = "cancel-long.cleanup";
          }
        ];
      };
      cancel-tail = {
        description = "Tail step that should never run after failure";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "cancel-tail.ok";
          }
        ];
      };
    };
  };
}
