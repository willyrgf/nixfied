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
  modelExportSchema = builtins.fromJSON (builtins.readFile ../../nixfied/schemas/model-export.json);
  modelExportRequired = modelExportSchema.required or [ ];
  modelExportProperties = modelExportSchema.properties or { };

  defaultCheckKind = name: if lib.hasInfix "smoke" name then "smoke" else "contract";
  defaultCheckLayer =
    name: kind:
    if
      name == "contract-migration-guard"
      || name == "framework-test-coverage-contract"
      || name == "workflow-modes-contract"
    then
      "migration"
    else if lib.hasInfix "manifest" name || name == "runtime-service-selection-contract" then
      "manifest"
    else if
      lib.hasInfix "launcher" name
      || lib.hasInfix "dispatcher-help" name
      || lib.hasInfix "shell-contract" name
      || lib.hasInfix "arg-forwarding" name
      || lib.hasInfix "signal-cleanup" name
      || lib.hasInfix "service-hook" name
    then
      "adapter"
    else if
      lib.hasInfix "workflow-" name
      || lib.hasInfix "registry" name
      || lib.hasInfix "run-id" name
      || lib.hasInfix "run-record" name
      || lib.hasInfix "runtime-events" name
      || lib.hasInfix "summary" name
      || lib.hasInfix "parallel-runner" name
      || lib.hasInfix "parallel-worker" name
      || lib.hasInfix "executor-runtime" name
      || lib.hasInfix "orchestrator-runtime" name
      || lib.hasInfix "runtime-env" name
      || lib.hasInfix "service-policy-runtime" name
      || lib.hasInfix "postgres-kernel-probe" name
    then
      "kernel"
    else if
      name == "help-snapshot"
      || name == "compiler-validation"
      || lib.hasInfix "package-output" name
      || lib.hasInfix "project-config" name
      || lib.hasInfix "no-legacy" name
      || lib.hasInfix "introspect" name
      || lib.hasInfix "service-surface-catalog" name
      || lib.hasInfix "operations-contract" name
      || lib.hasInfix "docs-guidance" name
    then
      "compile"
    else if kind == "smoke" then
      "e2e"
    else
      "compile";
  defaultProofKind =
    name: kind:
    if
      name == "contract-migration-guard"
      || name == "framework-test-coverage-contract"
      || name == "workflow-modes-contract"
    then
      "guard"
    else if lib.hasInfix "snapshot" name then
      "fixture"
    else if kind == "smoke" then
      "smoke"
    else
      "contract";

  mkFrameworkCheck =
    name: metadata: drv:
    let
      existingPassThru = drv.passthru or { };
      kind = metadata.kind or defaultCheckKind name;
      layer = metadata.layer or (defaultCheckLayer name kind);
      proofKind = metadata.proofKind or (defaultProofKind name kind);
      canonical = metadata.canonical or false;
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
            layer
            proofKind
            canonical
            covers
            ownerFiles
            ;
        }
        // lib.optionalAttrs (notes != null && notes != "") { inherit notes; };
      };
    };

  checkMetadata = {
    "help-snapshot" = {
      layer = "compile";
      proofKind = "fixture";
      canonical = true;
      covers = listUtils.uniquePreserveOrder (exposedTaskFeatureIds ++ workflowFeatureIds);
    };

    "compiler-validation" = {
      layer = "compile";
      proofKind = "contract";
      canonical = true;
      covers = serviceFeatureIds;
    };

    "log-prefix-contract" = {
      layer = "adapter";
      proofKind = "contract";
      canonical = true;
      covers = [ "runtime.output.prefix-contract" ];
    };

    "ephemeral-copy-mode-smoke" = {
      layer = "e2e";
      proofKind = "smoke";
      canonical = true;
      covers = [ "runtime.ephemeral.include-untracked" ];
    };

    "ephemeral-env-file-mode-smoke" = {
      layer = "e2e";
      proofKind = "smoke";
      canonical = true;
      covers = [ "runtime.ephemeral.env-file-loading" ];
    };

    "ephemeral-nix-source-smoke" = {
      layer = "e2e";
      proofKind = "smoke";
      canonical = true;
      covers = [ "runtime.ephemeral.source-materialization" ];
    };

    "ephemeral-registry-run-isolation-smoke" = {
      layer = "e2e";
      proofKind = "smoke";
      canonical = true;
      covers = [ "runtime.registry.isolation" ];
    };

    "skip-service-smoke" = {
      layer = "e2e";
      proofKind = "smoke";
    };

    "service-requirements-contract" = {
      layer = "compile";
      proofKind = "contract";
    };

    "selected-app-manifest-contract" = {
      layer = "adapter";
      proofKind = "contract";
      canonical = false;
      covers = [ "runtime.app-execution-manifests" ];
    };

    "service-set-surface-contract" = {
      layer = "e2e";
      proofKind = "contract";
      canonical = false;
      covers = [ "runtime.service-set-surfaces" ];
    };

    "runtime-manifest-fixture-contract" = {
      layer = "manifest";
      proofKind = "fixture";
      canonical = true;
      covers = [
        "runtime.app-execution-manifests"
        "runtime.service-set-surfaces"
      ];
    };

    "introspection-bundle-determinism" = { };

    "kernel-native-tests" = {
      layer = "kernel";
      proofKind = "contract";
    };

    "run-record-validator-failure" = { };

    "workflow-service-set-adapter-smoke" = {
      layer = "adapter";
      proofKind = "smoke";
      covers = [ "runtime.service-set-surfaces" ];
    };

    "machine-output-app-smoke" = {
      layer = "e2e";
      proofKind = "smoke";
      covers = [ "runtime.app-execution-manifests" ];
    };

    "workflow-ref-app-manifest-contract" = {
      layer = "adapter";
      proofKind = "contract";
      covers = [ "runtime.app-execution-manifests" ];
    };

    "helpers-runtime-contract" = {
      layer = "adapter";
      proofKind = "contract";
    };

    "runtime-events-contract" = {
      layer = "adapter";
      proofKind = "contract";
    };

    "registry-helper-contract" = {
      layer = "migration";
      proofKind = "guard";
    };

    "service-api-surface-contract" = {
      layer = "migration";
      proofKind = "guard";
    };

    "service-extractability-contract" = {
      layer = "compile";
      proofKind = "contract";
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
      layer = "adapter";
      covers = [ "runtime.service-hooks" ];
    };

    "launcher-skip-service-pruning-smoke" = { };

    "launcher-help-fast-path-smoke" = { };

    "dispatcher-help-fast-path-smoke" = { };

    "orchestrator-arg-forwarding-smoke" = { };

    "service-hook-env-smoke" = {
      layer = "adapter";
      proofKind = "smoke";
      canonical = true;
      covers = [ "runtime.service-hooks" ];
    };

    "service-op-composition-contract" = { };

    "runtime-service-selection-contract" = {
      layer = "manifest";
      proofKind = "contract";
    };

    "framework-test-coverage-contract" = {
      layer = "migration";
      proofKind = "guard";
    };

    "framework-test-shard-validation" = {
      layer = "migration";
      proofKind = "guard";
    };

    "contract-render-snapshot" = { };

    "contract-migration-guard" = {
      layer = "migration";
      proofKind = "guard";
    };
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

    "selected-app-manifest-contract" = import ./selected-app-manifest-contract.nix {
      inherit
        pkgs
        apps
        ;
    };

    "runtime-manifest-fixture-contract" = import ./runtime-manifest-fixture-contract.nix {
      inherit pkgs;
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

    "service-extractability-contract" = import ./service-extractability-contract.nix {
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

    "discovery-runtime-contract" = import ./discovery-runtime-contract.nix {
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

    "service-op-composition-contract" = import ./service-op-composition-contract.nix {
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

    "postgres-kernel-probe-lifecycle-smoke" = import ./postgres-kernel-probe-lifecycle-smoke.nix {
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

    "registry-detail-derivation-smoke" = import ./registry-detail-derivation-smoke.nix {
      inherit pkgs;
    };

    "registry-helper-contract" = import ./registry-helper-contract.nix {
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

    "runtime-events-status-smoke" = import ./runtime-events-status-smoke.nix {
      inherit pkgs;
    };

    "runtime-events-policy-smoke" = import ./runtime-events-policy-smoke.nix {
      inherit pkgs;
    };

    "service-policy-runtime-smoke" = import ./service-policy-runtime-smoke.nix {
      inherit pkgs;
    };

    "contract-migration-guard" = import ./contract-migration-guard.nix {
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

    "parallel-runner-process-tree-smoke" = import ./parallel-runner-process-tree-smoke.nix {
      inherit
        pkgs
        model
        services
        registry
        ;
    };

    "workflow-parallel-blocked-smoke" = import ./workflow-parallel-blocked-smoke.nix {
      inherit pkgs;
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

    "postgres-backup-contract" = import ./postgres-backup-contract.nix {
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
        registry
        ;
    };
  };

  baseChecks = lib.mapAttrs (
    name: drv: mkFrameworkCheck name (checkMetadata.${name} or { }) drv
  ) rawChecks;
  featureCoverageValidationCheck =
    mkFrameworkCheck "feature-coverage-validation"
      {
        layer = "compile";
        proofKind = "guard";
      }
      (
        import ./feature-coverage-validation.nix {
          inherit
            pkgs
            model
            ;
          checks = baseChecks;
        }
      );
  frameworkTestShardValidationCheck =
    mkFrameworkCheck "framework-test-shard-validation"
      {
        layer = "migration";
        proofKind = "guard";
      }
      (
        import ./framework-test-shard-validation.nix {
          inherit pkgs;
          checks = baseChecks // {
            "feature-coverage-validation" = featureCoverageValidationCheck;
          };
        }
      );
  finalChecks = baseChecks // {
    "framework-test-shard-validation" = frameworkTestShardValidationCheck;
    "feature-coverage-validation" = featureCoverageValidationCheck;
  };
in
finalChecks
