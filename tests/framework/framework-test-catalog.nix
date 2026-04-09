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
          ;
      };

      "introspection-bundle-determinism" = import ./introspection-bundle-determinism.nix {
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

      "proof-workspace-coverage-validation" = import ./proof-workspace-coverage-validation.nix {
        inherit
          pkgs
          model
          ;
      };

      "proof-workspace-scenario-1-public-surface-happy-path" =
        import ../../proof-workspace/scenarios/scenario-1-public-surface-happy-path.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "proof-workspace-scenario-2-interruption-process-cleanup" =
        import ../../proof-workspace/scenarios/scenario-2-interruption-process-cleanup.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "proof-workspace-scenario-3-ephemeral-workspace" =
        import ../../proof-workspace/scenarios/scenario-3-ephemeral-workspace.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "proof-workspace-scenario-4-isolation-matrix" =
        import ../../proof-workspace/scenarios/scenario-4-isolation-matrix.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "proof-workspace-scenario-5-wrapper-roundtrip" =
        import ../../proof-workspace/scenarios/scenario-5-wrapper-roundtrip.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "proof-workspace-scenario-6-failure-guardrails" =
        import ../../proof-workspace/scenarios/scenario-6-failure-guardrails.nix
          {
            inherit
              pkgs
              model
              services
              serviceDefinitions
              registry
              ;
          };

      "nix-client-env-smoke" = import ./nix-client-env-smoke.nix {
        inherit
          pkgs
          model
          services
          ;
      };

      "nix-ci-workflow-contract" = import ./nix-ci-workflow-contract.nix {
        inherit pkgs;
      };

      "shell-contract-runtime-smoke" = import ./shell-contract-runtime-smoke.nix {
        inherit pkgs;
      };

      "slot-env-runtime-smoke" = import ./slot-env-runtime-smoke.nix {
        inherit pkgs;
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

      "ready-helios-sync-gate-smoke" = import ./ready-helios-sync-gate-smoke.nix {
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

      "logging-injection-smoke" = import ./logging-injection-smoke.nix {
        inherit
          pkgs
          model
          services
          ;
      };

      "supervisor-lifecycle-smoke" = import ./supervisor-lifecycle-smoke.nix {
        inherit pkgs;
      };

      "framework-selfhost-contract" = import ./framework-selfhost-contract.nix {
        inherit
          pkgs
          model
          ;
      };

      "caller-pwd-remote-projectroot-smoke" = import ./caller-pwd-remote-projectroot-smoke.nix {
        inherit
          pkgs
          model
          services
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

      "isolation-nested-run-id-smoke" = import ./isolation-nested-run-id-smoke.nix {
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

    };
}
