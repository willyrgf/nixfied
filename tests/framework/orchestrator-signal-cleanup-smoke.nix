{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  baseTask = model.tasks."task.check";

  mkShellTask =
    {
      id,
      command,
      extraPassThroughEnv ? [ ],
    }:
    baseTask
    // {
      inherit id;
      summary = id;
      description = id;
      runner = {
        type = "shell";
        inherit command;
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
        passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ extraPassThroughEnv;
      };
    };

  signalWorkflow = {
    id = "workflow.test.orchestrator.signal";
    summary = "orchestrator signal smoke workflow";
    description = "workflow used by orchestrator signal cleanup smoke test";
    mode = "custom";
    maxWorkers = 1;
    units = {
      main = {
        taskId = "task.test.orchestrator.signal.sleep";
        needs = [ ];
        locks = [ ];
        when = {
          envEquals = { };
          envPresent = [ ];
        };
        skipIfMissingEnv = [ ];
      };
    };
    stages = [ [ "main" ] ];
    preRun = {
      tasks = [ ];
    };
    postRun = {
      tasks = [ ];
      alwaysRun = true;
    };
    artifacts = {
      root = "artifacts-root";
      keepOnSuccess = false;
      keepOnFailure = true;
      writeSummary = true;
    };
    execution = {
      parallel = false;
      failFast = true;
      lockPolicy = "exclusive";
      emitRegistryEvents = true;
      ephemeral = {
        enable = null;
      };
    };
    plan = [
      {
        name = "main";
        taskId = "task.test.orchestrator.signal.sleep";
        needs = [ ];
        locks = [ ];
        when = {
          envEquals = { };
          envPresent = [ ];
        };
        skipIfMissingEnv = [ ];
      }
    ];
  };

  signalModel = model // {
    tasks = model.tasks // {
      "task.test.orchestrator.signal.sleep" = mkShellTask {
        id = "task.test.orchestrator.signal.sleep";
        extraPassThroughEnv = [ "NIXFIED_SIGNAL_GATE_FILE" ];
        command = ''
          set -euo pipefail
          gate_file="''${NIXFIED_SIGNAL_GATE_FILE:-}"
          echo "INFO: signal smoke task started"
          while [ -z "$gate_file" ] || [ ! -f "$gate_file" ]; do
            sleep 0.1
          done
          echo "OK: signal smoke task completed"
        '';
      };
    };

    workflows = model.workflows // {
      "workflow.test.orchestrator.signal" = signalWorkflow;
    };
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = signalModel;
    inherit
      services
      serviceDefinitions
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "orchestrator-signal-cleanup-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

  signal_gate="$TMPDIR/signal-gate"
  NIXFIED_SIGNAL_GATE_FILE="$signal_gate" \
    "$ORCH" run-workflow workflow.test.orchestrator.signal --run-id-file "$TMPDIR/run-id" \
    > "$TMPDIR/orch.out" 2>&1 &
  orch_pid="$!"

  wait_for_condition 20 "run id file" test -f "$TMPDIR/run-id"
  run_id="$(read_trimmed_file "$TMPDIR/run-id")"
  require_non_empty "$run_id" "run id"

  wait_for_run_state "$ORCH" "$run_id" "running" 20

  kill -TERM "$orch_pid"
  set +e
  wait "$orch_pid"
  orch_rc="$?"
  set -e

  if [ "$orch_rc" -ne 130 ]; then
    echo "expected orchestrator TERM exit code 130, got $orch_rc"
    cat "$TMPDIR/orch.out"
    exit 1
  fi

  wait_for_run_state "$ORCH" "$run_id" "canceled" 20

  if ! ${pkgs.jq}/bin/jq -e --arg runId "$run_id" '
    select((.payload.runId // "") == $runId and (.payload.workflowId // "") == "workflow.test.orchestrator.signal" and (.payload.taskId // "") == "" and (.payload.state // "") == "canceled")
    | (.payload.detail.reason == "orchestrator-interrupted" and .payload.detail.signal == "TERM")
  ' "$REGISTRY_ROOT/events.ndjson" > /dev/null; then
    echo "expected cancellation event for interrupted orchestrator run"
    cat "$REGISTRY_ROOT/events.ndjson"
    exit 1
  fi

  if ! "$ORCH" runs "$run_id" | ${pkgs.jq}/bin/jq -e '.payload.stop_reason == "signal-term"' > /dev/null; then
    echo "expected run record stop_reason=signal-term"
    "$ORCH" runs "$run_id"
    exit 1
  fi

  touch "$signal_gate"

  echo "OK: orchestrator direct signal cleanup marks runs canceled and appends an event" > "$out"
''
