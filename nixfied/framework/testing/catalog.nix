let
  coverageMap = import ../../../proof-workspace/scenarios/coverage-map.nix;
  coverageScenarios = coverageMap.scenarios or { };
  coverageCapabilities = coverageMap.capabilities or { };
  safeRepoRoot = builtins.unsafeDiscardStringContext (builtins.toString ../../..);
  historicalGreenEvidence = coverageMap.historicalGreenEvidence or { };
  historicalArtifactRelPath = historicalGreenEvidence.artifactFile or "";
  historicalArtifactPath =
    if historicalArtifactRelPath == "" then "" else "${safeRepoRoot}/${historicalArtifactRelPath}";
  historicalGreenRecords =
    if historicalArtifactRelPath != "" && builtins.pathExists historicalArtifactPath then
      (builtins.fromJSON (builtins.readFile historicalArtifactPath)).records or [ ]
    else
      [ ];
  deletionEligibility = coverageMap.deletionEligibility or { };
  deletionRequiredProfiles = deletionEligibility.requiredProfiles or [ ];
  firstBlockDeletionEntries = deletionEligibility.firstBlock or [ ];
  remainingDeletionEntries = deletionEligibility.remaining or [ ];
  gatedDeletionEntries = firstBlockDeletionEntries ++ remainingDeletionEntries;
  scenarioIds = builtins.sort builtins.lessThan (builtins.attrNames coverageScenarios);

  enabledScenarioChecksForProfile =
    profileName:
    builtins.sort builtins.lessThan (
      builtins.concatLists (
        map (
          scenarioId:
          let
            scenario = coverageScenarios.${scenarioId};
            enabled = scenario.enabled or false;
            profiles = scenario.profiles or [ ];
            checkName = scenario.checkName or "";
          in
          if enabled && checkName != "" && builtins.elem profileName profiles then [ checkName ] else [ ]
        ) scenarioIds
      )
    );

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

  baseShardChecks = {
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
      "proof-workspace-coverage-validation"
      "project-config-boundary"
      "scheduler-order"
      "service-probe-overrides-contract"
      "service-requirements-contract"
      "service-surface-catalog-contract"
      "vendored-metadata-contract"
    ];

    manifest = [ ];

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
      "orchestrator-arg-forwarding-smoke"
      "orchestrator-signal-cleanup-smoke"
      "service-op-composition-contract"
      "service-set-behavior-contract"
      "shell-contract-runtime-smoke"
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
      "framework-install-filter-smoke"
      "framework-install-thin-smoke"
      "framework-install-vendor-smoke"
      "framework-template-install-upgrade-help-smoke"
      "framework-upgrade-preserve-smoke"
      "logging-injection-smoke"
      "nix-checks-deadnix-issues-fail-smoke"
      "nix-checks-visible-output-smoke"
      "nginx-site-management-smoke"
      "nix-checks-nil-issues-fail-smoke"
      "nix-checks-statix-issues-fail-smoke"
      "nix-client-env-smoke"
      "orchestrator-stop-controls-smoke"
      "postgres-backup-restore-smoke"
      "postgres-config-artifacts-smoke"
      "ready-health-matrix-smoke"
      "ready-health-shutdown-smoke"
      "ready-helios-sync-gate-smoke"
      "selected-source-only-resolution-smoke"
      "service-dir-isolation-smoke"
      "service-lifecycle-matrix-smoke"
      "service-probe-overrides-smoke"
      "slot-env-runtime-smoke"
      "supervisor-lifecycle-smoke"
      "task-hooks-smoke"
      "test-mode-cli-contract-smoke"
      "vendored-metadata-packaged-source-smoke"
    ]
    ++ enabledScenarioChecksForProfile "full";

    migration = [
      "no-legacy-project-modules"
    ];
  };

  baseFeatureProofShardChecks = {
    compile = [ "features-surface-contract" ];
    manifest = [ ];
    kernel = [ ];
    adapters = [ ];
    e2e = enabledScenarioChecksForProfile "feature-proof";
    migration = [ ];
  };

  baseCiShardChecks = {
    inherit (baseShardChecks)
      compile
      manifest
      kernel
      adapters
      migration
      ;
    e2e = enabledScenarioChecksForProfile "ci";
  };

  baseProfileShardChecks = {
    "feature-proof" = baseFeatureProofShardChecks;
    ci = baseCiShardChecks;
    full = baseShardChecks;
  };

  allProfileChecksFrom =
    profileShardChecks: profileName:
    builtins.concatLists (map (shardName: profileShardChecks.${profileName}.${shardName} or [ ]) order);

  scenarioHasHistoricalGreenRecord =
    scenarioId: profileName:
    builtins.any (
      record:
      (record.scenarioId or "") == scenarioId
      && (record.profile or "") == profileName
      && (record.status or "") == "green"
      && builtins.isAttrs (record.runState or { })
      && (record.runState.result or "") != ""
    ) historicalGreenRecords;

  replacementCapabilityIsGreen =
    requiredProfiles: capabilityId:
    let
      capabilityExists = builtins.hasAttr capabilityId coverageCapabilities;
      capability =
        if capabilityExists then
          coverageCapabilities.${capabilityId}
        else
          { };
      layer = capability.layer or "";
      scenarioRefs = capability.scenarios or [ ];
      scenarioCheckForProfile =
        profileName:
        builtins.any (
          scenarioId:
          if !(builtins.hasAttr scenarioId coverageScenarios) then
            false
          else
            let
              scenario = coverageScenarios.${scenarioId};
              enabled = scenario.enabled or false;
              checkName = scenario.checkName or "";
            in
            enabled
            && builtins.elem checkName (allProfileChecksFrom baseProfileShardChecks profileName)
            && scenarioHasHistoricalGreenRecord scenarioId profileName
        ) scenarioRefs;
    in
    capabilityExists
    && layer == "proof-workspace"
    && scenarioRefs != [ ]
    && requiredProfiles != [ ]
    && builtins.all scenarioCheckForProfile requiredProfiles;

  deleteCheckEntryIsGreen =
    entry:
    let
      checkName = entry.checkName or "";
      replacementCapabilities = entry.replacementCapabilities or [ ];
      requiredProfiles = entry.requiredProfiles or deletionRequiredProfiles;
    in
    checkName != ""
    && replacementCapabilities != [ ]
    && builtins.all (capabilityId: replacementCapabilityIsGreen requiredProfiles capabilityId) replacementCapabilities;

  gatedDeletedChecks =
    builtins.foldl'
      (
        acc: entry:
        let
          checkName = entry.checkName or "";
        in
        if deleteCheckEntryIsGreen entry && !(builtins.elem checkName acc) then acc ++ [ checkName ] else acc
      )
      [ ]
      gatedDeletionEntries;

  removeDeletedChecks = checks: builtins.filter (checkName: !(builtins.elem checkName gatedDeletedChecks)) checks;

  shardChecks = builtins.mapAttrs (_: removeDeletedChecks) baseShardChecks;

  featureProofShardChecks = builtins.mapAttrs (_: removeDeletedChecks) baseFeatureProofShardChecks;

  ciShardChecks = builtins.mapAttrs (_: removeDeletedChecks) baseCiShardChecks;

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
