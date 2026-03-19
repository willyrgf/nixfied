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
      extraPassThroughEnv ? [ ],
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
        passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ extraPassThroughEnv;
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
        extraPassThroughEnv = [ "NIXFIED_STOP_GATE_FILE" ];
        command = ''
          set -euo pipefail
          gate_file="''${NIXFIED_STOP_GATE_FILE:-}"
          deadline="$(( $(date +%s) + 20 ))"

          echo "INFO: stop smoke task started"
          while true; do
            if [ -n "$gate_file" ] && [ -f "$gate_file" ]; then
              echo "INFO: stop smoke gate released"
              break
            fi
            if [ "$(date +%s)" -ge "$deadline" ]; then
              echo "WARN: stop smoke gate timeout reached"
              break
            fi
            sleep 0.1
          done
          echo "OK: stop smoke task completed"
        '';
      };
    };

    workflows = model.workflows // {
      "workflow.test.orchestrator.stop" = stopWorkflow;
    };
  };

  harness = import ./lib/harness.nix {
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
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

  check_pid_not_running() {
    local pid="$1"
    ! ${pkgs.procps}/bin/ps -p "$pid" > /dev/null 2>&1
  }

  stop_gate="$TMPDIR/stop-gate"
  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'tracked stop controls probe\n' > "$repo/tracked.txt"
  printf 'tracked subdir probe\n' > "$repo/subdir/probe.txt"
  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add tracked.txt subdir/probe.txt
  ${pkgs.git}/bin/git -C "$repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
    commit -m "init stop controls probe repo" >/dev/null 2>&1

  NIXFIED_STOP_GATE_FILE="$stop_gate" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow workflow.test.orchestrator.stop --run-id-file "$TMPDIR/run-one.run-id" --bg > "$TMPDIR/detached-one.out" 2>&1
  run_one="$(read_trimmed_file "$TMPDIR/run-one.run-id")"

  wait_for_run_state "$ORCH" "$run_one" "running" 10
  run_one_pid="$("$ORCH" runs "$run_one" | ${pkgs.jq}/bin/jq -r '.pid')"
  if ! "$ORCH" stop-run "$run_one" > "$TMPDIR/stop-one.out" 2>&1; then
    echo "stop-run failed"
    cat "$TMPDIR/stop-one.out"
    exit 1
  fi

  wait_for_run_state "$ORCH" "$run_one" "canceled" 10
  wait_for_condition 10 "run one pid exit" check_pid_not_running "$run_one_pid"

  NIXFIED_STOP_GATE_FILE="$stop_gate" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow workflow.test.orchestrator.stop --run-id-file "$TMPDIR/run-two.run-id" --bg > "$TMPDIR/detached-two.out" 2>&1
  NIXFIED_STOP_GATE_FILE="$stop_gate" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow workflow.test.orchestrator.stop --run-id-file "$TMPDIR/run-three.run-id" --bg > "$TMPDIR/detached-three.out" 2>&1
  run_two="$(read_trimmed_file "$TMPDIR/run-two.run-id")"
  run_three="$(read_trimmed_file "$TMPDIR/run-three.run-id")"

  wait_for_run_state "$ORCH" "$run_two" "running" 10
  wait_for_run_state "$ORCH" "$run_three" "running" 10
  run_two_pid="$("$ORCH" runs "$run_two" | ${pkgs.jq}/bin/jq -r '.pid')"
  run_three_pid="$("$ORCH" runs "$run_three" | ${pkgs.jq}/bin/jq -r '.pid')"
  if ! "$ORCH" stop-all-runs > "$TMPDIR/stop-all.out" 2>&1; then
    echo "stop-all-runs failed"
    cat "$TMPDIR/stop-all.out"
    exit 1
  fi

  wait_for_run_not_state "$ORCH" "$run_two" "running" 10
  wait_for_run_not_state "$ORCH" "$run_three" "running" 10
  wait_for_condition 10 "run two pid exit" check_pid_not_running "$run_two_pid"
  wait_for_condition 10 "run three pid exit" check_pid_not_running "$run_three_pid"

  touch "$stop_gate"

  echo "OK: orchestrator stop controls are functional" > "$out"
''
