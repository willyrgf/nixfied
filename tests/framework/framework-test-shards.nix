let
  subtractNames = base: excluded: builtins.filter (name: !(builtins.elem name excluded)) base;

  layerChecks = {
    compile = [
      "compiler-validation"
      "contract-render-snapshot"
      "cross-machine-hash"
      "discovery-runtime-contract"
      "docs-guidance-contract"
      "env-sandbox-contract"
      "excluded-service-evaluation"
      "executor-contract"
      "feature-coverage-validation"
      "framework-selfhost-contract"
      "helios-pinned-source-contract"
      "help-snapshot"
      "install-runtime-contract"
      "introspect-contract"
      "introspection-bundle-determinism"
      "introspection-schema"
      "local-override-introspect-contract"
      "managed-service-lifecycle-contract"
      "model-hash"
      "nginx-site-management-contract"
      "no-legacy-project-modules"
      "operations-contract"
      "orchestrator-lifecycle-contract"
      "package-output-contract"
      "postgres-backup-contract"
      "postgres-config-artifacts-contract"
      "project-config-boundary"
      "scheduler-order"
      "service-extractability-contract"
      "service-observability-contract"
      "service-op-composition-contract"
      "service-probe-overrides-contract"
      "service-requirements-contract"
      "service-surface-catalog-contract"
      "slot-env-runtime-contract"
      "supervisor-runtime-contract"
      "vendored-metadata-contract"
    ];

    manifest = [
      "runtime-manifest-fixture-contract"
    ];

    kernel = [
      "ephemeral-runtime-env-isolation-smoke"
      "executor-runtime-contract"
      "isolation-nested-run-id-smoke"
      "kernel-native-tests"
      "nix-checks-parent-workflow-skip-smoke"
      "nix-ci-workflow-contract"
      "orchestrator-runtime-contract"
      "parallel-runner-process-tree-smoke"
      "parallel-runner-smoke"
      "parallel-worker-cap-invalid-smoke"
      "parallel-worker-cap-smoke"
      "postgres-kernel-probe-lifecycle-smoke"
      "registry-detail-derivation-smoke"
      "registry-events-contract"
      "registry-events-runtime-contract"
      "registry-lock-recovery-smoke"
      "registry-replay"
      "run-id-active-collision-suffix-smoke"
      "run-id-noise-stability-smoke"
      "run-id-semantic-inputs-contract"
      "run-record-atomicity-smoke"
      "run-record-validator-failure"
      "runtime-env-isolation-smoke"
      "runtime-events-policy-smoke"
      "runtime-events-status-smoke"
      "service-policy-runtime-smoke"
      "summary-json-smoke"
      "workflow-lifecycle-smoke"
      "workflow-mode-derived-smoke"
      "workflow-parallel-blocked-smoke"
      "workflow-probe-scope-smoke"
      "workflow-validation-errors"
      "workspace-registry-isolation-smoke"
    ];

    adapter = [
      "disabled-service-runtime-surface-smoke"
      "dispatcher-help-fast-path-smoke"
      "framework-utility-launcher-contract"
      "helpers-runtime-contract"
      "launcher-help-fast-path-smoke"
      "launcher-skip-service-pruning-smoke"
      "launcher-surface-contract"
      "log-prefix-contract"
      "orchestrator-arg-forwarding-smoke"
      "orchestrator-signal-cleanup-smoke"
      "runtime-events-contract"
      "selected-app-manifest-contract"
      "service-hook-env-smoke"
      "shell-contract-contract"
      "shell-contract-runtime-smoke"
      "unselected-service-public-launcher-smoke"
      "workflow-ref-app-manifest-contract"
      "workflow-service-set-adapter-smoke"
    ];

    e2e = [
      "artifacts-root-override-isolation-smoke"
      "artifacts-run-isolation-smoke"
      "caller-pwd-remote-projectroot-smoke"
      "ci-mode-matrix-smoke"
      "disabled-service-no-package-resolution-smoke"
      "discovery-command-surfaces-smoke"
      "env-loader-strict-smoke"
      "ephemeral-copy-budget-smoke"
      "ephemeral-copy-mode-smoke"
      "ephemeral-env-file-mode-smoke"
      "ephemeral-execution-smoke"
      "ephemeral-nix-source-smoke"
      "ephemeral-registry-run-isolation-smoke"
      "ephemeral-retention-smoke"
      "flake-show-no-service-materialization-smoke"
      "framework-install-filter-smoke"
      "framework-install-no-caller-compile-smoke"
      "framework-install-thin-smoke"
      "framework-install-vendor-smoke"
      "framework-template-install-upgrade-help-smoke"
      "framework-test-cli-contract-smoke"
      "framework-test-no-caller-compile-smoke"
      "framework-upgrade-no-caller-compile-smoke"
      "framework-upgrade-preserve-smoke"
      "logging-injection-smoke"
      "machine-output-app-smoke"
      "nginx-site-management-smoke"
      "nix-checks-nil-issues-fail-smoke"
      "nix-client-env-smoke"
      "orchestrator-stop-controls-smoke"
      "postgres-backup-restore-smoke"
      "postgres-config-artifacts-smoke"
      "ready-health-matrix-smoke"
      "ready-health-shutdown-smoke"
      "ready-helios-sync-gate-smoke"
      "runtime-controls-no-service-materialization-smoke"
      "runtime-owned-env-blocked-smoke"
      "selected-source-only-resolution-smoke"
      "sensitive-pass-through-smoke"
      "service-dir-isolation-smoke"
      "service-lifecycle-matrix-smoke"
      "service-probe-overrides-smoke"
      "service-set-surface-contract"
      "skip-service-smoke"
      "slot-env-runtime-smoke"
      "supervisor-lifecycle-smoke"
      "task-hooks-smoke"
      "unselected-service-no-package-resolution-smoke"
      "vendored-metadata-packaged-source-smoke"
    ];

    migration = [
      "contract-migration-guard"
      "framework-test-coverage-contract"
      "framework-test-shard-validation"
      "registry-helper-contract"
      "service-api-surface-contract"
      "workflow-modes-contract"
    ];
  };

  serviceChecks = [
    "disabled-service-no-package-resolution-smoke"
    "disabled-service-runtime-surface-smoke"
    "excluded-service-evaluation"
    "flake-show-no-service-materialization-smoke"
    "helios-pinned-source-contract"
    "launcher-skip-service-pruning-smoke"
    "managed-service-lifecycle-contract"
    "nginx-site-management-contract"
    "nginx-site-management-smoke"
    "postgres-backup-contract"
    "postgres-backup-restore-smoke"
    "postgres-config-artifacts-contract"
    "postgres-config-artifacts-smoke"
    "postgres-kernel-probe-lifecycle-smoke"
    "ready-health-matrix-smoke"
    "ready-health-shutdown-smoke"
    "ready-helios-sync-gate-smoke"
    "runtime-controls-no-service-materialization-smoke"
    "service-dir-isolation-smoke"
    "service-extractability-contract"
    "service-hook-env-smoke"
    "service-lifecycle-matrix-smoke"
    "service-observability-contract"
    "service-op-composition-contract"
    "service-policy-runtime-smoke"
    "service-probe-overrides-contract"
    "service-probe-overrides-smoke"
    "service-requirements-contract"
    "service-set-surface-contract"
    "service-surface-catalog-contract"
    "skip-service-smoke"
    "supervisor-lifecycle-smoke"
    "supervisor-runtime-contract"
    "unselected-service-no-package-resolution-smoke"
    "unselected-service-public-launcher-smoke"
    "workflow-service-set-adapter-smoke"
  ];

  order = [
    "compile"
    "manifest"
    "kernel"
    "adapters"
    "services"
    "e2e"
    "migration"
  ];

  shardChecks = {
    compile = subtractNames layerChecks.compile serviceChecks;
    manifest = layerChecks.manifest;
    kernel = subtractNames layerChecks.kernel serviceChecks;
    adapters = subtractNames layerChecks.adapter serviceChecks;
    services = serviceChecks;
    e2e = subtractNames layerChecks.e2e serviceChecks;
    migration = layerChecks.migration;
  };
in
{
  inherit order layerChecks serviceChecks;

  descriptions = {
    compile = "Build compile-time model, help, schema, and documentation proofs.";
    manifest = "Build runtime manifest fixtures and manifest handoff contracts.";
    kernel = "Build kernel-owned runtime semantics, registry, and workflow proofs.";
    adapters = "Build thin launcher, shell, and process-edge adapter proofs.";
    services = "Build service public-surface, lifecycle, readiness, and extractability proofs.";
    e2e = "Build end-to-end public behavior, install, upgrade, isolation, and runtime smokes.";
    migration = "Build deleted-seam guards and ownership-migration regressions.";
  };

  checks = shardChecks;

  profiles = {
    ci = [
      "compile"
      "manifest"
      "kernel"
      "adapters"
      "migration"
    ];
    full = order;
  };
}
