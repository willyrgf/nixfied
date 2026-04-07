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

  scenarioTask =
    baseTask
    // {
      id = "task.test.proof.scenario1";
      summary = "proof scenario 1 task";
      description = "proof scenario 1 task";
      runner = {
        type = "shell";
        package = null;
        workflowId = null;
        command = ''
          set -euo pipefail
          echo "INFO: proof scenario 1 task start"
          echo "OK: proof scenario 1 task complete"
        '';
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
      };
    };

  scenarioWorkflow = {
    id = "workflow.test.proof.scenario1";
    summary = "proof scenario 1 workflow";
    description = "proof scenario 1 workflow";
    mode = "custom";
    maxWorkers = 2;
    units = {
      main = {
        taskId = "task.test.proof.scenario1";
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
      parallel = true;
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
        taskId = "task.test.proof.scenario1";
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

  scenarioModel = model // {
    tasks = model.tasks // {
      "task.test.proof.scenario1" = scenarioTask;
    };
    workflows = model.workflows // {
      "workflow.test.proof.scenario1" = scenarioWorkflow;
    };
  };

  harness = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      services
      serviceDefinitions
      registry
      ;
    model = scenarioModel;
    projectRoot = ../..;
  };

  coverageDeps = [ ];
in
pkgs.runCommand "proof-workspace-scenario-1-public-surface-happy-path"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
    ];
    buildInputs = coverageDeps;
  }
  ''
    set -euo pipefail
    ${materialize.shellPrelude}

    ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

    workspace="$TMPDIR/proof-seed"
    proof_workspace_bootstrap_seed_copy "$workspace"
    proof_require_dir "$workspace/.git"
    proof_require_file "$workspace/flake.nix"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH" run-workflow workflow.test.proof.scenario1 \
      --run-id-file "$TMPDIR/seq.run-id" \
      --summary > "$TMPDIR/seq.out" 2>&1; then
      cat "$TMPDIR/seq.out"
      exit 1
    fi
    proof_require_file "$TMPDIR/seq.run-id"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH" run-workflow-parallel workflow.test.proof.scenario1 \
      --run-id-file "$TMPDIR/par.run-id" > "$TMPDIR/par.out" 2>&1; then
      cat "$TMPDIR/par.out"
      exit 1
    fi
    proof_require_file "$TMPDIR/par.run-id"

    seq_run_id="$(tr -d '\n' < "$TMPDIR/seq.run-id")"
    par_run_id="$(tr -d '\n' < "$TMPDIR/par.run-id")"
    proof_require_non_empty "$seq_run_id" "seq_run_id"
    proof_require_non_empty "$par_run_id" "par_run_id"

    "$ORCH" runs > "$TMPDIR/runs.out"
    proof_require_contains "$TMPDIR/runs.out" "$seq_run_id"
    proof_require_contains "$TMPDIR/runs.out" "$par_run_id"

    echo "OK: proof workspace scenario 1 public surface happy path passed" > "$out"
  ''
