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
      id = "task.test.proof.scenario3";
      summary = "proof scenario 3 task";
      description = "proof scenario 3 task";
      runner = {
        type = "shell";
        package = null;
        workflowId = null;
        command = ''
          set -euo pipefail
          echo "INFO: proof scenario 3 task start"
          echo "OK: proof scenario 3 task complete"
        '';
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
      };
    };

  scenarioModel = model // {
    tasks = model.tasks // {
      "task.test.proof.scenario3" = scenarioTask;
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

in
pkgs.runCommand "proof-workspace-scenario-3-ephemeral-workspace"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
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
    printf 'proof-untracked-sentinel\n' > "$workspace/fixtures/untracked.sentinel"
    printf 'PROOF_SCENARIO_3=1\n' > "$workspace/.proof.env"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH" run-task task.test.proof.scenario3 > "$TMPDIR/scenario3.out" 2>&1; then
      cat "$TMPDIR/scenario3.out" 2>/dev/null || true
      exit 1
    fi

    proof_require_file "$TMPDIR/scenario3.out"
    proof_require_contains "$TMPDIR/scenario3.out" "OK:"

    echo "OK: proof workspace scenario 3 ephemeral workspace passed" > "$out"
  ''
