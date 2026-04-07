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

  harness = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      model
      services
      serviceDefinitions
      registry
      ;
    projectRoot = ../..;
  };

  stopControlsSmoke = import ../../tests/framework/orchestrator-stop-controls-smoke.nix {
    inherit
      pkgs
      model
      services
      serviceDefinitions
      registry
      ;
  };
in
pkgs.runCommand "proof-workspace-scenario-2-interruption-process-cleanup"
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

    proof_require_file ${stopControlsSmoke}

    ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
    proof_require_file "$ORCH"

    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

    workspace="$TMPDIR/proof-seed"
    proof_workspace_bootstrap_seed_copy "$workspace"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH" stop-all-runs > "$TMPDIR/stop-all.out" 2>&1; then
      cat "$TMPDIR/stop-all.out" 2>/dev/null || true
      exit 1
    fi

    echo "OK: proof workspace scenario 2 interruption/process cleanup passed" > "$out"
  ''
