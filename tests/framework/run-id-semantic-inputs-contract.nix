{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  taskId = "task.test.run-id.semantic";
  workflowId = "workflow.test.run-id.semantic";

  mkCompiled =
    summary:
    frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        (
          _:
          {
            nixfied.tasks."test.run-id.semantic" = {
              id = taskId;
              inherit summary;
              description = "Used to prove semantic invocation inputs change run ids.";
              commandApi.commandClass = "passthrough";
              runtime.passThroughEnv = [ "RUN_ID_INPUT" ];
              runner.command = ''
                set -euo pipefail
                printf '%s\n' "OK: run id semantic probe input=''${RUN_ID_INPUT:-unset} args=$*"
              '';
            };

            nixfied.workflows."test.run-id.semantic" = {
              id = workflowId;
              summary = "Run id semantic workflow";
              description = "Minimal workflow used by run id semantic checks.";
              mode = "custom";
              maxWorkers = 1;
              units.main = {
                inherit taskId;
                needs = [ ];
                locks = [ ];
                when = {
                  envEquals = { };
                  envPresent = [ ];
                };
                skipIfMissingEnv = [ ];
              };
              stages = [ ];
              preRun.tasks = [ ];
              postRun = {
                tasks = [ ];
                alwaysRun = true;
              };
            };
          }
        )
      ];
      localOverrides = [ ];
    };

  compiledBase = mkCompiled "Run id semantic probe";
  compiledChanged = mkCompiled "Run id semantic probe changed";

  baseModelFile = pkgs.writeText "run-id-semantic-base-model.json" (builtins.toJSON compiledBase.model);
  changedModelFile = pkgs.writeText "run-id-semantic-changed-model.json" (
    builtins.toJSON compiledChanged.model
  );

  baseControlBootstrap = import ../../nixfied/framework/runtime/control-bootstrap.nix {
    inherit pkgs;
    inherit (compiledBase) model;
    projectRoot = ../..;
    modelFile = baseModelFile;
  };
  changedControlBootstrap = import ../../nixfied/framework/runtime/control-bootstrap.nix {
    inherit pkgs;
    inherit (compiledChanged) model;
    projectRoot = ../..;
    modelFile = changedModelFile;
  };

  sharedRuntimeBase = import ../../nixfied/framework/runtime/shared-runtime-lib.nix {
    inherit pkgs;
    inherit (compiledBase) model;
    runCounterLockPurpose = "run-id-semantic-inputs-base";
  };
  sharedRuntimeChanged = import ../../nixfied/framework/runtime/shared-runtime-lib.nix {
    inherit pkgs;
    inherit (compiledChanged) model;
    runCounterLockPurpose = "run-id-semantic-inputs-changed";
  };
in
pkgs.runCommand "run-id-semantic-inputs-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}
  ${registry.events.mkShellLib { }}

  compute_with_base() {
    local registry_root="$1"
    shift

    (
      export REGISTRY_ROOT="$registry_root"
      mkdir -p "$REGISTRY_ROOT"
      ${baseControlBootstrap}
      ${sharedRuntimeBase}
      RUN_SUFFIX_REASON=""
      run_id="$(compute_run_id "$@")" || exit 1
      deactivate_run "$run_id"
      printf '%s' "$run_id"
    )
  }

  compute_with_changed() {
    local registry_root="$1"
    shift

    (
      export REGISTRY_ROOT="$registry_root"
      mkdir -p "$REGISTRY_ROOT"
      ${changedControlBootstrap}
      ${sharedRuntimeChanged}
      RUN_SUFFIX_REASON=""
      run_id="$(compute_run_id "$@")" || exit 1
      deactivate_run "$run_id"
      printf '%s' "$run_id"
    )
  }

  workflow_base_id="$(
    RUN_ID_INPUT=alpha \
      compute_with_base "$TMPDIR/workflow-registry" workflow "${workflowId}" "" -- --probe one
  )"
  workflow_controls_id="$(
    RUN_ID_INPUT=alpha \
    LOG_LEVEL=debug \
    OUTPUT_MODE=both \
      compute_with_base "$TMPDIR/workflow-registry" workflow "${workflowId}" "" -- --probe one
  )"

  base_id="$(
    RUN_ID_INPUT=alpha \
      compute_with_base "$TMPDIR/task-registry" task "" "${taskId}" -- --probe one
  )"
  pass_id="$(
    RUN_ID_INPUT=beta \
      compute_with_base "$TMPDIR/task-registry" task "" "${taskId}" -- --probe one
  )"
  arg_id="$(
    RUN_ID_INPUT=alpha \
      compute_with_base "$TMPDIR/task-registry" task "" "${taskId}" -- --probe two
  )"
  env_id="$(
    RUN_ID_INPUT=alpha \
    PROJECT_ENV=test \
      compute_with_base "$TMPDIR/task-registry" task "" "${taskId}" -- --probe one
  )"
  slot_id="$(
    RUN_ID_INPUT=alpha \
    NIX_ENV=1 \
      compute_with_base "$TMPDIR/task-registry" task "" "${taskId}" -- --probe one
  )"
  model_id="$(
    RUN_ID_INPUT=alpha \
      compute_with_changed "$TMPDIR/task-registry-changed" task "" "${taskId}" -- --probe one
  )"

  if [ "$workflow_base_id" != "$workflow_controls_id" ]; then
    printf 'workflow_base=%s\nworkflow_controls=%s\n' "$workflow_base_id" "$workflow_controls_id"
    fail "workflow logging and output controls should not change run id"
  fi

  if [ "$base_id" = "$pass_id" ]; then
    fail "declared passthrough env should change run id"
  fi
  if [ "$base_id" = "$arg_id" ]; then
    fail "forwarded args should change run id"
  fi
  if [ "$base_id" = "$env_id" ]; then
    fail "PROJECT_ENV should change run id"
  fi
  if [ "$base_id" = "$slot_id" ]; then
    fail "NIX_ENV should change run id"
  fi
  if [ "$base_id" = "$model_id" ]; then
    fail "compiled model changes should change run id"
  fi

  echo "OK: run id helper tracks semantic inputs without orchestrator integration" > "$out"
''
