{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  taskId = "task.test.run-id.active-collision";
  workflowId = "workflow.test.run-id.active-collision";

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { ... }:
        {
          nixfied.tasks."test.run-id.active-collision" = {
            id = taskId;
            summary = "Run id active collision probe";
            description = "Blocks on a release gate so concurrent identical invocations must suffix the second run id.";
            commandApi.commandClass = "passthrough";
            runtime.passThroughEnv = [ "RUN_ID_COLLISION_GATE" ];
            runner.command = ''
              set -euo pipefail
              : "''${RUN_ID_COLLISION_GATE:?missing RUN_ID_COLLISION_GATE}"
              printf '%s\n' "INFO: collision probe waiting"
              while [ ! -f "$RUN_ID_COLLISION_GATE" ]; do
                ${pkgs.coreutils}/bin/sleep 0.1
              done
              printf '%s\n' "OK: collision probe released"
            '';
          };

          nixfied.workflows."test.run-id.active-collision" = {
            id = workflowId;
            summary = "Run id active collision workflow";
            description = "Minimal workflow used to prove active collision suffixing and attempt-scoped summaries.";
            mode = "custom";
            maxWorkers = 1;
            units.main = {
              taskId = taskId;
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

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = compiled.model;
    services = compiled.services;
    serviceDefinitions = compiled.serviceDefinitions;
    projectRoot = ../..;
  };
in
pkgs.runCommand "run-id-active-collision-suffix-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export RUN_ID_COLLISION_GATE="$TMPDIR/release-gate"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  wait_for_file() {
    local path="$1"
    [ -f "$path" ]
  }

  "$ORCH" run-workflow "${workflowId}" \
    --run-id-file "$TMPDIR/run-1.run-id" \
    --summary-file "$TMPDIR/run-1.summary.json" \
    --summary > "$TMPDIR/run-1.out" 2>&1 &
  pid_one="$!"

  wait_for_condition 30 "first run id file" wait_for_file "$TMPDIR/run-1.run-id"
  run_one="$(read_trimmed_file "$TMPDIR/run-1.run-id")"
  require_non_empty "$run_one" "run_one"

  case "$run_one" in
    *-c???)
      fail "first run should keep the canonical run id: $run_one"
      ;;
  esac

  wait_for_run_state "$ORCH" "$run_one" "running" 30
  run_one_record="$REGISTRY_ROOT/orchestrator/runs/$run_one.json"
  require_file "$run_one_record"
  attempt_one="$(${pkgs.jq}/bin/jq -r '.payload.attempt_id' "$run_one_record")"
  require_non_empty "$attempt_one" "attempt_one"

  "$ORCH" run-workflow "${workflowId}" \
    --run-id-file "$TMPDIR/run-2.run-id" \
    --summary-file "$TMPDIR/run-2.summary.json" \
    --summary > "$TMPDIR/run-2.out" 2>&1 &
  pid_two="$!"

  wait_for_condition 30 "second run id file" wait_for_file "$TMPDIR/run-2.run-id"
  run_two="$(read_trimmed_file "$TMPDIR/run-2.run-id")"
  require_non_empty "$run_two" "run_two"

  if [ "$run_one" = "$run_two" ]; then
    fail "second identical active run should receive a collision suffix"
  fi

  case "$run_two" in
    "$run_one"-c001) ;;
    *)
      printf 'run_one=%s\nrun_two=%s\n' "$run_one" "$run_two"
      fail "second run should receive the active-collision suffix"
      ;;
  esac

  run_two_record="$REGISTRY_ROOT/orchestrator/runs/$run_two.json"
  wait_for_condition 30 "second run record" wait_for_file "$run_two_record"
  require_file "$run_two_record"
  attempt_two="$(${pkgs.jq}/bin/jq -r '.payload.attempt_id' "$run_two_record")"
  require_non_empty "$attempt_two" "attempt_two"

  if [ "$attempt_one" = "$attempt_two" ]; then
    fail "active collision runs must keep distinct attempt ids"
  fi

  : > "$RUN_ID_COLLISION_GATE"

  wait "$pid_one"
  wait "$pid_two"

  require_file "$TMPDIR/run-1.summary.json"
  require_file "$TMPDIR/run-2.summary.json"

  ${pkgs.jq}/bin/jq -e --arg runId "$run_one" --arg attemptId "$attempt_one" '
    .payload.run_id == $runId and .payload.attempt_id == $attemptId
  ' "$TMPDIR/run-1.summary.json" > /dev/null

  ${pkgs.jq}/bin/jq -e --arg runId "$run_two" --arg attemptId "$attempt_two" '
    .payload.run_id == $runId and .payload.attempt_id == $attemptId
  ' "$TMPDIR/run-2.summary.json" > /dev/null

  run_one_artifacts="$(find "$CI_ARTIFACTS_ROOT" -type f -path "*/$run_one/$attempt_one/summary.json" | head -n 1 || true)"
  run_two_artifacts="$(find "$CI_ARTIFACTS_ROOT" -type f -path "*/$run_two/$attempt_two/summary.json" | head -n 1 || true)"
  require_non_empty "$run_one_artifacts" "run_one_artifacts"
  require_non_empty "$run_two_artifacts" "run_two_artifacts"

  case "$run_one_artifacts" in
    */"$run_one"/"$attempt_one"/summary.json) ;;
    *)
      fail "first run summary path is not run+attempt scoped: $run_one_artifacts"
      ;;
  esac

  case "$run_two_artifacts" in
    */"$run_two"/"$attempt_two"/summary.json) ;;
    *)
      fail "second run summary path is not run+attempt scoped: $run_two_artifacts"
      ;;
  esac

  echo "OK: active collisions suffix repeated run ids and keep attempt-scoped artifacts" > "$out"
''
