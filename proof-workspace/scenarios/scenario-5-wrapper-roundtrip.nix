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
      id = "task.test.proof.scenario5";
      summary = "proof scenario 5 task";
      description = "proof scenario 5 task";
      runner = {
        type = "shell";
        package = null;
        workflowId = null;
        command = ''
          set -euo pipefail
          echo "INFO: proof scenario 5 task start"
          echo "OK: proof scenario 5 task complete"
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
      "task.test.proof.scenario5" = scenarioTask;
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
pkgs.runCommand "proof-workspace-scenario-5-wrapper-roundtrip"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
      pkgs.nix
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

    thin_workspace="$TMPDIR/proof-install-thin"
    vendor_workspace="$TMPDIR/proof-install-vendor"

    proof_workspace_bootstrap_install "$thin_workspace" thin
    proof_workspace_bootstrap_install "$vendor_workspace" vendor

    proof_require_dir "$thin_workspace/.git"
    proof_require_file "$thin_workspace/flake.nix"
    proof_require_dir "$vendor_workspace/.git"
    proof_require_file "$vendor_workspace/flake.nix"

    if ! NIXFIED_CALLER_PWD="$thin_workspace" "$ORCH" run-task task.test.proof.scenario5 > "$TMPDIR/scenario5.thin.out" 2>&1; then
      cat "$TMPDIR/scenario5.thin.out" 2>/dev/null || true
      exit 1
    fi
    if ! NIXFIED_CALLER_PWD="$vendor_workspace" "$ORCH" run-task task.test.proof.scenario5 > "$TMPDIR/scenario5.vendor.out" 2>&1; then
      cat "$TMPDIR/scenario5.vendor.out" 2>/dev/null || true
      exit 1
    fi

    proof_require_contains "$TMPDIR/scenario5.thin.out" "OK:"
    proof_require_contains "$TMPDIR/scenario5.vendor.out" "OK:"

    echo "OK: proof workspace scenario 5 wrapper roundtrip passed" > "$out"
  ''
