{
  lib,
  mkCommandTask,
  mkTaskLauncher,
  commonRuntimeInputs,
  nixChecksPkg,
  nixChecksContractArgs,
  defaultTaskPassThroughEnv,
  nixFormatterPkg,
  frameworkInstallPreset,
}:
let
  tasks = {
    dev = mkCommandTask {
      id = "task.dev";
      summary = "Start the dev workflow";
      description = ''
        Runs the project's dev workflow.

        Customize this command in nixfied/project/tasks.nix.
      '';
      tags = [
        "dev"
        "local"
      ];
      command = ''
        set -euo pipefail
        echo "INFO: starting dev workflow"
        echo "SKIP: dev command placeholder. Edit nixfied/project/tasks.nix to run your app."
      '';
      launcher = mkTaskLauncher {
        appId = "dev";
      };
    };

    build = mkCommandTask {
      id = "task.build";
      summary = "Build artifacts";
      description = ''
        Runs the project's build workflow.

        Customize this command in nixfied/project/tasks.nix.
      '';
      command = ''
        set -euo pipefail
        echo "INFO: running build workflow"
        echo "SKIP: build command placeholder. Edit nixfied/project/tasks.nix."
      '';
      launcher = mkTaskLauncher {
        appId = "build";
      };
    };

    check = mkCommandTask {
      id = "task.check";
      summary = "Run quality checks";
      description = ''
        Runs reusable Nix quality checks for the repository.
      '';
      runner = {
        type = "derivation";
        package = nixChecksPkg;
        command = ''nix-checks "$@"'';
        workflowId = null;
      };
      contractArgs = nixChecksContractArgs;
      launcher = mkTaskLauncher {
        appId = "check";
        usage = [
          "nix run .#check"
          "nix run .#check -- --full"
        ];
        examples = [ "nix run .#check -- --full" ];
      };
    };

    format = mkCommandTask {
      id = "task.format";
      summary = "Format Nix files";
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
      launcher = mkTaskLauncher {
        appId = "format";
      };
    };

    test = mkCommandTask {
      id = "task.test";
      kind = "workflow";
      summary = "Run tests";
      description = "Run tests through the deterministic workflow executor.";
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
      launcher = mkTaskLauncher {
        appId = "test";
      };
    };

    ci = mkCommandTask {
      id = "task.ci";
      kind = "workflow";
      summary = "Run the CI pipeline";
      description = "Runs CI through workflow.ci.<mode> plans.";
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
      launcher = mkTaskLauncher {
        appId = "ci";
        usage = [
          "nix run .#ci"
          "nix run .#ci -- --summary"
        ];
      };
    };

    ci-quality = mkCommandTask {
      id = "task.ci.quality";
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
    };

    ci-tests = mkCommandTask {
      id = "task.ci.tests";
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
    };

    ci-system-quick = mkCommandTask {
      id = "task.ci.system-quick";
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
    };

    ci-nginx-proxy = mkCommandTask {
      id = "task.ci.nginx-proxy";
      kind = "ci-step";
      summary = "Nginx proxy test";
      description = "Nginx proxy CI step.";
      tags = [
        "ci"
        "proxy"
      ];
      runtimeInputs = commonRuntimeInputs;
      passThroughEnv = defaultTaskPassThroughEnv ++ [ "API_KEY" ];
      allowSensitivePassThrough = true;
      command = ''
        set -euo pipefail
        artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
        mkdir -p "$artifacts_dir"
        touch "$artifacts_dir/nginx-proxy.log"
        echo "OK: nginx proxy step complete"
      '';
    };

    test-isolation-unit = mkCommandTask {
      id = "task.test.isolation.unit";
      kind = "internal";
      summary = "Isolation probe unit";
      description = "Lightweight probe body used by test-isolation to validate slot/env scoping without rerunning full CI.";
      runtimeInputs = commonRuntimeInputs;
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

    test-isolation-probe = mkCommandTask {
      id = "task.test.isolation.probe";
      kind = "workflow";
      summary = "Isolation probe";
      description = "Workflow-backed probe used by test-isolation so each cell emits a summary without rerunning full CI.";
      workflowId = "workflow.test.isolation.probe";
    };

    test-parallel-sleep-a = mkCommandTask {
      id = "task.test.parallel.sleep-a";
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
    };

    test-parallel-sleep-b = mkCommandTask {
      id = "task.test.parallel.sleep-b";
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
    };

    test-parallel-sleep-c = mkCommandTask {
      id = "task.test.parallel.sleep-c";
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
    };

    test-parallel-sleep-d = mkCommandTask {
      id = "task.test.parallel.sleep-d";
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
    };

    test-parallel-skip = mkCommandTask {
      id = "task.test.parallel.skip";
      kind = "internal";
      summary = "Parallel smoke when-skip unit";
      description = "No-op task canceled by when.envPresent in smoke workflow.";
      runtimeInputs = commonRuntimeInputs;
      command = ''
        set -euo pipefail
        echo "WARN: parallel smoke skip task should not run"
      '';
    };

    test-parallel-fail = mkCommandTask {
      id = "task.test.parallel.fail";
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
    };

    test-parallel-slow-a = mkCommandTask {
      id = "task.test.parallel.slow-a";
      kind = "internal";
      summary = "Parallel fail-fast slow unit A";
      description = "Long-running unit that should be canceled by fail-fast.";
      passThroughEnv = defaultTaskPassThroughEnv ++ [ "NIXFIED_PARALLEL_CHILD_LEAK_DIR" ];
      runtimeInputs = commonRuntimeInputs;
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

    test-parallel-slow-b = mkCommandTask {
      id = "task.test.parallel.slow-b";
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
    };
  };

in
{
  config = {
    nixfied.tasks = tasks // frameworkInstallPreset.tasks;
  };
}
