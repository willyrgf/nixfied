{
  pkgs,
  model,
  services,
  serviceCatalog,
  stateHash,
  canonical,
  registry,
  packages,
  apps,
}:
let
  lib = pkgs.lib;
  listUtils = import ../../nixfied/framework/core/list-utils.nix;

  exposedTaskFeatureIds = builtins.sort builtins.lessThan (
    builtins.filter (taskId: taskId != null && taskId != "") (
      builtins.map (appName: model.views.apps.${appName}.taskId) (
        builtins.attrNames (model.views.apps or { })
      )
    )
  );
  workflowFeatureIds = builtins.sort builtins.lessThan (builtins.attrNames model.workflows);
  serviceFeatureIds = builtins.sort builtins.lessThan (builtins.attrNames serviceCatalog);

  defaultCheckKind = name: if lib.hasInfix "smoke" name then "smoke" else "contract";

  mkFrameworkCheck =
    name: metadata: drv:
    let
      existingPassThru = drv.passthru or { };
      kind = metadata.kind or defaultCheckKind name;
      covers = listUtils.uniquePreserveOrder (metadata.covers or [ ]);
      defaultOwnerFile =
        let
          checkPath = ./${name}.nix;
        in
        if builtins.pathExists checkPath then
          "tests/framework/${name}.nix"
        else
          "tests/framework/default.nix";
      ownerFiles = metadata.ownerFiles or [ defaultOwnerFile ];
      notes = metadata.notes or null;
    in
    drv
    // {
      passthru = existingPassThru // {
        nixfied = {
          inherit
            kind
            covers
            ownerFiles
            ;
        }
        // lib.optionalAttrs (notes != null && notes != "") { inherit notes; };
      };
    };

  checkMetadata = {
    "help-snapshot" = {
      covers = listUtils.uniquePreserveOrder (exposedTaskFeatureIds ++ workflowFeatureIds);
    };

    "compiler-validation" = {
      covers = serviceFeatureIds;
    };

    "log-prefix-contract" = {
      covers = [ "runtime.output.prefix-contract" ];
    };

    "ephemeral-copy-mode-smoke" = {
      covers = [ "runtime.ephemeral.include-untracked" ];
    };

    "ephemeral-env-file-mode-smoke" = {
      covers = [ "runtime.ephemeral.env-file-loading" ];
    };

    "ephemeral-nix-source-smoke" = {
      covers = [ "runtime.ephemeral.source-materialization" ];
    };

    "ephemeral-registry-run-isolation-smoke" = {
      covers = [ "runtime.registry.isolation" ];
    };

    "skip-service-smoke" = {
      covers = [
        "task.framework.test"
        "task.ops.health"
      ];
    };

    "service-requirements-contract" = {
      covers = [ "task.framework.test" ];
    };

    "selected-app-manifest-contract" = {
      covers = [ "runtime.app-execution-manifests" ];
    };

    "service-set-surface-contract" = {
      covers = [ "runtime.service-set-surfaces" ];
    };

    "workflow-service-set-adapter-smoke" = {
      covers = [ "runtime.service-set-surfaces" ];
    };

    "machine-output-app-smoke" = {
      covers = [ "runtime.app-execution-manifests" ];
    };

    "workflow-ref-app-manifest-contract" = {
      covers = [ "runtime.app-execution-manifests" ];
    };

    "launcher-surface-contract" = { };

    "framework-utility-launcher-contract" = { };

    "service-surface-catalog-contract" = { };

    "framework-install-no-caller-compile-smoke" = { };

    "framework-test-no-caller-compile-smoke" = { };

    "framework-upgrade-no-caller-compile-smoke" = { };

    "runtime-control-launcher-contract" = { };

    "runtime-controls-no-service-materialization-smoke" = { };

    "flake-show-no-service-materialization-smoke" = { };

    "run-id-noise-stability-smoke" = { };

    "run-id-semantic-inputs-contract" = { };

    "run-id-active-collision-suffix-smoke" = { };

    "unselected-service-no-package-resolution-smoke" = { };

    "unselected-service-public-launcher-smoke" = { };

    "selected-source-only-resolution-smoke" = { };

    "disabled-service-no-package-resolution-smoke" = { };

    "disabled-service-runtime-surface-smoke" = {
      covers = [ "runtime.service-hooks" ];
    };

    "launcher-skip-service-pruning-smoke" = { };

    "launcher-help-fast-path-smoke" = { };

    "dispatcher-help-fast-path-smoke" = { };

    "orchestrator-arg-forwarding-smoke" = { };

    "service-hook-env-smoke" = {
      covers = [ "runtime.service-hooks" ];
    };

    "runtime-service-selection-contract" = { };

    "framework-test-coverage-contract" = { };

    "contract-render-snapshot" = { };
  };

  rawChecks = {
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

    "introspection-schema" = pkgs.runCommand "framework-introspection-schema" { } ''
      ${pkgs.jq}/bin/jq -e '.required | index("serviceCatalog")' ${../../nixfied/schemas/model-export.json} > /dev/null
      ${pkgs.jq}/bin/jq -e '.properties.serviceCatalog.type == "object"' ${../../nixfied/schemas/model-export.json} > /dev/null
      ${pkgs.jq}/bin/jq -e '(.required | index("services")) == null' ${../../nixfied/schemas/model-export.json} > /dev/null
      ${pkgs.jq}/bin/jq -e '.required | index("features")' ${../../nixfied/schemas/model-export.json} > /dev/null
      ${pkgs.jq}/bin/jq -e '.properties.features.type == "object"' ${../../nixfied/schemas/model-export.json} > /dev/null
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

    "introspect-contract" = import ./introspect-contract.nix {
      inherit
        pkgs
        apps
        ;
    };

    "local-override-introspect-contract" = import ./local-override-introspect-contract.nix {
      inherit pkgs;
    };

    "selected-app-manifest-contract" = import ./selected-app-manifest-contract.nix {
      inherit
        pkgs
        apps
        ;
    };

    "workflow-ref-app-manifest-contract" = import ./workflow-ref-app-manifest-contract.nix {
      inherit pkgs;
    };

    "service-set-surface-contract" = import ./service-set-surface-contract.nix {
      inherit pkgs;
    };

    "workflow-service-set-adapter-smoke" = import ./workflow-service-set-adapter-smoke.nix {
      inherit pkgs;
    };

    "machine-output-app-smoke" = import ./machine-output-app-smoke.nix {
      inherit pkgs;
    };

    "launcher-surface-contract" = import ./launcher-surface-contract.nix {
      inherit
        pkgs
        model
        apps
        ;
    };

    "framework-utility-launcher-contract" = import ./framework-utility-launcher-contract.nix {
      inherit
        pkgs
        apps
        ;
    };

    "service-surface-catalog-contract" = import ./service-surface-catalog-contract.nix {
      inherit pkgs;
    };

    "contract-render-snapshot" = import ./contract-render-snapshot.nix {
      inherit pkgs;
    };

    "framework-install-no-caller-compile-smoke" =
      import ./framework-install-no-caller-compile-smoke.nix
        {
          inherit pkgs;
        };

    "framework-test-no-caller-compile-smoke" = import ./framework-test-no-caller-compile-smoke.nix {
      inherit pkgs;
    };

    "framework-upgrade-no-caller-compile-smoke" =
      import ./framework-upgrade-no-caller-compile-smoke.nix
        {
          inherit
            pkgs
            model
            services
            registry
            ;
        };

    "runtime-control-launcher-contract" = import ./runtime-control-launcher-contract.nix {
      inherit
        pkgs
        apps
        ;
    };

    "runtime-controls-no-service-materialization-smoke" =
      import ./runtime-controls-no-service-materialization-smoke.nix
        {
          inherit pkgs;
        };

    "flake-show-no-service-materialization-smoke" =
      import ./flake-show-no-service-materialization-smoke.nix
        {
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

    "unselected-service-no-package-resolution-smoke" =
      import ./unselected-service-no-package-resolution-smoke.nix
        {
          inherit pkgs;
        };

    "unselected-service-public-launcher-smoke" = import ./unselected-service-public-launcher-smoke.nix {
      inherit pkgs;
    };

    "selected-source-only-resolution-smoke" = import ./selected-source-only-resolution-smoke.nix {
      inherit pkgs;
    };

    "disabled-service-no-package-resolution-smoke" =
      import ./disabled-service-no-package-resolution-smoke.nix
        {
          inherit pkgs;
        };

    "disabled-service-runtime-surface-smoke" = import ./disabled-service-runtime-surface-smoke.nix {
      inherit pkgs;
    };

    "launcher-skip-service-pruning-smoke" = import ./launcher-skip-service-pruning-smoke.nix {
      inherit pkgs;
    };

    "launcher-help-fast-path-smoke" = import ./launcher-help-fast-path-smoke.nix {
      inherit pkgs;
    };

    "dispatcher-help-fast-path-smoke" = import ./dispatcher-help-fast-path-smoke.nix {
      inherit pkgs;
    };

    "orchestrator-arg-forwarding-smoke" = import ./orchestrator-arg-forwarding-smoke.nix {
      inherit pkgs;
    };

    "service-api-surface-contract" = import ./service-api-surface-contract.nix {
      inherit
        pkgs
        serviceCatalog
        apps
        ;
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

    "help-snapshot" = import ./help-snapshot.nix {
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
        services
        serviceCatalog
        ;
    };

    "executor-contract" = import ./executor-contract.nix {
      inherit pkgs;
    };

    "executor-runtime-contract" = import ./executor-runtime-contract.nix {
      inherit pkgs;
    };

    "env-sandbox-contract" = import ./env-sandbox-contract.nix {
      inherit pkgs;
    };

    "nix-client-env-smoke" = import ./nix-client-env-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "nix-ci-workflow-contract" = import ./nix-ci-workflow-contract.nix {
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

    "service-hook-env-smoke" = import ./service-hook-env-smoke.nix {
      inherit pkgs;
    };

    "runtime-service-selection-contract" = import ./runtime-service-selection-contract.nix {
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
        registry
        ;
    };

    "nix-checks-nil-issues-fail-smoke" = import ./nix-checks-nil-issues-fail-smoke.nix {
      inherit pkgs;
    };

    "skip-service-smoke" = import ./skip-service-smoke.nix {
      inherit
        pkgs
        registry
        ;
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

    "registry-events-contract" = import ./registry-events-contract.nix {
      inherit
        pkgs
        registry
        ;
    };

    "registry-events-runtime-contract" = import ./registry-events-runtime-contract.nix {
      inherit pkgs;
    };

    "log-prefix-contract" = import ./log-prefix-contract.nix {
      inherit pkgs;
    };

    "service-observability-contract" = import ./service-observability-contract.nix {
      inherit pkgs;
    };

    "runtime-events-contract" = import ./runtime-events-contract.nix {
      inherit pkgs;
    };

    "parallel-runner-smoke" = import ./parallel-runner-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "parallel-worker-cap-smoke" = import ./parallel-worker-cap-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "parallel-worker-cap-invalid-smoke" = import ./parallel-worker-cap-invalid-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "ci-mode-matrix-smoke" = import ./ci-mode-matrix-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "logging-injection-smoke" = import ./logging-injection-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "workflow-mode-derived-smoke" = import ./workflow-mode-derived-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "workflow-modes-contract" = import ./workflow-modes-contract.nix {
      inherit pkgs;
    };

    "managed-service-lifecycle-contract" = import ./managed-service-lifecycle-contract.nix {
      inherit pkgs;
    };

    "service-lifecycle-matrix-smoke" = import ./service-lifecycle-matrix-smoke.nix {
      inherit pkgs;
    };

    "supervisor-lifecycle-smoke" = import ./supervisor-lifecycle-smoke.nix {
      inherit pkgs;
    };

    "supervisor-runtime-contract" = import ./supervisor-runtime-contract.nix {
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

    "framework-test-cli-contract-smoke" = import ./framework-test-cli-contract-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "framework-test-coverage-contract" = import ./framework-test-coverage-contract.nix {
      inherit pkgs;
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
        services
        registry
        ;
    };

    "framework-install-thin-smoke" = import ./framework-install-thin-smoke.nix {
      inherit
        pkgs
        model
        services
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
            registry
            ;
        };

    "framework-upgrade-preserve-smoke" = import ./framework-upgrade-preserve-smoke.nix {
      inherit
        pkgs
        model
        services
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

    "nginx-site-management-contract" = import ./nginx-site-management-contract.nix {
      inherit pkgs;
    };

    "orchestrator-lifecycle-contract" = import ./orchestrator-lifecycle-contract.nix {
      inherit pkgs;
    };

    "orchestrator-runtime-contract" = import ./orchestrator-runtime-contract.nix {
      inherit pkgs;
    };

    "orchestrator-stop-controls-smoke" = import ./orchestrator-stop-controls-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "orchestrator-signal-cleanup-smoke" = import ./orchestrator-signal-cleanup-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "workflow-lifecycle-smoke" = import ./workflow-lifecycle-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "summary-json-smoke" = import ./summary-json-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "isolation-nested-run-id-smoke" = import ./isolation-nested-run-id-smoke.nix {
      inherit
        pkgs
        model
        services
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

    "ephemeral-env-file-mode-smoke" = import ./ephemeral-env-file-mode-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "runtime-env-isolation-smoke" = import ./runtime-env-isolation-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "ephemeral-registry-run-isolation-smoke" = import ./ephemeral-registry-run-isolation-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "ephemeral-copy-mode-smoke" = import ./ephemeral-copy-mode-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "ephemeral-nix-source-smoke" = import ./ephemeral-nix-source-smoke.nix {
      inherit
        pkgs
        model
        services
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
        registry
        ;
    };

    "workflow-validation-errors" = import ./workflow-validation-errors.nix {
      inherit pkgs;
    };

    "service-dir-isolation-smoke" = import ./service-dir-isolation-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };
  };

  baseChecks = lib.mapAttrs (
    name: drv: mkFrameworkCheck name (checkMetadata.${name} or { }) drv
  ) rawChecks;
in
baseChecks
// {
  "feature-coverage-validation" = mkFrameworkCheck "feature-coverage-validation" { } (
    import ./feature-coverage-validation.nix {
      inherit
        pkgs
        model
        ;
      checks = baseChecks;
    }
  );
}
