{ runtime }:
{
  "runtime.ephemeral.source-materialization" = {
    kind = "runtime";
    summary = "Ephemeral source materialization defaults";
    surfaces = [
      {
        kind = "workflow-ephemeral";
        name = "ephemeral";
      }
    ];
    ownerFiles = [
      "nixfied/project/conf.nix"
      "nixfied/project/runtime.nix"
      "nixfied/framework/runtime/ephemeral.nix"
    ];
    modelPaths = [ "runtime.ephemeral.copyMode" ];
    status = "stable";
    defaults = {
      inherit (runtime.ephemeral) copyMode;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.ephemeral.include-untracked" = {
    kind = "runtime";
    summary = "Ephemeral worktree copy policy";
    surfaces = [
      {
        kind = "workflow-ephemeral";
        name = "ephemeral";
      }
    ];
    ownerFiles = [
      "nixfied/project/conf.nix"
      "nixfied/project/runtime.nix"
      "nixfied/framework/runtime/ephemeral.nix"
    ];
    modelPaths = [ "runtime.ephemeral.includeUntracked" ];
    status = "stable";
    defaults = {
      inherit (runtime.ephemeral) includeUntracked;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.ephemeral.env-file-loading" = {
    kind = "runtime";
    summary = "Ephemeral host env file loading policy";
    surfaces = [
      {
        kind = "workflow-ephemeral";
        name = "ephemeral";
      }
    ];
    ownerFiles = [
      "nixfied/project/conf.nix"
      "nixfied/project/runtime.nix"
      "nixfied/framework/runtime/ephemeral.nix"
    ];
    modelPaths = [
      "runtime.ephemeral.envFileMode"
      "runtime.ephemeral.envFilePath"
    ];
    status = "stable";
    defaults = {
      inherit (runtime.ephemeral) envFileMode;
      inherit (runtime.ephemeral) envFilePath;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.registry.isolation" = {
    kind = "runtime";
    summary = "Run-scoped registry isolation";
    surfaces = [
      {
        kind = "runtime";
        name = "nixfied-runtime";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/orchestrator.nix"
      "nixfied/framework/runtime/env-sandbox.nix"
    ];
    modelPaths = [ "state.policy.registryRoot" ];
    status = "stable";
    defaults = {
      registryRoot = null;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.service-operations" = {
    kind = "runtime";
    summary = "Compiler-published service operation ABI and runtime service dispatch";
    surfaces = [
      {
        kind = "app";
        name = "svc::<service>::<op>";
      }
    ];
    ownerFiles = [
      "nixfied/compiler/compile-service-surface-catalog.nix"
      "nixfied/framework/runtime/engine.nix"
    ];
    modelPaths = [ "services" ];
    status = "stable";
    defaults = {
      appPrefix = "svc::";
    };
    coverageRequired = true;
    coverageLayer = "adapter";
    docs = [ ];
  };

  "runtime.workflow-service-phases" = {
    kind = "runtime";
    summary = "Workflow preRun/postRun service phases over compiled service-set policy";
    surfaces = [
      {
        kind = "workflow-phase";
        name = "preRun.serviceSets/postRun.serviceSets";
      }
    ];
    ownerFiles = [
      "nixfied/modules/service-sets.nix"
      "nixfied/framework/core/materializeExecution.nix"
      "nixfied/compiler/compile-service-sets.nix"
      "nixfied/compiler/compile-workflows.nix"
    ];
    modelPaths = [ "serviceSets" ];
    status = "stable";
    defaults = {
      operations = [
        "start"
        "stop"
        "status"
        "health"
        "ready"
      ];
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.machine-output-behavior" = {
    kind = "runtime";
    summary = "Machine-output contract surfaces for task/workflow launcher execution";
    surfaces = [
      {
        kind = "app";
        name = "machine-output:<app-id>";
      }
    ];
    ownerFiles = [
      "nixfied/modules/machine-outputs.nix"
      "nixfied/framework/core/mkMachineOutputPrograms.nix"
      "nixfied/framework/runtime/kernel/src/machine_output.rs"
    ];
    modelPaths = [ "machineOutputs" ];
    status = "stable";
    defaults = {
      contractBundle = "nixfied-machine-output-contract-bundle.json";
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.summary-sidecars" = {
    kind = "runtime";
    summary = "Workflow summary sidecar generation and machine summary-file mirroring";
    surfaces = [
      {
        kind = "workflow-output";
        name = "--summary/--summary-file";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/executor.nix"
      "nixfied/framework/runtime/orchestrator-runtime.nix"
      "nixfied/framework/runtime/kernel/src/summary.rs"
    ];
    modelPaths = [ "workflows.*.artifacts.writeSummary" ];
    status = "stable";
    defaults = {
      summaryFileName = "summary.json";
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.task-hooks" = {
    kind = "runtime";
    summary = "Task pre/post hook execution semantics";
    surfaces = [
      {
        kind = "task-runtime";
        name = "runtime.preHooks/runtime.postHooks";
      }
    ];
    ownerFiles = [
      "nixfied/modules/tasks.nix"
      "nixfied/framework/runtime/executor.nix"
    ];
    modelPaths = [ "tasks.*.runtime.preHooks" "tasks.*.runtime.postHooks" ];
    status = "stable";
    defaults = {
      hookTimeoutSec = null;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.stop-run-semantics" = {
    kind = "runtime";
    summary = "Run cancellation surfaces for stop-run and stop-all-runs";
    surfaces = [
      {
        kind = "app";
        name = "stop-run/stop-all-runs";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/orchestrator-runtime.nix"
      "nixfied/framework/runtime/kernel/src/run_record.rs"
    ];
    modelPaths = [ "runtime.orchestrator.stopTimeoutSec" ];
    status = "stable";
    defaults = {
      inherit (runtime.orchestrator) stopTimeoutSec;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.process-group-cleanup" = {
    kind = "runtime";
    summary = "Process-group cancellation and descendant cleanup guarantees";
    surfaces = [
      {
        kind = "runtime";
        name = "run cancellation lifecycle";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/orchestrator-runtime.nix"
      "nixfied/framework/runtime/executor.nix"
    ];
    modelPaths = [ "runtime.orchestrator.stopTimeoutSec" ];
    status = "stable";
    defaults = {
      killSignal = "TERM";
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.install-semantics" = {
    kind = "runtime";
    summary = "framework::install wrapper generation and argument semantics";
    surfaces = [
      {
        kind = "app";
        name = "framework::install";
      }
    ];
    ownerFiles = [
      "nixfied/framework/install/wrapper-command.nix"
      "nixfied/framework/install/wrapper-flake.nix"
      "nixfied/framework/install/internal/install-runtime.nix"
    ];
    modelPaths = [ ];
    status = "stable";
    defaults = {
      vendorDefault = false;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.upgrade-semantics" = {
    kind = "runtime";
    summary = "framework::upgrade preservation and refresh semantics";
    surfaces = [
      {
        kind = "app";
        name = "framework::upgrade";
      }
    ];
    ownerFiles = [
      "nixfied/framework/install/wrapper-command.nix"
      "nixfied/framework/install/internal/vendored-metadata.nix"
    ];
    modelPaths = [ ];
    status = "stable";
    defaults = {
      preserveProject = true;
      preserveLocal = true;
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.workflow-interruption-semantics" = {
    kind = "runtime";
    summary = "Workflow interruption status/event semantics";
    surfaces = [
      {
        kind = "workflow-runtime";
        name = "canceled workflow outcome";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/executor.nix"
      "nixfied/framework/runtime/kernel/src/workflow.rs"
      "nixfied/framework/runtime/kernel/src/run_record.rs"
    ];
    modelPaths = [ "workflows.*.execution.failFast" ];
    status = "stable";
    defaults = {
      canceledState = "canceled";
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.artifact-placement-semantics" = {
    kind = "runtime";
    summary = "Per-run and per-attempt artifact placement invariants";
    surfaces = [
      {
        kind = "workflow-output";
        name = "CI_ARTIFACTS_ROOT/CI_ARTIFACTS_DIR placement";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/executor.nix"
      "nixfied/framework/runtime/env-sandbox.nix"
    ];
    modelPaths = [ "workflows.*.artifacts.root" ];
    status = "stable";
    defaults = {
      artifactFile = "summary.json";
    };
    coverageRequired = true;
    coverageLayer = "e2e";
    docs = [ ];
  };

  "runtime.output.prefix-contract" = {
    kind = "runtime";
    summary = "Stable ASCII log prefix contract";
    surfaces = [
      {
        kind = "cli";
        name = "all-user-facing-output";
      }
    ];
    ownerFiles = [
      "nixfied/framework/runtime/helpers/helpers.nix"
      "nixfied/project/tasks.nix"
    ];
    modelPaths = [ ];
    status = "stable";
    defaults = {
      prefixes = [
        "INFO:"
        "WARN:"
        "ERROR:"
        "OK:"
        "SKIP:"
      ];
    };
    coverageRequired = true;
    coverageLayer = "adapter";
    docs = [ ];
  };
}
