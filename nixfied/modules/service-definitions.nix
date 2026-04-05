{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  rootConfig = config.nixfied;
  probeLib = import ./services/probes.nix { inherit lib; };
  contractSchema = import ./services/contract-schema.nix { inherit lib; };
  sourceOptions = import ./services/source-options.nix { inherit lib; };
  sourceSpec = sourceOptions.mkSourceSpec {
    withClientPackage = true;
  };

  sourceKindType = t.enum [
    "real"
    "shim"
    "mock"
    "unknown"
  ];

  endpointSpec = t.submodule {
    options = {
      protocol = lib.mkOption {
        type = t.str;
        default = "http";
      };
      portKey = lib.mkOption {
        type = t.str;
      };
    };
  };

  mkOperationSpec =
    {
      defaultRuntimeOp,
      defaultSummary,
      defaultDetails,
      defaultExposeApp ? true,
      defaultExposeHook ? false,
    }:
    t.submodule {
      options = {
        runtimeOp = lib.mkOption {
          type = t.nullOr t.str;
          default = defaultRuntimeOp;
        };
        summary = lib.mkOption {
          type = t.str;
          default = defaultSummary;
        };
        details = lib.mkOption {
          type = t.str;
          default = defaultDetails;
        };
        preOps = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        postOps = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        usage = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        examples = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        args = lib.mkOption {
          type = t.listOf contractSchema.kvSpecType;
          default = [ ];
        };
        env = lib.mkOption {
          type = t.listOf contractSchema.kvSpecType;
          default = [ ];
        };
        category = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        class = lib.mkOption {
          type = t.enum [
            "typed"
            "passthrough"
            "json"
            "batch-runner"
          ];
          default = "passthrough";
        };
        idempotent = lib.mkOption {
          type = t.bool;
          default = false;
        };
        exposeApp = lib.mkOption {
          type = t.bool;
          default = defaultExposeApp;
        };
        exposeHook = lib.mkOption {
          type = t.bool;
          default = defaultExposeHook;
        };
        appName = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        hook = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
      };
    };

  mkCheckSpec =
    {
      defaultRuntimeOp,
      defaultSummary,
      defaultDetails,
    }:
    t.submodule {
      options = {
        runtimeOp = lib.mkOption {
          type = t.nullOr t.str;
          default = defaultRuntimeOp;
        };
        summary = lib.mkOption {
          type = t.str;
          default = defaultSummary;
        };
        details = lib.mkOption {
          type = t.str;
          default = defaultDetails;
        };
        preOps = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        postOps = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        usage = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        examples = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        args = lib.mkOption {
          type = t.listOf contractSchema.kvSpecType;
          default = [ ];
        };
        env = lib.mkOption {
          type = t.listOf contractSchema.kvSpecType;
          default = [ ];
        };
        category = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        class = lib.mkOption {
          type = t.enum [
            "typed"
            "passthrough"
            "json"
            "batch-runner"
          ];
          default = "passthrough";
        };
        idempotent = lib.mkOption {
          type = t.bool;
          default = false;
        };
        exposeApp = lib.mkOption {
          type = t.bool;
          default = true;
        };
        exposeHook = lib.mkOption {
          type = t.bool;
          default = false;
        };
        appName = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        hook = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        steps = lib.mkOption {
          type = t.listOf probeLib.probeStepSpec;
          default = [ ];
        };
        wait = lib.mkOption {
          type = probeLib.probeWaitSpec;
          default = { };
        };
      };
    };

  extraOpSpec = t.submodule (
    { name, ... }:
    {
      options = {
        runtimeOp = lib.mkOption {
          type = t.nullOr t.str;
          default = name;
        };
        summary = lib.mkOption {
          type = t.str;
          default = name;
        };
        details = lib.mkOption {
          type = t.str;
          default = "";
        };
        preOps = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        postOps = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        usage = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        examples = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };
        args = lib.mkOption {
          type = t.listOf contractSchema.kvSpecType;
          default = [ ];
        };
        env = lib.mkOption {
          type = t.listOf contractSchema.kvSpecType;
          default = [ ];
        };
        category = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        class = lib.mkOption {
          type = t.enum [
            "typed"
            "passthrough"
            "json"
            "batch-runner"
          ];
          default = "passthrough";
        };
        idempotent = lib.mkOption {
          type = t.bool;
          default = false;
        };
        exposeApp = lib.mkOption {
          type = t.bool;
          default = true;
        };
        exposeHook = lib.mkOption {
          type = t.bool;
          default = false;
        };
        appName = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
        hook = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
      };
    }
  );

  fixtureRefSpec = t.submodule {
    options = {
      description = lib.mkOption {
        type = t.str;
        default = "";
      };
      argumentFields = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };
      defaults = lib.mkOption {
        type = t.attrsOf contractSchema.artifactValueType;
        default = { };
      };
      script = lib.mkOption {
        type = t.lines;
      };
    };
  };

  fixtureInvocationSpec = t.submodule {
    options = {
      description = lib.mkOption {
        type = t.str;
        default = "";
      };
      operation = lib.mkOption {
        type = t.str;
      };
      argumentFields = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };
      defaults = lib.mkOption {
        type = t.attrsOf contractSchema.artifactValueType;
        default = { };
      };
    };
  };

  serviceDefinitionType = t.submodule (
    { name, config, ... }:
    let
      displayName = if (config.displayName or "") == "" then name else config.displayName;
      serviceDir = contractSchema.mkServiceDirExpr config.dataDirName;

      lifecycleOps = {
        "pre-start" = config.lifecycle.preStart;
        start = config.lifecycle.start // {
          preOps = lib.unique ([ "pre-start" ] ++ (config.lifecycle.start.preOps or [ ]));
        };
        status = config.lifecycle.status;
        "pre-stop" = config.lifecycle.preStop;
        stop = config.lifecycle.stop // {
          preOps = lib.unique ([ "pre-stop" ] ++ (config.lifecycle.stop.preOps or [ ]));
        };
      };

      checkOps = {
        health = builtins.removeAttrs config.checks.health [
          "steps"
          "wait"
        ];
        ready = builtins.removeAttrs config.checks.ready [
          "steps"
          "wait"
        ];
      };

      duplicateExtraOps = builtins.filter (opName: builtins.hasAttr opName (lifecycleOps // checkOps)) (
        builtins.attrNames config.extraOps
      );
    in
    {
      freeformType = t.attrsOf t.anything;

      options = {
        enable = lib.mkOption {
          type = t.bool;
          default = false;
        };

        displayName = lib.mkOption {
          type = t.str;
          default = name;
        };

        summary = lib.mkOption {
          type = t.str;
          default = "${displayName} service management API";
        };

        details = lib.mkOption {
          type = t.str;
          default = "Public service contract for managing ${displayName} across dev/prod/test/ci.";
        };

        ownerFile = lib.mkOption {
          type = t.str;
          default = "nixfied.services.${name}";
        };

        profiles = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };

        dataDirName = lib.mkOption {
          type = t.str;
          default = name;
        };

        sources = lib.mkOption {
          type = t.attrsOf sourceSpec;
          default = { };
        };

        sourceKeys = lib.mkOption {
          type = t.listOf t.str;
          default = [ ];
        };

        defaultSource = lib.mkOption {
          type = t.str;
          default = "";
        };

        sourceKinds = lib.mkOption {
          type = t.attrsOf sourceKindType;
          default = { };
        };

        requiredSourceArtifacts = lib.mkOption {
          type = t.listOf (
            t.enum [
              "package"
              "clientPackage"
            ]
          );
          default = [ ];
        };

        checkRuntimeInputs = lib.mkOption {
          type = t.listOf t.package;
          default = [ ];
        };

        endpoints = lib.mkOption {
          type = t.attrsOf endpointSpec;
          default = { };
        };

        artifacts = lib.mkOption {
          type = t.attrsOf contractSchema.artifactValueType;
          default = { };
        };

        lifecycle = {
          preStart = lib.mkOption {
            type = mkOperationSpec {
              defaultRuntimeOp = "pre-start";
              defaultSummary = "Prepare ${displayName} startup";
              defaultDetails = "Runs deterministic setup and validation required before starting ${displayName}.";
              defaultExposeApp = false;
            };
            default = { };
          };
          start = lib.mkOption {
            type = mkOperationSpec {
              defaultRuntimeOp = "start";
              defaultSummary = "Start ${displayName}";
              defaultDetails = "Starts ${displayName} for the current slot/environment.";
            };
            default = { };
          };
          status = lib.mkOption {
            type = mkOperationSpec {
              defaultRuntimeOp = "status";
              defaultSummary = "Show ${displayName} status";
              defaultDetails = "Prints ${displayName} status for the current slot/environment.";
            };
            default = { };
          };
          preStop = lib.mkOption {
            type = mkOperationSpec {
              defaultRuntimeOp = "pre-stop";
              defaultSummary = "Prepare ${displayName} shutdown";
              defaultDetails = "Runs deterministic shutdown preparation for ${displayName}.";
              defaultExposeApp = false;
            };
            default = { };
          };
          stop = lib.mkOption {
            type = mkOperationSpec {
              defaultRuntimeOp = "stop";
              defaultSummary = "Stop ${displayName}";
              defaultDetails = "Stops ${displayName} for the current slot/environment.";
            };
            default = { };
          };
        };

        checks = {
          health = lib.mkOption {
            type = mkCheckSpec {
              defaultRuntimeOp = "health";
              defaultSummary = "Run ${displayName} health check";
              defaultDetails = "Checks ${displayName} health for the current slot/environment.";
            };
            default = { };
          };
          ready = lib.mkOption {
            type = mkCheckSpec {
              defaultRuntimeOp = "ready";
              defaultSummary = "Wait for ${displayName} readiness";
              defaultDetails = "Waits for ${displayName} to be ready for the current slot/environment.";
            };
            default = { };
          };
        };

        extraOps = lib.mkOption {
          type = t.attrsOf extraOpSpec;
          default = { };
        };

        fixture = {
          refs = lib.mkOption {
            type = t.attrsOf fixtureRefSpec;
            default = { };
          };
          exports = lib.mkOption {
            type = t.attrsOf fixtureInvocationSpec;
            default = { };
          };
          bootstrap = lib.mkOption {
            type = t.attrsOf fixtureInvocationSpec;
            default = { };
          };
        };

        implementation = lib.mkOption {
          type = t.submodule {
            options = {
              version = lib.mkOption {
                type = t.int;
                default = 1;
              };
              module = lib.mkOption {
                type = t.nullOr t.path;
                default = null;
              };
            };
          };
          default = { };
        };

        contract = contractSchema.mkContractOption "Derived public service contract.";
      };

      config = {
        sourceKeys = lib.mkDefault (builtins.sort builtins.lessThan (builtins.attrNames config.sources));
        assertions = [
          {
            assertion = duplicateExtraOps == [ ];
            message = "nixfied.services.${name}.extraOps redefines canonical operations: ${builtins.concatStringsSep ", " duplicateExtraOps}";
          }
        ];

        contract = {
          version = 1;
          service = name;
          summary = config.summary;
          details = config.details;
          ownerFile = config.ownerFile;
          profiles = config.profiles;
          artifacts = {
            serviceDir = serviceDir;
            dataDir = serviceDir;
          }
          // config.artifacts;
          runtimePrimitives = contractSchema.mkRuntimePrimitivesV1 rootConfig.runtime;
          operations =
            lifecycleOps
            // checkOps
            // config.extraOps
            // contractSchema.mkObservabilityOperations {
              service = name;
              summaryName = displayName;
            };
        };
      };
    }
  );
in
{
  options.nixfied.services = lib.mkOption {
    type = t.attrsOf serviceDefinitionType;
    default = { };
  };
}
