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

  "discovery-command-surfaces-smoke" = import ./discovery-command-surfaces-smoke.nix {
    inherit pkgs;
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

  "env-loader-strict-smoke" = import ./env-loader-strict-smoke.nix {
    inherit pkgs;
  };

  "shell-contract-contract" = import ./shell-contract-contract.nix {
    inherit pkgs;
  };

  "shell-contract-runtime-smoke" = import ./shell-contract-runtime-smoke.nix {
    inherit pkgs;
  };

  "slot-env-runtime-contract" = import ./slot-env-runtime-contract.nix {
    inherit pkgs;
  };

  "slot-env-runtime-smoke" = import ./slot-env-runtime-smoke.nix {
    inherit pkgs;
  };

  "workspace-registry-isolation-smoke" = import ./workspace-registry-isolation-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "sensitive-pass-through-smoke" = import ./sensitive-pass-through-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "runtime-owned-env-blocked-smoke" = import ./runtime-owned-env-blocked-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "operations-contract" = import ./operations-contract.nix {
    inherit
      pkgs
      model
      ;
  };

  "project-config-boundary" = import ./project-config-boundary.nix {
    inherit pkgs;
  };

  "no-legacy-project-modules" = import ./no-legacy-project-modules.nix {
    inherit pkgs;
  };

  "helios-pinned-source-contract" = import ./helios-pinned-source-contract.nix {
    inherit pkgs;
  };

  "ready-health-matrix-smoke" = import ./ready-health-matrix-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "ready-health-shutdown-smoke" = import ./ready-health-shutdown-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "ready-helios-sync-gate-smoke" = import ./ready-helios-sync-gate-smoke.nix {
    inherit
      pkgs
      registry
      ;
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

  "logging-injection-smoke" = import ./logging-injection-smoke.nix {
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

  "workflow-modes-contract" = import ./workflow-modes-contract.nix {
    inherit pkgs;
  };

  "managed-service-lifecycle-contract" = import ./managed-service-lifecycle-contract.nix {
    inherit pkgs;
  };

  "supervisor-runtime-contract" = import ./supervisor-runtime-contract.nix {
    inherit pkgs;
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

  "framework-install-filter-smoke" = import ./framework-install-filter-smoke.nix {
    inherit pkgs;
  };

  "install-runtime-contract" = import ./install-runtime-contract.nix {
    inherit pkgs;
  };

  "caller-pwd-remote-projectroot-smoke" = import ./caller-pwd-remote-projectroot-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "framework-template-install-upgrade-help-smoke" =
    import ./framework-template-install-upgrade-help-smoke.nix
      {
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

  "postgres-backup-restore-smoke" = import ./postgres-backup-restore-smoke.nix {
    inherit pkgs;
  };

  "nginx-site-management-smoke" = import ./nginx-site-management-smoke.nix {
    inherit pkgs;
  };

  "nginx-site-management-contract" = import ./nginx-site-management-contract.nix {
    inherit pkgs;
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

  "artifacts-run-isolation-smoke" = import ./artifacts-run-isolation-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "artifacts-root-override-isolation-smoke" = import ./artifacts-root-override-isolation-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "helpers-runtime-contract" = import ./helpers-runtime-contract.nix {
    inherit pkgs;
  };

  "ephemeral-execution-smoke" = import ./ephemeral-execution-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-runtime-env-isolation-smoke" = import ./ephemeral-runtime-env-isolation-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-env-file-mode-smoke" = import ./ephemeral-env-file-mode-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "runtime-env-isolation-smoke" = import ./runtime-env-isolation-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-registry-run-isolation-smoke" = import ./ephemeral-registry-run-isolation-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-copy-mode-smoke" = import ./ephemeral-copy-mode-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-nix-source-smoke" = import ./ephemeral-nix-source-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-retention-smoke" = import ./ephemeral-retention-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "ephemeral-copy-budget-smoke" = import ./ephemeral-copy-budget-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "registry-lock-recovery-smoke" = import ./registry-lock-recovery-smoke.nix {
    inherit
      pkgs
      registry
      ;
  };

  "run-record-atomicity-smoke" = import ./run-record-atomicity-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "service-dir-isolation-smoke" = import ./service-dir-isolation-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };
}
