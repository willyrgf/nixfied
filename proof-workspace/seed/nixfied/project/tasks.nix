{
  pkgs,
  conf,
  commonRuntimeInputs,
  ...
}:
let
  project = conf.project;
  commonPassThroughEnv = [
    project.envVar
    project.slotVar
    "CI_MAX_WORKERS"
    "NIXFIED_CI_MAX_WORKERS"
    "NIXFIED_PARALLEL_SMOKE"
    "NIXFIED_PARALLEL_SKIP"
  ];

  mkShellTask =
    {
      id,
      summary,
      description,
      command,
      kind ? "internal",
      passThroughEnv ? commonPassThroughEnv,
    }:
    {
      inherit
        id
        kind
        summary
        description
        ;
      runner = {
        type = "shell";
        inherit command;
      };
      runtime = {
        slotEnv = "optional";
        workdir = "projectRoot";
        hermetic = true;
        runtimeInputs = commonRuntimeInputs;
        inherit passThroughEnv;
      };
      scheduling = {
        locks = [ ];
        maxAttempts = 1;
        retryBackoffSec = [ ];
        priority = 100;
      };
      deps = {
        needs = [ ];
        softNeeds = [ ];
      };
      produces = {
        artifacts = [ ];
        stateKeys = [ ];
      };
    };
in
{
  config = {
    nixfied.tasks = {
      "framework-install" = {
        id = "task.framework.install";
        kind = "utility";
        summary = "Install thin or vendored wrapper flake";
        description = "Publishes the framework::install help contract for installed proof workspaces.";
        runner = {
          type = "shell";
          command = ''
            set -euo pipefail
            echo "ERROR: use nix run .#framework::install"
            exit 2
          '';
        };
        commandApi = {
          summary = "Install thin or vendored wrapper flake";
          details = "Creates a thin wrapper by default, or a vendored wrapper with --vendor.";
          usage = [
            "nix run .#framework::install"
            "nix run .#framework::install -- --vendor"
            "nix run .#framework::install -- --vendor --target ."
            "nix run .#framework::install -- --vendor --upgrade --target ."
          ];
          category = "framework";
          args = [
            {
              name = "vendor";
              kind = "flag";
              long = "--vendor";
              description = "Generate a vendored wrapper flake.";
            }
            {
              name = "target";
              kind = "option";
              long = "--target";
              type = "string";
              description = "Output directory for generated wrapper.";
            }
            {
              name = "upgrade";
              kind = "flag";
              long = "--upgrade";
              description = "Upgrade vendored framework files in-place and preserve nixfied/project + nixfied/local.";
            }
            {
              name = "reset-project";
              kind = "flag";
              long = "--reset-project";
              description = "When vendoring, overwrite nixfied/project.";
            }
            {
              name = "reset-local";
              kind = "flag";
              long = "--reset-local";
              description = "When vendoring, overwrite nixfied/local.";
            }
          ];
        };
        runtime = {
          slotEnv = "optional";
          workdir = "projectRoot";
          hermetic = true;
          runtimeInputs = commonRuntimeInputs;
          passThroughEnv = commonPassThroughEnv;
        };
      };

      "framework-upgrade" = {
        id = "task.framework.upgrade";
        kind = "utility";
        summary = "Upgrade vendored wrapper in-place";
        description = "Publishes the framework::upgrade help contract for installed proof workspaces.";
        runner = {
          type = "shell";
          command = ''
            set -euo pipefail
            echo "ERROR: use nix run .#framework::upgrade"
            exit 2
          '';
        };
        commandApi = {
          summary = "Upgrade vendored wrapper in-place";
          details = "Upgrades framework files while preserving nixfied/project and nixfied/local by default.";
          usage = [
            "nix run .#framework::upgrade -- --target ."
            "nix run .#framework::upgrade -- --target . --reset-project"
            "nix run .#framework::upgrade -- --target . --reset-local"
          ];
          category = "framework";
          args = [
            {
              name = "vendor";
              kind = "flag";
              long = "--vendor";
              description = "Generate a vendored wrapper flake (default for framework::upgrade).";
            }
            {
              name = "target";
              kind = "option";
              long = "--target";
              type = "string";
              description = "Output directory for generated wrapper.";
            }
            {
              name = "reset-project";
              kind = "flag";
              long = "--reset-project";
              description = "When vendoring, overwrite nixfied/project.";
            }
            {
              name = "reset-local";
              kind = "flag";
              long = "--reset-local";
              description = "When vendoring, overwrite nixfied/local.";
            }
          ];
        };
        runtime = {
          slotEnv = "optional";
          workdir = "projectRoot";
          hermetic = true;
          runtimeInputs = commonRuntimeInputs;
          passThroughEnv = commonPassThroughEnv;
        };
      };

      "test-isolation-unit" = mkShellTask {
        id = "task.test.isolation.unit";
        summary = "Isolation probe unit";
        description = "Lightweight probe body used by test-isolation.";
        command = ''
          set -euo pipefail
          artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
          mkdir -p "$artifacts_dir"
          {
            printf 'slot=%s\n' "''${NIX_ENV:-}"
            printf 'env=%s\n' "''${PROJECT_ENV:-}"
            printf 'registry_root=%s\n' "''${REGISTRY_ROOT:-}"
            printf 'artifacts_dir=%s\n' "''${CI_ARTIFACTS_DIR:-}"
          } > "$artifacts_dir/isolation-probe.txt"
          echo "OK: isolation probe complete slot=''${NIX_ENV:-} env=''${PROJECT_ENV:-}"
        '';
      };

      "test-isolation-probe" = {
        id = "task.test.isolation.probe";
        kind = "workflow";
        summary = "Isolation probe";
        description = "Workflow-backed probe used by test-isolation.";
        runner = {
          type = "workflowRef";
          workflowId = "workflow.test.isolation.probe";
        };
      };

      "test-parallel-sleep-a" = mkShellTask {
        id = "task.test.parallel.sleep-a";
        summary = "Parallel smoke unit A";
        description = "Sleeps for scheduler validation.";
        command = ''
          set -euo pipefail
          echo "INFO: parallel smoke sleep-a start"
          sleep 1
          echo "OK: parallel smoke sleep-a done"
        '';
      };

      "test-parallel-sleep-b" = mkShellTask {
        id = "task.test.parallel.sleep-b";
        summary = "Parallel smoke unit B";
        description = "Sleeps for scheduler validation.";
        command = ''
          set -euo pipefail
          echo "INFO: parallel smoke sleep-b start"
          sleep 1
          echo "OK: parallel smoke sleep-b done"
        '';
      };

      "test-parallel-sleep-c" = mkShellTask {
        id = "task.test.parallel.sleep-c";
        summary = "Parallel smoke unit C";
        description = "Dependent parallel smoke unit.";
        command = ''
          set -euo pipefail
          echo "INFO: parallel smoke sleep-c start"
          sleep 1
          echo "OK: parallel smoke sleep-c done"
        '';
      };

      "test-parallel-sleep-d" = mkShellTask {
        id = "task.test.parallel.sleep-d";
        summary = "Parallel smoke unit D";
        description = "Lock-sharing parallel smoke unit.";
        command = ''
          set -euo pipefail
          echo "INFO: parallel smoke sleep-d start"
          sleep 1
          echo "OK: parallel smoke sleep-d done"
        '';
      };

      "test-parallel-skip" = mkShellTask {
        id = "task.test.parallel.skip";
        summary = "Parallel smoke skip unit";
        description = "Task canceled by missing when-env.";
        command = ''
          set -euo pipefail
          echo "WARN: parallel smoke skip task should not run"
        '';
      };

      "test-parallel-fail" = mkShellTask {
        id = "task.test.parallel.fail";
        summary = "Parallel fail-fast trigger";
        description = "Fails intentionally for fail-fast validation.";
        command = ''
          set -euo pipefail
          echo "ERROR: intentional fail-fast trigger"
          sleep 1
          exit 7
        '';
      };

      "test-parallel-slow-a" = mkShellTask {
        id = "task.test.parallel.slow-a";
        summary = "Parallel slow unit A";
        description = "Long-running unit canceled by fail-fast.";
        passThroughEnv = commonPassThroughEnv ++ [ "NIXFIED_PARALLEL_CHILD_LEAK_DIR" ];
        command = ''
          set -euo pipefail
          echo "INFO: fail-fast slow-a start"
          if [ -n "''${NIXFIED_PARALLEL_CHILD_LEAK_DIR:-}" ]; then
            mkdir -p "$NIXFIED_PARALLEL_CHILD_LEAK_DIR"
            (
              trap "" TERM INT
              while true; do
                sleep 1
              done
            ) &
            child_pid="$!"
            printf '%s\n' "$child_pid" > "$NIXFIED_PARALLEL_CHILD_LEAK_DIR/slow-a.pid"
            echo "INFO: fail-fast slow-a child pid=$child_pid"
          fi
          sleep 10
          echo "OK: fail-fast slow-a done"
        '';
      };

      "test-parallel-slow-b" = mkShellTask {
        id = "task.test.parallel.slow-b";
        summary = "Parallel slow unit B";
        description = "Long-running unit canceled by fail-fast.";
        command = ''
          set -euo pipefail
          echo "INFO: fail-fast slow-b start"
          sleep 10
          echo "OK: fail-fast slow-b done"
        '';
      };
    };
  };
}
