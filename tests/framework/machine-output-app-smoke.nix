{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  contractTypes = frameworkLib.contracts.types;
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;
  workflowTaskId = "task.test.machine-output.workflow-body";
  jsonTaskId = "task.test.machine-output.json-body";
  invalidJsonTaskId = "task.test.machine-output.invalid-json-body";
  unknownFieldTaskId = "task.test.machine-output.unknown-field-body";
  machineLogTaskId = "task.test.machine-output.log-on-machine-channel";
  setupTaskId = "task.test.machine-output.setup";
  teardownTaskId = "task.test.machine-output.teardown";
  workflowId = "workflow.test.machine-output.sample";

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.contracts.definitions."machineOutput.result" = contractTypes.record {
          doc = "Validated machine-output payload.";
          fields = {
            ok = contractTypes.field {
              schema = contractTypes.bool { };
            };
            kind = contractTypes.field {
              schema = contractTypes.enum {
                values = [
                  "task"
                  "workflow"
                ];
              };
            };
          };
        };

        nixfied.tasks."test.machine-output.workflow-body" = {
          id = workflowTaskId;
          summary = "machine-output workflow body";
          description = "Emits a marker so workflowRef app execution stays testable.";
          runner.command = ''
            set -euo pipefail
            printf '%s\n' "WORKFLOW_BODY"
          '';
        };

        nixfied.tasks."test.machine-output.json-body" = {
          id = jsonTaskId;
          summary = "machine-output json body";
          description = "Emits strict JSON to the declared machine channel.";
          commandApi.outputs = {
            mode = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
            ];
          };
          runner.command = ''
            set -euo pipefail
            if [ -n "''${NIXFIED_MACHINE_OUTPUT_FILE:-}" ]; then
              printf '%s\n' "json-body human log"
              printf '%s\n' '{"ok":true,"kind":"task"}' > "$NIXFIED_MACHINE_OUTPUT_FILE"
            else
              printf '%s\n' '{"ok":true,"kind":"task"}'
            fi
          '';
        };

        nixfied.tasks."test.machine-output.invalid-json-body" = {
          id = invalidJsonTaskId;
          summary = "machine-output invalid json body";
          description = "Emits JSON with an invalid type to the declared machine channel.";
          commandApi.outputs = {
            mode = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
            ];
          };
          runner.command = ''
            set -euo pipefail
            if [ -n "''${NIXFIED_MACHINE_OUTPUT_FILE:-}" ]; then
              printf '%s\n' "invalid-json-body human log"
              printf '%s\n' '{"ok":"yes","kind":"task"}' > "$NIXFIED_MACHINE_OUTPUT_FILE"
            else
              printf '%s\n' '{"ok":"yes","kind":"task"}'
            fi
          '';
        };

        nixfied.tasks."test.machine-output.unknown-field-body" = {
          id = unknownFieldTaskId;
          summary = "machine-output unknown-field body";
          description = "Emits JSON with an unexpected field to the declared machine channel.";
          commandApi.outputs = {
            mode = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
              "extra"
            ];
          };
          runner.command = ''
            set -euo pipefail
            if [ -n "''${NIXFIED_MACHINE_OUTPUT_FILE:-}" ]; then
              printf '%s\n' "unknown-field-body human log"
              printf '%s\n' '{"ok":true,"kind":"task","extra":"boom"}' > "$NIXFIED_MACHINE_OUTPUT_FILE"
            else
              printf '%s\n' '{"ok":true,"kind":"task","extra":"boom"}'
            fi
          '';
        };

        nixfied.tasks."test.machine-output.log-on-machine-channel" = {
          id = machineLogTaskId;
          summary = "machine-output log on machine channel";
          description = "Writes human log text to the machine channel so validation rejects it.";
          commandApi.outputs = {
            mode = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
            ];
          };
          runner.command = ''
            set -euo pipefail
            if [ -n "''${NIXFIED_MACHINE_OUTPUT_FILE:-}" ]; then
              printf '%s\n' "INFO: this is not json" > "$NIXFIED_MACHINE_OUTPUT_FILE"
            else
              printf '%s\n' "INFO: this is not json"
            fi
          '';
        };

        nixfied.tasks."test.machine-output.setup" = {
          id = setupTaskId;
          summary = "machine-output setup";
          description = "Creates setup marker files and emits stdout noise.";
          runtime.passThroughEnv = [ "MACHINE_OUTPUT_TEST_DIR" ];
          commandApi.env = [
            {
              name = "MACHINE_OUTPUT_TEST_DIR";
              type = "pathAbs";
              required = true;
              description = "Test output directory for machine-output smoke checks.";
            }
          ];
          runner.command = ''
            set -euo pipefail
            : "''${MACHINE_OUTPUT_TEST_DIR:?}"
            ${pkgs.coreutils}/bin/mkdir -p "$MACHINE_OUTPUT_TEST_DIR"
            printf '%s\n' "setup-noise"
            printf '%s\n' "setup" > "$MACHINE_OUTPUT_TEST_DIR/setup.txt"
          '';
        };

        nixfied.tasks."test.machine-output.teardown" = {
          id = teardownTaskId;
          summary = "machine-output teardown";
          description = "Creates teardown marker files and emits stdout noise.";
          runtime.passThroughEnv = [ "MACHINE_OUTPUT_TEST_DIR" ];
          commandApi.env = [
            {
              name = "MACHINE_OUTPUT_TEST_DIR";
              type = "pathAbs";
              required = true;
              description = "Test output directory for machine-output smoke checks.";
            }
          ];
          runner.command = ''
            set -euo pipefail
            : "''${MACHINE_OUTPUT_TEST_DIR:?}"
            ${pkgs.coreutils}/bin/mkdir -p "$MACHINE_OUTPUT_TEST_DIR"
            printf '%s\n' "teardown-noise"
            printf '%s\n' "teardown" > "$MACHINE_OUTPUT_TEST_DIR/teardown.txt"
          '';
        };

        nixfied.workflows."test.machine-output.sample" = {
          id = workflowId;
          summary = "Machine-output workflow smoke";
          description = "Exercises workflowRef and machineOutput app kinds.";
          units.main.taskId = workflowTaskId;
          launcher = {
            enable = true;
            appId = "workflow-smoke";
            summary = "Workflow app runtime smoke";
            description = "Runs a workflowRef app through the runtime-backed workflow app surface.";
            usage = [ "nix run .#workflow-smoke" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };
        };

        nixfied.tasks."test.machine-output.json-body".launcher = {
          enable = true;
          appId = "json-body";
          summary = "JSON task app runtime smoke";
          description = "Runs a taskRef app that emits strict JSON.";
          usage = [ "nix run .#json-body" ];
          ownerFile = "tests/framework/machine-output-app-smoke.nix";
        };

        nixfied.tasks."test.machine-output.setup".launcher = {
          enable = true;
          appId = "machine-output-setup";
          summary = "Machine-output setup app";
          description = "Setup helper for machine-output app smoke coverage.";
          usage = [ "nix run .#machine-output-setup" ];
          ownerFile = "tests/framework/machine-output-app-smoke.nix";
        };

        nixfied.tasks."test.machine-output.teardown".launcher = {
          enable = true;
          appId = "machine-output-teardown";
          summary = "Machine-output teardown app";
          description = "Teardown helper for machine-output app smoke coverage.";
          usage = [ "nix run .#machine-output-teardown" ];
          ownerFile = "tests/framework/machine-output-app-smoke.nix";
        };

        nixfied.tasks."test.machine-output.invalid-json-body".launcher = {
          enable = true;
          appId = "json-body-invalid";
          summary = "Invalid JSON task app";
          description = "Runs a taskRef app that violates the machine-output contract.";
          usage = [ "nix run .#json-body-invalid" ];
          ownerFile = "tests/framework/machine-output-app-smoke.nix";
        };

        nixfied.tasks."test.machine-output.unknown-field-body".launcher = {
          enable = true;
          appId = "json-body-unknown-field";
          summary = "Unknown-field JSON task app";
          description = "Runs a taskRef app that emits an unknown field.";
          usage = [ "nix run .#json-body-unknown-field" ];
          ownerFile = "tests/framework/machine-output-app-smoke.nix";
        };

        nixfied.tasks."test.machine-output.log-on-machine-channel".launcher = {
          enable = true;
          appId = "json-body-log-on-machine-channel";
          summary = "Log-text machine channel app";
          description = "Runs a taskRef app that writes log text to the machine channel.";
          usage = [ "nix run .#json-body-log-on-machine-channel" ];
          ownerFile = "tests/framework/machine-output-app-smoke.nix";
        };

        nixfied.machineOutputs = {
          "machine-json" = {
            id = "machine-json";
            targetAppId = "json-body";
            setupAppIds = [ "machine-output-setup" ];
            teardownAppIds = [ "machine-output-teardown" ];
            validation = {
              contractRef = "machineOutput.result";
            };
            summary = "Machine-output app smoke";
            description = "Wraps a JSON-emitting task app with setup and teardown helpers.";
            usage = [ "nix run .#machine-json" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "machine-json-invalid-payload" = {
            id = "machine-json-invalid-payload";
            targetAppId = "json-body-invalid";
            validation = {
              contractRef = "machineOutput.result";
            };
            summary = "Machine-output invalid-payload smoke";
            description = "Shows that contract-invalid machine output fails cleanly.";
            usage = [ "nix run .#machine-json-invalid-payload" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "machine-json-unknown-field" = {
            id = "machine-json-unknown-field";
            targetAppId = "json-body-unknown-field";
            validation = {
              contractRef = "machineOutput.result";
            };
            summary = "Machine-output unknown-field smoke";
            description = "Shows that unknown fields are rejected on closed machine-output payloads.";
            usage = [ "nix run .#machine-json-unknown-field" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "workflow-machine-invalid" = {
            id = "workflow-machine-invalid";
            targetAppId = "workflow-smoke";
            validation = {
              contractRef = "machineOutput.result";
            };
            summary = "Workflow machine-output failure smoke";
            description = "Shows that workflowRef targets fail cleanly when they do not emit strict JSON.";
            usage = [ "nix run .#workflow-machine-invalid" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "machine-json-log-on-machine-channel" = {
            id = "machine-json-log-on-machine-channel";
            targetAppId = "json-body-log-on-machine-channel";
            validation = {
              contractRef = "machineOutput.result";
            };
            summary = "Machine-output log-text failure smoke";
            description = "Shows that log text on the machine channel fails validation.";
            usage = [ "nix run .#machine-json-log-on-machine-channel" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };
        };
      }
    ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "machine-output-app-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  WORKFLOW_APP="${frameworkOutputs.apps."workflow-smoke".program}"
  MACHINE_APP="${frameworkOutputs.apps."machine-json".program}"
  INVALID_PAYLOAD_APP="${frameworkOutputs.apps."machine-json-invalid-payload".program}"
  UNKNOWN_FIELD_APP="${frameworkOutputs.apps."machine-json-unknown-field".program}"
  LOG_ON_MACHINE_CHANNEL_APP="${frameworkOutputs.apps."machine-json-log-on-machine-channel".program}"
  INVALID_APP="${frameworkOutputs.apps."workflow-machine-invalid".program}"
  INTROSPECT_APP="${frameworkOutputs.apps.introspect.program}"
  JQ=${pkgs.jq}/bin/jq
  GREP=${pkgs.gnugrep}/bin/grep

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export MACHINE_OUTPUT_TEST_DIR="$TMPDIR/machine-output-test"
  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE" "$MACHINE_OUTPUT_TEST_DIR"
  cd "$NIXFIED_FLAKE_ROOT"

  "$WORKFLOW_APP" > "$TMPDIR/workflow.out" 2>&1 || {
    cat "$TMPDIR/workflow.out"
    fail "workflowRef app should execute successfully"
  }
  require_contains "$TMPDIR/workflow.out" "WORKFLOW_BODY"

  "$MACHINE_APP" > "$TMPDIR/machine.json" 2>"$TMPDIR/machine.err" || {
    cat "$TMPDIR/machine.err"
    cat "$TMPDIR/machine.json"
    fail "machineOutput app should succeed"
  }
  "$JQ" -e '.ok == true and .kind == "task"' "$TMPDIR/machine.json" > /dev/null || {
    cat "$TMPDIR/machine.json"
    fail "machineOutput app did not replay the expected payload"
  }
  require_contains "$TMPDIR/machine.err" "setup-noise"
  require_contains "$TMPDIR/machine.err" "json-body human log"
  require_contains "$TMPDIR/machine.err" "teardown-noise"
  require_not_contains "$TMPDIR/machine.json" "setup-noise"
  require_not_contains "$TMPDIR/machine.json" "teardown-noise"
  require_file "$MACHINE_OUTPUT_TEST_DIR/setup.txt"
  require_file "$MACHINE_OUTPUT_TEST_DIR/teardown.txt"

  if "$INVALID_APP" > "$TMPDIR/invalid.json" 2>"$TMPDIR/invalid.err"; then
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput app should fail for non-json output"
  fi
  "$JQ" -e '.ok == false' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure payload must be JSON"
  }
  "$JQ" -e '.stage == "validation"' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure stage must be validation"
  }
  "$JQ" -e '.code == "machine-output-validation-failed"' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure code must be machine-output-validation-failed"
  }
  "$JQ" -e '.targetAppId == "workflow-smoke"' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure payload must identify the target app"
  }
  "$JQ" -e '.contractRef == "machineOutput.result" and .validator == "nixfied-kernel"' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure payload must identify the contract and validator"
  }

  if "$INVALID_PAYLOAD_APP" > "$TMPDIR/invalid-payload.json" 2>"$TMPDIR/invalid-payload.err"; then
    cat "$TMPDIR/invalid-payload.json"
    fail "contract-invalid machineOutput app should fail"
  fi
  "$JQ" -e '.ok == false and .stage == "validation" and .code == "machine-output-validation-failed"' "$TMPDIR/invalid-payload.json" > /dev/null || {
    cat "$TMPDIR/invalid-payload.json"
    fail "contract-invalid machineOutput failure payload must use the validation failure envelope"
  }
  "$JQ" -e '.targetAppId == "json-body-invalid" and .contractRef == "machineOutput.result" and .validator == "nixfied-kernel"' "$TMPDIR/invalid-payload.json" > /dev/null || {
    cat "$TMPDIR/invalid-payload.json"
    fail "contract-invalid machineOutput failure payload must identify the target app and contract"
  }

  if "$UNKNOWN_FIELD_APP" > "$TMPDIR/unknown-field.json" 2>"$TMPDIR/unknown-field.err"; then
    cat "$TMPDIR/unknown-field.json"
    fail "unknown-field machineOutput app should fail"
  fi
  "$JQ" -e '.ok == false and .stage == "validation" and .code == "machine-output-validation-failed"' "$TMPDIR/unknown-field.json" > /dev/null || {
    cat "$TMPDIR/unknown-field.json"
    fail "unknown-field machineOutput failure payload must use the validation failure envelope"
  }
  "$JQ" -e '.targetAppId == "json-body-unknown-field" and .contractRef == "machineOutput.result"' "$TMPDIR/unknown-field.json" > /dev/null || {
    cat "$TMPDIR/unknown-field.json"
    fail "unknown-field machineOutput failure payload must identify the target app and contract"
  }

  if "$LOG_ON_MACHINE_CHANNEL_APP" > "$TMPDIR/log-on-machine-channel.json" 2>"$TMPDIR/log-on-machine-channel.err"; then
    cat "$TMPDIR/log-on-machine-channel.json"
    fail "log-text machineOutput app should fail"
  fi
  "$JQ" -e '.ok == false and .stage == "validation" and .code == "machine-output-validation-failed"' "$TMPDIR/log-on-machine-channel.json" > /dev/null || {
    cat "$TMPDIR/log-on-machine-channel.json"
    fail "log-text machineOutput failure payload must use the validation failure envelope"
  }
  "$JQ" -e '.targetAppId == "json-body-log-on-machine-channel" and .contractRef == "machineOutput.result"' "$TMPDIR/log-on-machine-channel.json" > /dev/null || {
    cat "$TMPDIR/log-on-machine-channel.json"
    fail "log-text machineOutput failure payload must identify the target app and contract"
  }

  "$INTROSPECT_APP" app:machine-json --json > "$TMPDIR/machine-introspect.json"
  "$JQ" -e '.payload.resolved.nodeId == "app:machine-json"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must resolve machine-json as an app"
  }
  "$JQ" -e '.payload.resolution.data.contractRef == "machineOutput.result"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must expose machine-json contractRef"
  }
  "$JQ" -e '.payload.resolution.data.targetAppId == "json-body"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must expose machine-json targetAppId"
  }
  "$JQ" -e '.payload.execution.runSurface == "app-wrapper"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must expose app-wrapper execution for machineOutput apps"
  }

  "$INTROSPECT_APP" app:workflow-smoke --json > "$TMPDIR/workflow-introspect.json"
  "$JQ" -e '.payload.resolved.nodeId == "app:workflow-smoke"' "$TMPDIR/workflow-introspect.json" > /dev/null || {
    cat "$TMPDIR/workflow-introspect.json"
    fail "introspect must resolve workflow-smoke as an app"
  }
  "$JQ" -e --arg workflowId ${pkgs.lib.escapeShellArg workflowId} '.payload.resolution.data.workflowId == $workflowId' "$TMPDIR/workflow-introspect.json" > /dev/null || {
    cat "$TMPDIR/workflow-introspect.json"
    fail "introspect must expose workflowId for workflowRef apps"
  }
  "$JQ" -e '.payload.execution.runSurface == "run-workflow"' "$TMPDIR/workflow-introspect.json" > /dev/null || {
    cat "$TMPDIR/workflow-introspect.json"
    fail "introspect must expose run-workflow execution for workflowRef apps"
  }

  echo "OK: workflowRef and machineOutput apps execute with stable runtime contracts" > "$out"
''
