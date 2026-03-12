{
  lib,
  mkCommandTask,
  commonRuntimeInputs,
  nixChecksPkg,
  nixChecksContractArgs,
  defaultTaskPassThroughEnv,
  nixFormatterPkg,
  frameworkInstallPreset,
  frameworkTestPreset,
  frameworkSelfhostPreset,
}:
{
  config = {
    nixfied.tasks = {
      dev = mkCommandTask {
        id = "task.dev";
        appName = "dev";
        summary = "Start the dev workflow";
        description = ''
          Runs the project's dev workflow.

          Customize this command in nixfied/project/tasks.nix.
        '';
        tags = [
          "dev"
          "local"
        ];
        usage = [ "NIX_ENV=0 nix run .#dev" ];
        examples = [ "NIX_ENV=0 nix run .#dev" ];
        command = ''
          set -euo pipefail
          echo "INFO: starting dev workflow"
          echo "SKIP: dev command placeholder. Edit nixfied/project/tasks.nix to run your app."
        '';
      };

      build = mkCommandTask {
        id = "task.build";
        appName = "build";
        summary = "Build artifacts";
        description = ''
          Runs the project's build workflow.

          Customize this command in nixfied/project/tasks.nix.
        '';
        usage = [ "nix run .#build" ];
        command = ''
          set -euo pipefail
          echo "INFO: running build workflow"
          echo "SKIP: build command placeholder. Edit nixfied/project/tasks.nix."
        '';
      };

      check = mkCommandTask {
        id = "task.check";
        appName = "check";
        summary = "Run quality checks";
        description = ''
          Runs reusable Nix quality checks for the repository.
        '';
        usage = [
          "nix run .#check"
          "nix run .#check -- --full"
        ];
        examples = [ "nix run .#check -- --full" ];
        runner = {
          type = "derivation";
          package = nixChecksPkg;
          command = "nix-checks";
          workflowId = null;
        };
        contractArgs = nixChecksContractArgs;
      };

      format = mkCommandTask {
        id = "task.format";
        appName = "format";
        summary = "Format Nix files";
        usage = [ "nix run .#format" ];
        runtimeInputs = commonRuntimeInputs ++ [
          nixFormatterPkg
        ];
        postHooks = {
          "framework.nixfmt" = {
            command = lib.mkDefault ''
              set -euo pipefail
              find . -name '*.nix' -print0 | xargs -0 nixfmt --
              echo "OK: formatted nix files"
            '';
          };
        };
        command = ''
          set -euo pipefail
          echo "INFO: running format task"
        '';
      };

      test = mkCommandTask {
        id = "task.test";
        appName = "test";
        kind = "workflow";
        summary = "Run tests";
        description = "Run tests through the deterministic workflow executor.";
        usage = [ "nix run .#test" ];
        workflowId = "workflow.ci.full";
        contractArgs = [
          {
            name = "summary";
            kind = "flag";
            long = "--summary";
            description = "Print compact summary output.";
          }
          {
            name = "mode";
            kind = "option";
            long = "--mode";
            type = "enum";
            values = [
              "basic"
              "app"
              "env"
              "full"
            ];
            description = "Select workflow mode.";
          }
        ];
      };

      ci = mkCommandTask {
        id = "task.ci";
        appName = "ci";
        kind = "workflow";
        summary = "Run the CI pipeline";
        description = "Runs CI through workflow.ci.<mode> plans.";
        usage = [
          "nix run .#ci"
          "nix run .#ci -- --summary"
        ];
        workflowId = "workflow.ci.full";
        contractArgs = [
          {
            name = "summary";
            kind = "flag";
            long = "--summary";
            description = "Print compact summary output.";
          }
          {
            name = "mode";
            kind = "option";
            long = "--mode";
            type = "enum";
            values = [
              "basic"
              "app"
              "env"
              "full"
            ];
            description = "Select workflow mode.";
          }
          {
            name = "basic";
            kind = "flag";
            long = "--basic";
            description = "Alias for --mode basic.";
          }
          {
            name = "app";
            kind = "flag";
            long = "--app";
            description = "Alias for --mode app.";
          }
          {
            name = "env";
            kind = "flag";
            long = "--env";
            description = "Alias for --mode env.";
          }
          {
            name = "full";
            kind = "flag";
            long = "--full";
            description = "Alias for --mode full.";
          }
        ];
      };

      ci-quality =
        mkCommandTask {
          id = "task.ci.quality";
          appName = "ci-quality";
          kind = "ci-step";
          summary = "Quality checks";
          description = "Reusable Nix quality checks in full mode.";
          tags = [
            "ci"
            "quality"
          ];
          runner = {
            type = "derivation";
            package = nixChecksPkg;
            command = "nix-checks --mode full";
            workflowId = null;
          };
        }
        // {
          ui.app.expose = false;
        };

      ci-tests =
        mkCommandTask {
          id = "task.ci.tests";
          appName = "ci-tests";
          kind = "ci-step";
          summary = "Tests";
          description = "Test CI step.";
          tags = [
            "ci"
            "tests"
          ];
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
            mkdir -p "$artifacts_dir"
            touch "$artifacts_dir/tests.log"
            echo "OK: tests step complete"
          '';
        }
        // {
          ui.app.expose = false;
        };

      ci-system-quick =
        mkCommandTask {
          id = "task.ci.system-quick";
          appName = "ci-system-quick";
          kind = "ci-step";
          summary = "Quick system tests";
          description = "Optional system test gate.";
          tags = [
            "ci"
            "system"
          ];
          runtimeInputs = commonRuntimeInputs;
          passThroughEnv = defaultTaskPassThroughEnv ++ [ "API_KEY" ];
          allowSensitivePassThrough = true;
          command = ''
            set -euo pipefail
            if [ -z "''${API_KEY:-}" ]; then
              echo "SKIP: API_KEY not set"
              exit 0
            fi
            artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
            mkdir -p "$artifacts_dir"
            touch "$artifacts_dir/system-quick.log"
            echo "OK: quick system step complete"
          '';
        }
        // {
          ui.app.expose = false;
        };

      ci-nginx-proxy =
        mkCommandTask {
          id = "task.ci.nginx-proxy";
          appName = "ci-nginx-proxy";
          kind = "ci-step";
          summary = "Nginx proxy test";
          description = "Nginx proxy CI step.";
          tags = [
            "ci"
            "proxy"
          ];
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
            mkdir -p "$artifacts_dir"
            touch "$artifacts_dir/nginx-proxy.log"
            echo "OK: nginx proxy step complete"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-sleep-a =
        mkCommandTask {
          id = "task.test.parallel.sleep-a";
          appName = "test-parallel-sleep-a";
          kind = "internal";
          summary = "Parallel smoke unit A";
          description = "Sleeps for 1 second for workflow scheduler validation.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "INFO: parallel smoke sleep-a start"
            sleep 1
            echo "OK: parallel smoke sleep-a done"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-sleep-b =
        mkCommandTask {
          id = "task.test.parallel.sleep-b";
          appName = "test-parallel-sleep-b";
          kind = "internal";
          summary = "Parallel smoke unit B";
          description = "Sleeps for 1 second for workflow scheduler validation.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "INFO: parallel smoke sleep-b start"
            sleep 1
            echo "OK: parallel smoke sleep-b done"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-sleep-c =
        mkCommandTask {
          id = "task.test.parallel.sleep-c";
          appName = "test-parallel-sleep-c";
          kind = "internal";
          summary = "Parallel smoke dependent unit";
          description = "Sleeps for 1 second and depends on unit A in smoke workflow.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "INFO: parallel smoke sleep-c start"
            sleep 1
            echo "OK: parallel smoke sleep-c done"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-sleep-d =
        mkCommandTask {
          id = "task.test.parallel.sleep-d";
          appName = "test-parallel-sleep-d";
          kind = "internal";
          summary = "Parallel smoke lock unit";
          description = "Sleeps for 1 second and shares lock with unit B.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "INFO: parallel smoke sleep-d start"
            sleep 1
            echo "OK: parallel smoke sleep-d done"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-skip =
        mkCommandTask {
          id = "task.test.parallel.skip";
          appName = "test-parallel-skip";
          kind = "internal";
          summary = "Parallel smoke when-skip unit";
          description = "No-op task canceled by when.envPresent in smoke workflow.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "WARN: parallel smoke skip task should not run"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-fail =
        mkCommandTask {
          id = "task.test.parallel.fail";
          appName = "test-parallel-fail";
          kind = "internal";
          summary = "Parallel fail-fast trigger";
          description = "Fails intentionally for fail-fast workflow validation.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "ERROR: intentional fail-fast trigger"
            sleep 1
            exit 7
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-slow-a =
        mkCommandTask {
          id = "task.test.parallel.slow-a";
          appName = "test-parallel-slow-a";
          kind = "internal";
          summary = "Parallel fail-fast slow unit A";
          description = "Long-running unit that should be canceled by fail-fast.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "INFO: fail-fast slow-a start"
            sleep 10
            echo "OK: fail-fast slow-a done"
          '';
        }
        // {
          ui.app.expose = false;
        };

      test-parallel-slow-b =
        mkCommandTask {
          id = "task.test.parallel.slow-b";
          appName = "test-parallel-slow-b";
          kind = "internal";
          summary = "Parallel fail-fast slow unit B";
          description = "Long-running unit that should be canceled by fail-fast.";
          runtimeInputs = commonRuntimeInputs;
          command = ''
            set -euo pipefail
            echo "INFO: fail-fast slow-b start"
            sleep 10
            echo "OK: fail-fast slow-b done"
          '';
        }
        // {
          ui.app.expose = false;
        };
    }
    // frameworkInstallPreset.tasks
    // frameworkTestPreset.tasks
    // frameworkSelfhostPreset.tasks;
  };
}
