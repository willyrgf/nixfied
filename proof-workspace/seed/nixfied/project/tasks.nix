{
  conf,
  commonRuntimeInputs,
  ...
}:
let
  inherit (conf) project;
  commonPassThroughEnv = [
    project.envVar
    project.slotVar
    "CI_MAX_WORKERS"
    "NIXFIED_CI_MAX_WORKERS"
    "NIXFIED_PARALLEL_SMOKE"
    "NIXFIED_PARALLEL_SKIP"
    "PROOF_HOOK_LOG_FILE"
    "PROOF_WORKFLOW_PHASE_LOG_FILE"
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
  config.nixfied = {
    contracts.definitions."machineOutput.result" = {
      kind = "record";
      doc = "Validated machine-output payload.";
      fields = {
        ok = {
          schema = {
            kind = "bool";
          };
        };
        kind = {
          schema = {
            kind = "enum";
            values = [
              "task"
              "workflow"
            ];
          };
        };
      };
    };

    tasks = {
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

      "test-hooks-order" =
        let
          baseTask = mkShellTask {
            id = "task.test.hooks.order";
            summary = "Task hook order proof";
            description = "Writes a deterministic hook execution order log through runtime pre/post hooks.";
            command = ''
              set -euo pipefail
              : "''${PROOF_HOOK_LOG_FILE:?PROOF_HOOK_LOG_FILE is required}"
              printf 'main\n' >> "$PROOF_HOOK_LOG_FILE"
              echo "OK: hook order main complete"
            '';
            passThroughEnv = commonPassThroughEnv ++ [ "PROOF_HOOK_LOG_FILE" ];
          };
        in
        baseTask
        // {
          runtime =
            baseTask.runtime
            // {
              preHooks = {
                "01.pre" = {
                  command = ''
                    set -euo pipefail
                    : "''${PROOF_HOOK_LOG_FILE:?PROOF_HOOK_LOG_FILE is required}"
                    printf 'pre-1\n' >> "$PROOF_HOOK_LOG_FILE"
                  '';
                };
                "02.pre" = {
                  command = ''
                    set -euo pipefail
                    : "''${PROOF_HOOK_LOG_FILE:?PROOF_HOOK_LOG_FILE is required}"
                    printf 'pre-2\n' >> "$PROOF_HOOK_LOG_FILE"
                  '';
                };
              };
              postHooks = {
                "10.post" = {
                  command = ''
                    set -euo pipefail
                    : "''${PROOF_HOOK_LOG_FILE:?PROOF_HOOK_LOG_FILE is required}"
                    printf 'post\n' >> "$PROOF_HOOK_LOG_FILE"
                  '';
                };
              };
            };
        };

      "test-workflow-marker" = mkShellTask {
        id = "task.test.workflow.marker";
        summary = "Workflow phase marker";
        description = "Writes a marker between workflow service phases.";
        command = ''
          set -euo pipefail
          : "''${PROOF_WORKFLOW_PHASE_LOG_FILE:?PROOF_WORKFLOW_PHASE_LOG_FILE is required}"
          printf 'unit\n' >> "$PROOF_WORKFLOW_PHASE_LOG_FILE"
          echo "OK: workflow marker written"
        '';
        passThroughEnv = commonPassThroughEnv ++ [ "PROOF_WORKFLOW_PHASE_LOG_FILE" ];
      };

      "test-workflow-service-ready" = mkShellTask {
        id = "task.test.workflow.service.ready";
        summary = "Workflow service ready proof";
        description = "Runs proof service ready checks within a workflow run.";
        command = ''
          set -euo pipefail
          if [ -z "''${NIXFIED_RUNTIME_BIN:-}" ]; then
            echo "ERROR: NIXFIED_RUNTIME_BIN is required"
            exit 1
          fi
          "$NIXFIED_RUNTIME_BIN" run-service helios ready
          "$NIXFIED_RUNTIME_BIN" run-service minio ready
          "$NIXFIED_RUNTIME_BIN" run-service nginx ready
          "$NIXFIED_RUNTIME_BIN" run-service postgres ready
          "$NIXFIED_RUNTIME_BIN" run-service reth ready
          echo "OK: workflow service readiness complete"
        '';
        passThroughEnv = commonPassThroughEnv ++ [ "PROOF_WORKFLOW_PHASE_LOG_FILE" ];
      };

      "test-runtime-owned-pass-through-blocked" = mkShellTask {
        id = "task.test.runtime-owned.pass-through";
        summary = "Runtime-owned pass-through blocked task";
        description = "Proof task that must fail before execution when HOME is requested as pass-through.";
        command = ''
          set -euo pipefail
          echo "OK: runtime-owned pass-through should not execute"
        '';
        passThroughEnv = commonPassThroughEnv ++ [ "HOME" ];
      };

      "test-runtime-owned-env-blocked" =
        let
          baseTask = mkShellTask {
            id = "task.test.runtime-owned.env";
            summary = "Runtime-owned env override blocked task";
            description = "Proof task that must fail before execution when HOME is overridden.";
            command = ''
              set -euo pipefail
              echo "OK: runtime-owned env override should not execute"
            '';
          };
        in
        baseTask
        // {
          runtime =
            baseTask.runtime
            // {
              env = {
                HOME = "/tmp/forbidden-home";
              };
            };
        };

      "test-runtime-owned-scope-pass-through-blocked" = mkShellTask {
        id = "task.test.runtime-owned.scope-pass-through";
        summary = "Runtime-owned scope pass-through blocked task";
        description = "Proof task that must fail before execution when runtime scope override is requested.";
        command = ''
          set -euo pipefail
          echo "OK: runtime-owned scope pass-through should not execute"
        '';
        passThroughEnv = commonPassThroughEnv ++ [ "NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE" ];
      };

      "test-sensitive-pass-through-blocked" = mkShellTask {
        id = "task.test.sensitive.blocked";
        summary = "Sensitive pass-through blocked task";
        description = "Proof task that must fail before execution when a sensitive env var is requested without opt-in.";
        command = ''
          set -euo pipefail
          echo "OK: blocked task command should not execute"
        '';
        passThroughEnv = commonPassThroughEnv ++ [ "API_KEY" ];
      };

      "test-sensitive-pass-through-allowed" =
        let
          baseTask = mkShellTask {
            id = "task.test.sensitive.allowed";
            summary = "Sensitive pass-through allowed task";
            description = "Proof task that receives a sensitive env var only when explicit opt-in is enabled.";
            command = ''
              set -euo pipefail
              if [ -z "''${API_KEY:-}" ]; then
                echo "ERROR: API_KEY missing inside allowed task"
                exit 1
              fi
              echo "OK: allowed task received API_KEY"
            '';
            passThroughEnv = commonPassThroughEnv ++ [ "API_KEY" ];
          };
        in
        baseTask
        // {
          runtime =
            baseTask.runtime
            // {
              allowSensitivePassThrough = true;
            };
        };

      "test-machine-output-json-body" =
        let
          baseTask = mkShellTask {
            id = "task.test.machine-output.json-body";
            summary = "Machine-output JSON task";
            description = "Emits strict JSON to the declared machine-output channel.";
            command = ''
              set -euo pipefail
              if [ -n "''${NIXFIED_MACHINE_OUTPUT_FILE:-}" ]; then
                printf '%s\n' "json-body human log"
                printf '%s\n' '{"ok":true,"kind":"task"}' > "$NIXFIED_MACHINE_OUTPUT_FILE"
              else
                printf '%s\n' '{"ok":true,"kind":"task"}'
              fi
            '';
          };
        in
        baseTask
        // {
          commandApi.outputs = {
            mode = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
            ];
          };
          launcher = {
            enable = true;
            appId = "json-body";
            summary = "JSON task app";
            description = "Runs a task app that emits strict JSON for machine-output validation.";
            usage = [ "nix run .#json-body" ];
            ownerFile = "nixfied/project/tasks.nix";
          };
        };

      "test-machine-output-invalid-json-body" =
        let
          baseTask = mkShellTask {
            id = "task.test.machine-output.invalid-json-body";
            summary = "Machine-output invalid JSON task";
            description = "Emits a contract-invalid JSON payload to the declared machine-output channel.";
            command = ''
              set -euo pipefail
              if [ -n "''${NIXFIED_MACHINE_OUTPUT_FILE:-}" ]; then
                printf '%s\n' "invalid-json-body human log"
                printf '%s\n' '{"ok":"yes","kind":"task"}' > "$NIXFIED_MACHINE_OUTPUT_FILE"
              else
                printf '%s\n' '{"ok":"yes","kind":"task"}'
              fi
            '';
          };
        in
        baseTask
        // {
          commandApi.outputs = {
            mode = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
            ];
          };
          launcher = {
            enable = true;
            appId = "json-body-invalid";
            summary = "Invalid JSON task app";
            description = "Runs a task app that violates the machine-output contract.";
            usage = [ "nix run .#json-body-invalid" ];
            ownerFile = "nixfied/project/tasks.nix";
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

    machineOutputs = {
      "machine-json" = {
        id = "machine-json";
        targetAppId = "json-body";
        validation.contractRef = "machineOutput.result";
        summary = "Machine-output happy-path app";
        description = "Wraps the proof JSON task app with strict machine-output validation.";
        usage = [ "nix run .#machine-json" ];
        ownerFile = "nixfied/project/tasks.nix";
      };

      "machine-json-invalid-payload" = {
        id = "machine-json-invalid-payload";
        targetAppId = "json-body-invalid";
        validation.contractRef = "machineOutput.result";
        summary = "Machine-output invalid-payload app";
        description = "Wraps a contract-invalid JSON task app and exposes the structured validation failure envelope.";
        usage = [ "nix run .#machine-json-invalid-payload" ];
        ownerFile = "nixfied/project/tasks.nix";
      };
    };
  };
}
