{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };

  baseTask = model.tasks."task.check";

  failingTask =
    baseTask
    // {
      id = "task.test.proof.failure";
      summary = "proof scenario 6 failing task";
      description = "proof scenario 6 failing task";
      runner = {
        type = "shell";
        package = null;
        workflowId = null;
        command = ''
          set -euo pipefail
          echo "ERROR: proof scenario 6 deliberate failure" >&2
          exit 7
        '';
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
      };
    };

  failingWorkflow = {
    id = "workflow.test.proof.failure";
    summary = "proof scenario 6 failing workflow";
    description = "proof scenario 6 failing workflow";
    mode = "custom";
    maxWorkers = 1;
    units = {
      main = {
        taskId = "task.test.proof.failure";
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
      serviceSets = [ ];
    };
    postRun = {
      tasks = [ ];
      serviceSets = [ ];
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
        taskId = "task.test.proof.failure";
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

  failingModel = model // {
    tasks = model.tasks // {
      "task.test.proof.failure" = failingTask;
    };
    workflows = model.workflows // {
      "workflow.test.proof.failure" = failingWorkflow;
    };
  };

  harness = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      services
      serviceDefinitions
      registry
      ;
    model = failingModel;
    projectRoot = ../..;
  };

  runtimeOwnedEnvBlocked = import ../../tests/framework/runtime-owned-env-blocked-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  sensitivePassThrough = import ../../tests/framework/sensitive-pass-through-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  parallelWorkerCapInvalid = import ../../tests/framework/parallel-worker-cap-invalid-smoke.nix {
    inherit
      pkgs
      model
      services
      serviceDefinitions
      registry
      ;
  };
in
pkgs.runCommand "proof-workspace-scenario-6-failure-guardrails"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
      pkgs.jq
    ];
  }
  ''
    set -euo pipefail
    ${materialize.shellPrelude}

    ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
    proof_require_file "$ORCH"

    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

    workspace="$TMPDIR/proof-seed"
    proof_workspace_bootstrap_seed_copy "$workspace"

    set +e
    NIXFIED_CALLER_PWD="$workspace" "$ORCH" run-workflow workflow.test.proof.failure \
      --run-id-file "$TMPDIR/fail.run-id" > "$TMPDIR/fail.out" 2>&1
    fail_rc="$?"
    set -e

    if [ "$fail_rc" -eq 0 ]; then
      cat "$TMPDIR/fail.out" 2>/dev/null || true
      echo "ERROR: failing workflow unexpectedly succeeded"
      exit 1
    fi

    proof_require_file "$TMPDIR/fail.run-id"
    proof_require_file "$TMPDIR/fail.out"
    fail_run_id="$(tr -d '\n' < "$TMPDIR/fail.run-id")"
    proof_require_non_empty "$fail_run_id" "fail_run_id"

    "$ORCH" runs "$fail_run_id" > "$TMPDIR/fail.run.json"
    ${pkgs.jq}/bin/jq -e '
      .payload.state == "failed"
      and .payload.exit_code == 7
      and .payload.command == "run-workflow"
    ' "$TMPDIR/fail.run.json" >/dev/null

    set +e
    NIXFIED_CALLER_PWD="$workspace" "$ORCH" run-task task.not.real > "$TMPDIR/invalid-task.out" 2>&1
    invalid_task_rc="$?"
    set -e
    if [ "$invalid_task_rc" -eq 0 ]; then
      echo "ERROR: expected unknown task to fail"
      exit 1
    fi
    proof_require_contains "$TMPDIR/invalid-task.out" "ERROR:"

    # Keep negative-path replacement evidence explicit while migration is in
    # progress by depending on existing targeted failure checks.
    proof_require_file ${runtimeOwnedEnvBlocked}
    proof_require_file ${sensitivePassThrough}
    proof_require_file ${parallelWorkerCapInvalid}

    echo "OK: proof workspace scenario 6 failure/guardrails passed" > "$out"
  ''
