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
      copyMode = runtime.ephemeral.copyMode;
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
      includeUntracked = runtime.ephemeral.includeUntracked;
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
      envFileMode = runtime.ephemeral.envFileMode;
      envFilePath = runtime.ephemeral.envFilePath;
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
