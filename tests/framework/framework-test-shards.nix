let
  listUtils = import ../../nixfied/framework/core/list-utils.nix;

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
      "features-surface-contract"
      "framework-selfhost-contract"
      "framework-test-layout-validation"
      "helios-pinned-source-contract"
      "install-runtime-contract"
      "introspect-contract"
      "introspection-bundle-determinism"
      "introspection-schema"
      "local-override-introspect-contract"
      "managed-service-lifecycle-contract"
      "model-hash"
      "nginx-site-management-contract"
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
      "feature-manifest-proof"
      "selected-execution-contract"
    ];

    kernel = [
      "ephemeral-runtime-env-isolation-smoke"
      "runtime-handoff-contract"
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
      "dispatcher-help-fast-path-smoke"
      "feature-adapter-proof"
      "framework-utility-launcher-contract"
      "helpers-runtime-contract"
      "launcher-help-fast-path-smoke"
      "launcher-skip-service-pruning-smoke"
      "launcher-surface-contract"
      "orchestrator-arg-forwarding-smoke"
      "orchestrator-signal-cleanup-smoke"
      "runtime-events-contract"
      "service-hook-env-smoke"
      "shell-contract-contract"
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
      "feature-e2e-proof"
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
      "service-set-behavior-contract"
      "skip-service-smoke"
      "slot-env-runtime-smoke"
      "supervisor-lifecycle-smoke"
      "task-hooks-smoke"
      "unselected-service-no-package-resolution-smoke"
      "vendored-metadata-packaged-source-smoke"
    ];

    migration = [
      "contract-migration-guard"
      "no-legacy-project-modules"
    ];
  };

  order = [
    "compile"
    "manifest"
    "kernel"
    "adapters"
    "e2e"
    "migration"
  ];

  shardChecks = {
    compile = layerChecks.compile;
    manifest = layerChecks.manifest;
    kernel = layerChecks.kernel;
    adapters = layerChecks.adapter;
    e2e = layerChecks.e2e;
    migration = layerChecks.migration;
  };

  canonicalFeatureProofChecks = [
    "compiler-validation"
    "feature-adapter-proof"
    "feature-manifest-proof"
    "feature-e2e-proof"
  ];

  featureProofChecks = canonicalFeatureProofChecks ++ [ "features-surface-contract" ];

  allChecks = listUtils.uniquePreserveOrder (
    builtins.concatLists (builtins.map (name: shardChecks.${name} or [ ]) order)
  );

  profiles = {
    "feature-proof" = featureProofChecks;
    ci = listUtils.uniquePreserveOrder (
      canonicalFeatureProofChecks
      ++ shardChecks.compile
      ++ shardChecks.manifest
      ++ shardChecks.kernel
      ++ shardChecks.adapters
      ++ shardChecks.migration
    );
    full = allChecks;
  };

  profileShardChecks = builtins.mapAttrs (
    _: profileChecks:
    builtins.mapAttrs (
      _: shardCheckNames: builtins.filter (name: builtins.elem name profileChecks) shardCheckNames
    ) shardChecks
  ) profiles;
in
{
  inherit
    order
    layerChecks
    allChecks
    canonicalFeatureProofChecks
    featureProofChecks
    profileShardChecks
    ;

  descriptions = {
    compile = "Build compile-time model, help, schema, documentation, and governance proofs.";
    manifest = "Build manifest-owned feature and fixture contracts.";
    kernel = "Build kernel-owned runtime semantics, registry, and workflow proofs.";
    adapters = "Build thin launcher, shell, and process-edge adapter proofs.";
    e2e = "Build end-to-end public behavior, install, upgrade, isolation, and runtime smokes.";
    migration = "Build deleted-seam guards and ownership-migration regressions.";
  };

  profileDescriptions = {
    "feature-proof" = "Run only direct feature proofs backed by covers metadata.";
    ci = "Run canonical feature proofs plus compile, manifest, kernel, adapters, and migration shards.";
    full = "Run every registered framework check.";
  };

  checks = shardChecks;
  profiles = profiles;
}
