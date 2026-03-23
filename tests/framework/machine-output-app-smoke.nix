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
  setupTaskId = "task.test.machine-output.setup";
  teardownTaskId = "task.test.machine-output.teardown";
  workflowId = "workflow.test.machine-output.sample";

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.contracts.definitions.machineOutput.result = contractTypes.record {
          fields = {
            kind = contractTypes.field {
              schema = contractTypes.enum {
                values = [ "task" ];
              };
            };
            ok = contractTypes.field {
              schema = contractTypes.bool { };
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
          description = "Emits strict JSON so the machine-output wrapper can replay it.";
          contract.output = {
            format = "json";
            channels = "stdout";
            keys = [
              "ok"
              "kind"
            ];
          };
          runner.command = ''
            set -euo pipefail
            printf '%s\n' '{"ok":true,"kind":"task"}'
          '';
        };

        nixfied.tasks."test.machine-output.setup" = {
          id = setupTaskId;
          summary = "machine-output setup";
          description = "Creates setup marker files and emits stdout noise.";
          runtime.passThroughEnv = [ "MACHINE_OUTPUT_TEST_DIR" ];
          contract.input.env.extra = [
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
          contract.input.env.extra = [
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
        };

        nixfied.apps = {
          "workflow-smoke" = {
            id = "workflow-smoke";
            kind = "workflowRef";
            workflowId = workflowId;
            summary = "Workflow app runtime smoke";
            description = "Runs a workflowRef app through the selected-app launcher.";
            usage = [ "nix run .#workflow-smoke" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "json-body" = {
            id = "json-body";
            kind = "taskRef";
            taskId = jsonTaskId;
            summary = "JSON task app runtime smoke";
            description = "Runs a taskRef app that emits strict JSON.";
            usage = [ "nix run .#json-body" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "machine-output-setup" = {
            id = "machine-output-setup";
            kind = "taskRef";
            taskId = setupTaskId;
            summary = "Machine-output setup app";
            description = "Setup helper for machine-output app smoke coverage.";
            usage = [ "nix run .#machine-output-setup" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "machine-output-teardown" = {
            id = "machine-output-teardown";
            kind = "taskRef";
            taskId = teardownTaskId;
            summary = "Machine-output teardown app";
            description = "Teardown helper for machine-output app smoke coverage.";
            usage = [ "nix run .#machine-output-teardown" ];
            ownerFile = "tests/framework/machine-output-app-smoke.nix";
          };

          "machine-json" = {
            id = "machine-json";
            kind = "machineOutput";
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

          "workflow-machine-invalid" = {
            id = "workflow-machine-invalid";
            kind = "machineOutput";
            targetAppId = "workflow-smoke";
            validation = {
              contractRef = "machineOutput.result";
            };
            summary = "Workflow machine-output failure smoke";
            description = "Shows that workflowRef targets fail cleanly when they do not emit strict JSON.";
            usage = [ "nix run .#workflow-machine-invalid" ];
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
  "$JQ" -e '.code == "machine-output-invalid-json"' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure code must be machine-output-invalid-json"
  }
  "$JQ" -e '.targetAppId == "workflow-smoke"' "$TMPDIR/invalid.json" > /dev/null || {
    cat "$TMPDIR/invalid.json"
    fail "workflow-targeted machineOutput failure payload must identify the target app"
  }

  "$INTROSPECT_APP" app:machine-json --json > "$TMPDIR/machine-introspect.json"
  "$JQ" -e '.resolved.nodeId == "app:machine-json"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must resolve machine-json as an app"
  }
  "$JQ" -e '.resolution.data.contractRef == "machineOutput.result"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must expose machine-json contractRef"
  }
  "$JQ" -e '.resolution.data.targetAppId == "json-body"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must expose machine-json targetAppId"
  }
  "$JQ" -e '.execution.runSurface == "app-wrapper"' "$TMPDIR/machine-introspect.json" > /dev/null || {
    cat "$TMPDIR/machine-introspect.json"
    fail "introspect must expose app-wrapper execution for machineOutput apps"
  }

  "$INTROSPECT_APP" app:workflow-smoke --json > "$TMPDIR/workflow-introspect.json"
  "$JQ" -e '.resolved.nodeId == "app:workflow-smoke"' "$TMPDIR/workflow-introspect.json" > /dev/null || {
    cat "$TMPDIR/workflow-introspect.json"
    fail "introspect must resolve workflow-smoke as an app"
  }
  "$JQ" -e --arg workflowId ${pkgs.lib.escapeShellArg workflowId} '.resolution.data.workflowId == $workflowId' "$TMPDIR/workflow-introspect.json" > /dev/null || {
    cat "$TMPDIR/workflow-introspect.json"
    fail "introspect must expose workflowId for workflowRef apps"
  }
  "$JQ" -e '.execution.runSurface == "run-workflow"' "$TMPDIR/workflow-introspect.json" > /dev/null || {
    cat "$TMPDIR/workflow-introspect.json"
    fail "introspect must expose run-workflow execution for workflowRef apps"
  }

  echo "OK: workflowRef and machineOutput apps execute with stable runtime contracts" > "$out"
''
