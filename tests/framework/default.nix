{
  pkgs,
  model,
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
    builtins.map (appName: model.views.apps.${appName}.taskId) (
      builtins.attrNames (model.views.apps or { })
    )
  );
  workflowFeatureIds = builtins.sort builtins.lessThan (builtins.attrNames model.workflows);
  serviceFeatureIds = builtins.sort builtins.lessThan (builtins.attrNames model.services);

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

    "launcher-surface-contract" = { };

    "disabled-service-runtime-surface-smoke" = {
      covers = [ "runtime.service-hooks" ];
    };

    "launcher-skip-service-pruning-smoke" = { };

    "launcher-help-fast-path-smoke" = { };

    "service-hook-env-smoke" = {
      covers = [ "runtime.service-hooks" ];
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

    "introspection-schema" = pkgs.runCommand "framework-introspection-schema" { } ''
      ${pkgs.jq}/bin/jq -e '.required | index("features")' ${../../nixfied/schemas/model-export.json} > /dev/null
      ${pkgs.jq}/bin/jq -e '.properties.features.type == "object"' ${../../nixfied/schemas/model-export.json} > /dev/null
      echo "OK: model export schema includes features" > "$out"
    '';

    "package-output-contract" = import ./package-output-contract.nix {
      inherit
        pkgs
        model
        packages
        apps
        ;
    };

    "launcher-surface-contract" = import ./launcher-surface-contract.nix {
      inherit
        pkgs
        model
        apps
        ;
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

    "service-api-surface-contract" = import ./service-api-surface-contract.nix {
      inherit
        pkgs
        model
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
        registry
        ;
    };

    "orchestrator-signal-cleanup-smoke" = import ./orchestrator-signal-cleanup-smoke.nix {
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

    "workflow-validation-errors" = import ./workflow-validation-errors.nix {
      inherit pkgs;
    };

    "service-dir-isolation-smoke" = import ./service-dir-isolation-smoke.nix {
      inherit
        pkgs
        model
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
