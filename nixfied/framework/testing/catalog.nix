let
  order = [
    "compile"
    "manifest"
    "kernel"
    "adapters"
    "e2e"
    "migration"
  ];

  profileNames = [
    "feature-proof"
    "ci"
    "full"
  ];

  shardChecks = {
    compile = [
      "compiler-validation"
      "contract-render-snapshot"
      "cross-machine-hash"
      "docs-guidance-contract"
      "excluded-service-evaluation"
      "features-surface-contract"
      "framework-selfhost-contract"
      "helios-pinned-source-contract"
      "introspect-contract"
      "introspection-bundle-determinism"
      "introspection-schema"
      "local-override-introspect-contract"
      "model-hash"
      "operations-contract"
      "package-output-contract"
      "postgres-config-artifacts-contract"
      "project-config-boundary"
      "scheduler-order"
      "service-op-composition-contract"
      "service-probe-overrides-contract"
      "service-requirements-contract"
      "service-surface-catalog-contract"
      "vendored-metadata-contract"
    ];

    manifest = [
      "machine-output-app-smoke"
      "service-set-behavior-contract"
    ];

    kernel = [
      "ephemeral-runtime-env-isolation-smoke"
      "isolation-nested-run-id-smoke"
      "kernel-native-tests"
      "nix-checks-parent-workflow-skip-smoke"
      "nix-ci-workflow-contract"
      "parallel-runner-process-tree-smoke"
      "parallel-runner-smoke"
      "parallel-worker-cap-invalid-smoke"
      "parallel-worker-cap-smoke"
      "postgres-kernel-probe-lifecycle-smoke"
      "registry-detail-derivation-smoke"
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
      "workflow-probe-scope-smoke"
      "workflow-validation-errors"
      "workspace-registry-isolation-smoke"
    ];

    adapters = [
      "dispatcher-help-fast-path-smoke"
      "launcher-help-fast-path-smoke"
      "launcher-skip-service-pruning-smoke"
      "orchestrator-arg-forwarding-smoke"
      "orchestrator-signal-cleanup-smoke"
      "service-hook-env-smoke"
      "shell-contract-runtime-smoke"
      "unselected-service-public-launcher-smoke"
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
      "ephemeral-execution-smoke"
      "ephemeral-runtime-behavior-smoke"
      "ephemeral-retention-smoke"
      "flake-show-no-service-materialization-smoke"
      "framework-install-filter-smoke"
      "framework-install-no-caller-compile-smoke"
      "framework-install-thin-smoke"
      "framework-install-vendor-smoke"
      "framework-template-install-upgrade-help-smoke"
      "framework-upgrade-no-caller-compile-smoke"
      "framework-upgrade-preserve-smoke"
      "logging-injection-smoke"
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
      "skip-service-smoke"
      "slot-env-runtime-smoke"
      "supervisor-lifecycle-smoke"
      "task-hooks-smoke"
      "test-mode-cli-contract-smoke"
      "unselected-service-no-package-resolution-smoke"
      "vendored-metadata-packaged-source-smoke"
    ];

    migration = [
      "no-legacy-project-modules"
    ];
  };

  featureProofShardChecks = {
    compile = [ "features-surface-contract" ];
    manifest = [
      "machine-output-app-smoke"
      "service-set-behavior-contract"
    ];
    kernel = [ ];
    adapters = [ "service-hook-env-smoke" ];
    e2e = [ "ephemeral-runtime-behavior-smoke" ];
    migration = [ ];
  };

  ciShardChecks = {
    compile = shardChecks.compile;
    manifest = shardChecks.manifest;
    kernel = shardChecks.kernel;
    adapters = shardChecks.adapters;
    e2e = [ ];
    migration = shardChecks.migration;
  };

  profileShardChecks = {
    "feature-proof" = featureProofShardChecks;
    ci = ciShardChecks;
    full = shardChecks;
  };
in
{
  inherit
    order
    profileNames
    profileShardChecks
    ;

  checks = shardChecks;
}
