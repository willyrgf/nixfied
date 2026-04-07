{
  schemaVersion = 1;

  profileNames = [
    "feature-proof"
    "ci"
    "full"
  ];

  scenarios = {
    "scenario-1-public-surface-happy-path" = {
      checkName = "proof-workspace-scenario-1-public-surface-happy-path";
      ownerFile = "proof-workspace/scenarios/scenario-1-public-surface-happy-path.nix";
      profiles = [ ];
      enabled = false;
      tier = "pr";
    };

    "scenario-2-interruption-process-cleanup" = {
      checkName = "proof-workspace-scenario-2-interruption-process-cleanup";
      ownerFile = "proof-workspace/scenarios/scenario-2-interruption-process-cleanup.nix";
      profiles = [ ];
      enabled = false;
      tier = "pr";
    };

    "scenario-3-ephemeral-workspace" = {
      checkName = "proof-workspace-scenario-3-ephemeral-workspace";
      ownerFile = "proof-workspace/scenarios/scenario-3-ephemeral-workspace.nix";
      profiles = [ ];
      enabled = false;
      tier = "pre-merge";
    };

    "scenario-4-isolation-matrix" = {
      checkName = "proof-workspace-scenario-4-isolation-matrix";
      ownerFile = "proof-workspace/scenarios/scenario-4-isolation-matrix.nix";
      profiles = [ ];
      enabled = false;
      tier = "pre-merge";
    };

    "scenario-5-wrapper-roundtrip" = {
      checkName = "proof-workspace-scenario-5-wrapper-roundtrip";
      ownerFile = "proof-workspace/scenarios/scenario-5-wrapper-roundtrip.nix";
      profiles = [ ];
      enabled = false;
      tier = "pre-merge";
    };

    "scenario-6-failure-guardrails" = {
      checkName = "proof-workspace-scenario-6-failure-guardrails";
      ownerFile = "proof-workspace/scenarios/scenario-6-failure-guardrails.nix";
      profiles = [ ];
      enabled = false;
      tier = "pr";
    };
  };

  capabilities = {
    "runtime.artifact-placement-semantics" = {
      layer = "proof-workspace";
      scenarios = [
        "scenario-1-public-surface-happy-path"
        "scenario-4-isolation-matrix"
      ];
      replaces = [
        "tests/framework/artifacts-run-isolation-smoke.nix"
        "tests/framework/artifacts-root-override-isolation-smoke.nix"
      ];
    };

    "runtime.ephemeral.env-file-loading" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-3-ephemeral-workspace" ];
      replaces = [
        "tests/framework/env-loader-strict-smoke.nix"
        "tests/framework/ephemeral-runtime-env-isolation-smoke.nix"
      ];
    };

    "runtime.ephemeral.include-untracked" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-3-ephemeral-workspace" ];
      replaces = [ "tests/framework/ephemeral-runtime-behavior-smoke.nix" ];
    };

    "runtime.ephemeral.source-materialization" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-3-ephemeral-workspace" ];
      replaces = [
        "tests/framework/ephemeral-execution-smoke.nix"
        "tests/framework/ephemeral-copy-budget-smoke.nix"
        "tests/framework/ephemeral-retention-smoke.nix"
      ];
    };

    "runtime.install-semantics" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-5-wrapper-roundtrip" ];
      replaces = [
        "tests/framework/framework-install-thin-smoke.nix"
        "tests/framework/framework-install-vendor-smoke.nix"
        "tests/framework/framework-install-filter-smoke.nix"
      ];
    };

    "runtime.machine-output-behavior" = {
      layer = "proof-workspace";
      scenarios = [
        "scenario-1-public-surface-happy-path"
        "scenario-6-failure-guardrails"
      ];
      replaces = [ "tests/framework/machine-output-app-smoke.nix" ];
    };

    "runtime.output.prefix-contract" = {
      layer = "proof-workspace";
      scenarios = [
        "scenario-1-public-surface-happy-path"
        "scenario-6-failure-guardrails"
      ];
      replaces = [
        "tests/framework/discovery-command-surfaces-smoke.nix"
        "tests/framework/test-mode-cli-contract-smoke.nix"
      ];
    };

    "runtime.process-group-cleanup" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-2-interruption-process-cleanup" ];
      replaces = [
        "tests/framework/orchestrator-signal-cleanup-smoke.nix"
        "tests/framework/parallel-runner-process-tree-smoke.nix"
      ];
    };

    "runtime.registry.isolation" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-4-isolation-matrix" ];
      replaces = [
        "tests/framework/workspace-registry-isolation-smoke.nix"
        "tests/framework/service-dir-isolation-smoke.nix"
      ];
    };

    "runtime.service-operations" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-1-public-surface-happy-path" ];
      replaces = [
        "tests/framework/service-op-composition-contract.nix"
        "tests/framework/service-lifecycle-matrix-smoke.nix"
        "tests/framework/ready-health-matrix-smoke.nix"
        "tests/framework/ready-health-shutdown-smoke.nix"
      ];
    };

    "runtime.stop-run-semantics" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-2-interruption-process-cleanup" ];
      replaces = [ "tests/framework/orchestrator-stop-controls-smoke.nix" ];
    };

    "runtime.summary-sidecars" = {
      layer = "proof-workspace";
      scenarios = [
        "scenario-1-public-surface-happy-path"
        "scenario-2-interruption-process-cleanup"
      ];
      replaces = [
        "tests/framework/summary-json-smoke.nix"
        "tests/framework/workflow-lifecycle-smoke.nix"
      ];
    };

    "runtime.task-hooks" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-1-public-surface-happy-path" ];
      replaces = [ "tests/framework/task-hooks-smoke.nix" ];
    };

    "runtime.upgrade-semantics" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-5-wrapper-roundtrip" ];
      replaces = [
        "tests/framework/framework-upgrade-preserve-smoke.nix"
        "tests/framework/framework-template-install-upgrade-help-smoke.nix"
      ];
    };

    "runtime.workflow-interruption-semantics" = {
      layer = "proof-workspace";
      scenarios = [
        "scenario-2-interruption-process-cleanup"
        "scenario-6-failure-guardrails"
      ];
      replaces = [
        "tests/framework/orchestrator-stop-controls-smoke.nix"
        "tests/framework/orchestrator-signal-cleanup-smoke.nix"
      ];
    };

    "runtime.workflow-service-phases" = {
      layer = "proof-workspace";
      scenarios = [ "scenario-1-public-surface-happy-path" ];
      replaces = [
        "tests/framework/workflow-lifecycle-smoke.nix"
        "tests/framework/workflow-mode-derived-smoke.nix"
        "tests/framework/workflow-probe-scope-smoke.nix"
      ];
    };
  };

  deletionEligibility = {
    requiredProfiles = [
      "feature-proof"
      "ci"
      "full"
    ];

    firstBlock = [
      {
        checkName = "ci-mode-matrix-smoke";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
      {
        checkName = "discovery-command-surfaces-smoke";
        replacementCapabilities = [ "runtime.output.prefix-contract" ];
      }
      {
        checkName = "introspect-contract";
        replacementCapabilities = [ "runtime.output.prefix-contract" ];
      }
      {
        checkName = "local-override-introspect-contract";
        replacementCapabilities = [ "runtime.output.prefix-contract" ];
      }
      {
        checkName = "orchestrator-arg-forwarding-smoke";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
      {
        checkName = "orchestrator-signal-cleanup-smoke";
        replacementCapabilities = [
          "runtime.process-group-cleanup"
          "runtime.workflow-interruption-semantics"
        ];
      }
      {
        checkName = "orchestrator-stop-controls-smoke";
        replacementCapabilities = [
          "runtime.stop-run-semantics"
          "runtime.workflow-interruption-semantics"
        ];
      }
      {
        checkName = "parallel-runner-process-tree-smoke";
        replacementCapabilities = [ "runtime.process-group-cleanup" ];
      }
      {
        checkName = "parallel-runner-smoke";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
      {
        checkName = "parallel-worker-cap-invalid-smoke";
        replacementCapabilities = [ "runtime.workflow-interruption-semantics" ];
      }
      {
        checkName = "parallel-worker-cap-smoke";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
      {
        checkName = "ready-health-matrix-smoke";
        replacementCapabilities = [ "runtime.service-operations" ];
      }
      {
        checkName = "ready-health-shutdown-smoke";
        replacementCapabilities = [ "runtime.service-operations" ];
      }
      {
        checkName = "service-lifecycle-matrix-smoke";
        replacementCapabilities = [ "runtime.service-operations" ];
      }
      {
        checkName = "service-op-composition-contract";
        replacementCapabilities = [ "runtime.service-operations" ];
      }
      {
        checkName = "service-set-behavior-contract";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
      {
        checkName = "summary-json-smoke";
        replacementCapabilities = [ "runtime.summary-sidecars" ];
      }
      {
        checkName = "task-hooks-smoke";
        replacementCapabilities = [ "runtime.task-hooks" ];
      }
      {
        checkName = "workflow-lifecycle-smoke";
        replacementCapabilities = [
          "runtime.summary-sidecars"
          "runtime.workflow-service-phases"
        ];
      }
      {
        checkName = "workflow-mode-derived-smoke";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
      {
        checkName = "workflow-probe-scope-smoke";
        replacementCapabilities = [ "runtime.workflow-service-phases" ];
      }
    ];

    remaining = [ ];
  };
}
