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
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };

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

  mkOrchestrator =
    compiled:
    let
      runtimeDeps = runtimeFixture.runtimeMaterialization {
        inherit (compiled) model services serviceDefinitions;
      };
    in
    import ../../nixfied/framework/runtime/orchestrator.nix {
      inherit
        pkgs
        registry
        ;
      inherit (compiled) model services;
      projectRoot = ../..;
      inherit (runtimeDeps) serviceDispatcherProgram;
      runtimeBin = serviceDispatcherProgram;
    };

  orchestratorBase = mkOrchestrator compiledBase;
  orchestratorChanged = mkOrchestrator compiledChanged;
in
pkgs.runCommand "run-id-semantic-inputs-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  ORCH_BASE="${orchestratorBase}/bin/nixfied-orchestrator"
  ORCH_CHANGED="${orchestratorChanged}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  RUN_ID_INPUT=alpha "$ORCH_BASE" run-workflow "${workflowId}" --run-id-file "$TMPDIR/workflow-base.run-id" --summary -- --probe one > "$TMPDIR/workflow-base.out" 2>&1
  RUN_ID_INPUT=alpha "$ORCH_BASE" run-workflow "${workflowId}" --run-id-file "$TMPDIR/workflow-summary.run-id" --summary --log-level debug --output-mode both -- --probe one > "$TMPDIR/workflow-summary.out" 2>&1

  RUN_ID_INPUT=alpha "$ORCH_BASE" run-task "${taskId}" --run-id-file "$TMPDIR/base.run-id" -- --probe one > "$TMPDIR/base.out" 2>&1
  RUN_ID_INPUT=beta "$ORCH_BASE" run-task "${taskId}" --run-id-file "$TMPDIR/pass.run-id" -- --probe one > "$TMPDIR/pass.out" 2>&1
  RUN_ID_INPUT=alpha "$ORCH_BASE" run-task "${taskId}" --run-id-file "$TMPDIR/arg.run-id" -- --probe two > "$TMPDIR/arg.out" 2>&1
  RUN_ID_INPUT=alpha PROJECT_ENV=test "$ORCH_BASE" run-task "${taskId}" --run-id-file "$TMPDIR/env.run-id" -- --probe one > "$TMPDIR/env.out" 2>&1
  RUN_ID_INPUT=alpha NIX_ENV=1 "$ORCH_BASE" run-task "${taskId}" --run-id-file "$TMPDIR/slot.run-id" -- --probe one > "$TMPDIR/slot.out" 2>&1
  RUN_ID_INPUT=alpha "$ORCH_CHANGED" run-task "${taskId}" --run-id-file "$TMPDIR/model.run-id" -- --probe one > "$TMPDIR/model.out" 2>&1

  workflow_base_id="$(read_trimmed_file "$TMPDIR/workflow-base.run-id")"
  workflow_summary_id="$(read_trimmed_file "$TMPDIR/workflow-summary.run-id")"
  base_id="$(read_trimmed_file "$TMPDIR/base.run-id")"
  pass_id="$(read_trimmed_file "$TMPDIR/pass.run-id")"
  arg_id="$(read_trimmed_file "$TMPDIR/arg.run-id")"
  env_id="$(read_trimmed_file "$TMPDIR/env.run-id")"
  slot_id="$(read_trimmed_file "$TMPDIR/slot.run-id")"
  model_id="$(read_trimmed_file "$TMPDIR/model.run-id")"

  if [ "$workflow_base_id" != "$workflow_summary_id" ]; then
    printf 'workflow_base=%s\nworkflow_summary=%s\n' "$workflow_base_id" "$workflow_summary_id"
    fail "summary/logging output controls should not change run id"
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

  echo "OK: run id tracks semantic invocation inputs only" > "$out"
''
