{
  pkgs,
  model,
  stateHash,
  canonical,
  registry,
}:
{
  "model-hash" = import ./model-hash.nix {
    inherit
      pkgs
      model
      stateHash
      canonical
      ;
  };

  "cross-machine-hash" = import ./cross-machine-hash.nix {
    inherit
      pkgs
      model
      stateHash
      canonical
      ;
  };

  "scheduler-order" = import ./scheduler-order.nix {
    inherit
      pkgs
      model
      ;
  };

  "help-snapshot" = import ./help-snapshot.nix {
    inherit
      pkgs
      model
      ;
  };

  "registry-replay" = import ./registry-replay.nix {
    inherit
      pkgs
      registry
      ;
  };

  "compiler-validation" = import ./compiler-validation.nix {
    inherit
      pkgs
      model
      ;
  };

  "executor-contract" = import ./executor-contract.nix {
    inherit pkgs;
  };

  "env-sandbox-contract" = import ./env-sandbox-contract.nix {
    inherit pkgs;
  };

  "operations-contract" = import ./operations-contract.nix {
    inherit pkgs;
  };

  "registry-events-contract" = import ./registry-events-contract.nix {
    inherit
      pkgs
      registry
      ;
  };

  "log-prefix-contract" = import ./log-prefix-contract.nix {
    inherit pkgs;
  };

  "parallel-runner-smoke" = import ./parallel-runner-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "parallel-worker-cap-smoke" = import ./parallel-worker-cap-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "parallel-worker-cap-invalid-smoke" = import ./parallel-worker-cap-invalid-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ci-mode-matrix-smoke" = import ./ci-mode-matrix-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "workflow-mode-derived-smoke" = import ./workflow-mode-derived-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "task-hooks-smoke" = import ./task-hooks-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "framework-test-cli-contract-smoke" = import ./framework-test-cli-contract-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "framework-selfhost-contract" = import ./framework-selfhost-contract.nix {
    inherit
      pkgs
      model
      ;
  };

  "framework-install-vendor-smoke" = import ./framework-install-vendor-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "framework-install-thin-smoke" = import ./framework-install-thin-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "framework-upgrade-preserve-smoke" = import ./framework-upgrade-preserve-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "orchestrator-lifecycle-contract" = import ./orchestrator-lifecycle-contract.nix {
    inherit pkgs;
  };

  "orchestrator-stop-controls-smoke" = import ./orchestrator-stop-controls-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "workflow-lifecycle-smoke" = import ./workflow-lifecycle-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "summary-json-smoke" = import ./summary-json-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-execution-smoke" = import ./ephemeral-execution-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };
}
