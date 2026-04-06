let
  selectionCatalog = import ../../nixfied/framework/testing/catalog.nix;
in
selectionCatalog
// {

  mkChecks =
    {
      pkgs,
      model,
      services,
      serviceDefinitions,
      serviceCatalog,
      stateHash,
      canonical,
      registry,
      packages,
      apps,
    }:
    let
      modelExportSchema = builtins.fromJSON (builtins.readFile ../../nixfied/schemas/model-export.json);
      modelExportRequired = modelExportSchema.required or [ ];
      modelExportProperties = modelExportSchema.properties or { };
    in
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

      "introspection-schema" =
        assert builtins.elem "serviceCatalog" modelExportRequired;
        assert (modelExportProperties.serviceCatalog.type or null) == "object";
        assert !(builtins.elem "services" modelExportRequired);
        assert builtins.elem "features" modelExportRequired;
        assert (modelExportProperties.features.type or null) == "object";
        pkgs.runCommand "framework-introspection-schema" { } ''
          echo "OK: model export schema includes serviceCatalog and features" > "$out"
        '';

      "package-output-contract" = import ./package-output-contract.nix {
        inherit
          pkgs
          serviceCatalog
          packages
          apps
          ;
      };

      "features-surface-contract" = import ./features-surface-contract.nix {
        inherit
          pkgs
          model
          apps
          ;
      };

      "introspect-contract" = import ./introspect-contract.nix {
        inherit
          pkgs
          apps
          ;
      };

      "introspection-bundle-determinism" = import ./introspection-bundle-determinism.nix {
        inherit pkgs;
      };

      "local-override-introspect-contract" = import ./local-override-introspect-contract.nix {
        inherit pkgs;
      };

      "machine-output-app-smoke" = import ./machine-output-app-smoke.nix {
        inherit pkgs;
      };

      "service-surface-catalog-contract" = import ./service-surface-catalog-contract.nix {
        inherit pkgs;
      };

      "contract-render-snapshot" = import ./contract-render-snapshot.nix {
        inherit pkgs;
      };

      "run-id-noise-stability-smoke" = import ./run-id-noise-stability-smoke.nix {
        inherit
          pkgs
          registry
          ;
      };

      "run-id-semantic-inputs-contract" = import ./run-id-semantic-inputs-contract.nix {
        inherit
          pkgs
          registry
          ;
      };

      "run-id-active-collision-suffix-smoke" = import ./run-id-active-collision-suffix-smoke.nix {
        inherit
          pkgs
          registry
          ;
      };

      "selected-source-only-resolution-smoke" = import ./selected-source-only-resolution-smoke.nix {
        inherit pkgs;
      };

      "disabled-service-no-package-resolution-smoke" =
        import ./disabled-service-no-package-resolution-smoke.nix
          {
            inherit pkgs;
          };

      "orchestrator-arg-forwarding-smoke" = import ./orchestrator-arg-forwarding-smoke.nix {
        inherit pkgs;
      };

      "vendored-metadata-contract" = import ./vendored-metadata-contract.nix {
        inherit pkgs;
      };

      "scheduler-order" = import ./scheduler-order.nix {
        inherit
          pkgs
          model
          ;
      };

      "docs-guidance-contract" = import ./docs-guidance-contract.nix {
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
          serviceDefinitions
          serviceCatalog
          ;
      };

      "nix-client-env-smoke" = import ./nix-client-env-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "nix-ci-workflow-contract" = import ./nix-ci-workflow-contract.nix {
        inherit pkgs;
      };

      "env-loader-strict-smoke" = import ./env-loader-strict-smoke.nix {
        inherit pkgs;
      };

      "shell-contract-runtime-smoke" = import ./shell-contract-runtime-smoke.nix {
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

      "nix-checks-parent-workflow-skip-smoke" = import ./nix-checks-parent-workflow-skip-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "nix-checks-visible-output-smoke" = import ./nix-checks-visible-output-smoke.nix {
        inherit pkgs;
      };

      "nix-checks-nil-issues-fail-smoke" = import ./nix-checks-nil-issues-fail-smoke.nix {
        inherit pkgs;
      };

      "nix-checks-deadnix-issues-fail-smoke" = import ./nix-checks-deadnix-issues-fail-smoke.nix {
        inherit pkgs;
      };

      "nix-checks-statix-issues-fail-smoke" = import ./nix-checks-statix-issues-fail-smoke.nix {
        inherit pkgs;
      };

      "excluded-service-evaluation" = import ./excluded-service-evaluation.nix {
        inherit
          pkgs
          registry
          ;
      };

      "service-requirements-contract" = import ./service-requirements-contract.nix {
        inherit
          pkgs
          registry
          ;
      };

      "service-op-composition-contract" = import ./service-op-composition-contract.nix {
        inherit pkgs;
      };

      "service-set-behavior-contract" = import ./service-set-behavior-contract.nix {
        inherit pkgs;
      };

      "operations-contract" = import ./operations-contract.nix {
        inherit
          pkgs
          model
          ;
      };

      "service-probe-overrides-contract" = import ./service-probe-overrides-contract.nix {
        inherit pkgs;
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

      "workflow-probe-scope-smoke" = import ./workflow-probe-scope-smoke.nix {
        inherit
          pkgs
          registry
          ;
      };

      "service-probe-overrides-smoke" = import ./service-probe-overrides-smoke.nix {
        inherit pkgs;
      };

      "postgres-kernel-probe-lifecycle-smoke" = import ./postgres-kernel-probe-lifecycle-smoke.nix {
        inherit pkgs;
      };

      "registry-events-runtime-contract" = import ./registry-events-runtime-contract.nix {
        inherit pkgs;
      };

      "registry-detail-derivation-smoke" = import ./registry-detail-derivation-smoke.nix {
        inherit pkgs;
      };

      "runtime-events-status-smoke" = import ./runtime-events-status-smoke.nix {
        inherit pkgs;
      };

      "runtime-events-policy-smoke" = import ./runtime-events-policy-smoke.nix {
        inherit pkgs;
      };

      "service-policy-runtime-smoke" = import ./service-policy-runtime-smoke.nix {
        inherit pkgs;
      };

      "parallel-runner-smoke" = import ./parallel-runner-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "parallel-runner-process-tree-smoke" = import ./parallel-runner-process-tree-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "parallel-worker-cap-smoke" = import ./parallel-worker-cap-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "parallel-worker-cap-invalid-smoke" = import ./parallel-worker-cap-invalid-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "ci-mode-matrix-smoke" = import ./ci-mode-matrix-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "logging-injection-smoke" = import ./logging-injection-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "workflow-mode-derived-smoke" = import ./workflow-mode-derived-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "service-lifecycle-matrix-smoke" = import ./service-lifecycle-matrix-smoke.nix {
        inherit pkgs;
      };

      "supervisor-lifecycle-smoke" = import ./supervisor-lifecycle-smoke.nix {
        inherit pkgs;
      };

      "task-hooks-smoke" = import ./task-hooks-smoke.nix {
        inherit
          pkgs
          model
          services
          registry
          ;
      };

      "test-mode-cli-contract-smoke" = import ./test-mode-cli-contract-smoke.nix { inherit pkgs; };

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
          services
          registry
          ;
      };

      "framework-install-thin-smoke" = import ./framework-install-thin-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "framework-install-filter-smoke" = import ./framework-install-filter-smoke.nix {
        inherit pkgs;
      };

      "caller-pwd-remote-projectroot-smoke" = import ./caller-pwd-remote-projectroot-smoke.nix {
        inherit
          pkgs
          model
          services
          registry
          ;
      };

      "framework-template-install-upgrade-help-smoke" =
        import ./framework-template-install-upgrade-help-smoke.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "framework-upgrade-preserve-smoke" = import ./framework-upgrade-preserve-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "vendored-metadata-packaged-source-smoke" = import ./vendored-metadata-packaged-source-smoke.nix {
        inherit pkgs;
      };

      "postgres-backup-restore-smoke" = import ./postgres-backup-restore-smoke.nix {
        inherit pkgs;
      };

      "postgres-config-artifacts-contract" = import ./postgres-config-artifacts-contract.nix {
        inherit pkgs;
      };

      "postgres-config-artifacts-smoke" = import ./postgres-config-artifacts-smoke.nix {
        inherit pkgs;
      };

      "nginx-site-management-smoke" = import ./nginx-site-management-smoke.nix {
        inherit pkgs;
      };

      "orchestrator-stop-controls-smoke" = import ./orchestrator-stop-controls-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "orchestrator-signal-cleanup-smoke" = import ./orchestrator-signal-cleanup-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "workflow-lifecycle-smoke" = import ./workflow-lifecycle-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "summary-json-smoke" = import ./summary-json-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "isolation-nested-run-id-smoke" = import ./isolation-nested-run-id-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
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
          services
          serviceDefinitions
          registry
          ;
      };

      "ephemeral-execution-smoke" = import ./ephemeral-execution-smoke.nix {
        inherit
          pkgs
          model
          services
          registry
          ;
      };

      "ephemeral-runtime-env-isolation-smoke" = import ./ephemeral-runtime-env-isolation-smoke.nix {
        inherit
          pkgs
          model
          services
          registry
          ;
      };

      "ephemeral-runtime-behavior-smoke" = import ./ephemeral-runtime-behavior-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "runtime-env-isolation-smoke" = import ./runtime-env-isolation-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };

      "ephemeral-retention-smoke" = import ./ephemeral-retention-smoke.nix {
        inherit
          pkgs
          model
          services
          registry
          ;
      };

      "ephemeral-copy-budget-smoke" = import ./ephemeral-copy-budget-smoke.nix {
        inherit
          pkgs
          model
          services
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
          services
          serviceDefinitions
          registry
          ;
      };

      "run-record-validator-failure" = import ./run-record-validator-failure.nix {
        inherit pkgs;
      };

      "kernel-native-tests" = import ./kernel-native-tests.nix {
        inherit pkgs;
      };

      "workflow-validation-errors" = import ./workflow-validation-errors.nix {
        inherit pkgs;
      };

      "service-dir-isolation-smoke" = import ./service-dir-isolation-smoke.nix {
        inherit
          pkgs
          model
          services
          serviceDefinitions
          registry
          ;
      };
    };
}
