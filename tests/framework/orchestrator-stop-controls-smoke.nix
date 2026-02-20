{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.check";

  mkShellTask =
    {
      id,
      command,
    }:
    baseTask
    // {
      inherit id;
      summary = id;
      description = id;
      runner = {
        type = "shell";
        command = command;
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
      };
      ui = baseTask.ui // {
        app = baseTask.ui.app // {
          expose = false;
          name = builtins.replaceStrings [ "." ] [ "-" ] id;
        };
      };
    };

  stopWorkflow = {
    id = "workflow.test.orchestrator.stop";
    summary = "orchestrator stop smoke workflow";
    description = "workflow used by orchestrator stop control smoke test";
    mode = "custom";
    maxWorkers = 1;
    units = {
      main = {
        taskId = "task.test.orchestrator.sleep";
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
      root = "/tmp/ci-artifacts";
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
        taskId = "task.test.orchestrator.sleep";
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

  stopModel = model // {
    tasks = model.tasks // {
      "task.test.orchestrator.sleep" = mkShellTask {
        id = "task.test.orchestrator.sleep";
        command = ''
          set -euo pipefail
          echo "INFO: stop smoke task started"
          sleep 30
          echo "OK: stop smoke task completed"
        '';
      };
    };

    workflows = model.workflows // {
      "workflow.test.orchestrator.stop" = stopWorkflow;
    };
  };

  orchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = stopModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "orchestrator-stop-controls-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  detached_one="$($ORCH run-workflow workflow.test.orchestrator.stop --bg 2>&1)"
  run_one="$(printf '%s\n' "$detached_one" | ${pkgs.gnused}/bin/sed -n 's/^OK: detached run_id=\([^ ]*\).*/\1/p' | ${pkgs.coreutils}/bin/head -n 1)"
  if [ -z "$run_one" ]; then
    echo "failed to parse detached run id"
    echo "$detached_one"
    exit 1
  fi

  sleep 1
  if ! "$ORCH" stop-run "$run_one" > "$TMPDIR/stop-one.out" 2>&1; then
    echo "stop-run failed"
    cat "$TMPDIR/stop-one.out"
    exit 1
  fi

  state_one="$($ORCH runs "$run_one" | ${pkgs.jq}/bin/jq -r '.state')"
  if [ "$state_one" != "canceled" ]; then
    echo "expected canceled state after stop-run, got $state_one"
    $ORCH runs "$run_one"
    exit 1
  fi

  detached_two="$($ORCH run-workflow workflow.test.orchestrator.stop --bg 2>&1)"
  detached_three="$($ORCH run-workflow workflow.test.orchestrator.stop --bg 2>&1)"
  run_two="$(printf '%s\n' "$detached_two" | ${pkgs.gnused}/bin/sed -n 's/^OK: detached run_id=\([^ ]*\).*/\1/p' | ${pkgs.coreutils}/bin/head -n 1)"
  run_three="$(printf '%s\n' "$detached_three" | ${pkgs.gnused}/bin/sed -n 's/^OK: detached run_id=\([^ ]*\).*/\1/p' | ${pkgs.coreutils}/bin/head -n 1)"

  if [ -z "$run_two" ] || [ -z "$run_three" ]; then
    echo "failed to parse detached run ids for stop-all"
    echo "$detached_two"
    echo "$detached_three"
    exit 1
  fi

  sleep 1
  if ! "$ORCH" stop-all-runs > "$TMPDIR/stop-all.out" 2>&1; then
    echo "stop-all-runs failed"
    cat "$TMPDIR/stop-all.out"
    exit 1
  fi

  state_two="$($ORCH runs "$run_two" | ${pkgs.jq}/bin/jq -r '.state')"
  state_three="$($ORCH runs "$run_three" | ${pkgs.jq}/bin/jq -r '.state')"

  if [ "$state_two" = "running" ] || [ "$state_three" = "running" ]; then
    echo "expected stop-all-runs to terminate active runs"
    $ORCH runs
    exit 1
  fi

  echo "OK: orchestrator stop controls are functional" > "$out"
''
