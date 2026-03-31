{ lib }:
let
  t = lib.types;
  tokenLib = import ../../framework/core/normalize-token.nix { inherit lib; };
  normalizeToken = tokenLib.normalizeToken;

  kvSpecType = t.submodule {
    options = {
      name = lib.mkOption {
        type = t.str;
      };

      description = lib.mkOption {
        type = t.str;
        default = "";
      };
    };
  };

  runtimePrimitiveSpecType = t.submodule {
    options = {
      env = lib.mkOption {
        type = t.str;
      };

      aliases = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      values = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      default = lib.mkOption {
        type = t.str;
      };
    };
  };

  runtimePrimitivesType = t.submodule {
    options = {
      version = lib.mkOption {
        type = t.int;
      };

      logLevel = lib.mkOption {
        type = runtimePrimitiveSpecType;
      };

      outputMode = lib.mkOption {
        type = runtimePrimitiveSpecType;
      };
    };
  };

  operationType = t.submodule (
    { name, ... }:
    {
      options = {
        runtimeOp = lib.mkOption {
          type = t.nullOr t.str;
          default = name;
        };

        summary = lib.mkOption {
          type = t.str;
        };

        details = lib.mkOption {
          type = t.str;
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
          type = t.listOf kvSpecType;
          default = [ ];
        };

        env = lib.mkOption {
          type = t.listOf kvSpecType;
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
          default = true;
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

  artifactValueType = t.oneOf [
    t.str
    t.bool
    t.int
  ];

  serviceImplementationType = t.submodule {
    options = {
      version = lib.mkOption {
        type = t.int;
      };

      module = lib.mkOption {
        type = t.path;
      };
    };
  };

  serviceContractType = t.submodule {
    options = {
      version = lib.mkOption {
        type = t.int;
      };

      service = lib.mkOption {
        type = t.str;
      };

      summary = lib.mkOption {
        type = t.str;
      };

      details = lib.mkOption {
        type = t.str;
      };

      ownerFile = lib.mkOption {
        type = t.str;
      };

      profiles = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      artifacts = lib.mkOption {
        type = t.attrsOf artifactValueType;
        default = { };
      };

      runtimePrimitives = lib.mkOption {
        type = runtimePrimitivesType;
      };

      operations = lib.mkOption {
        type = t.attrsOf operationType;
        default = { };
      };
    };
  };
in
{
  serviceContractType = serviceContractType;
  serviceImplementationType = serviceImplementationType;

  mkContractOption =
    description:
    lib.mkOption {
      type = serviceContractType;
      readOnly = true;
      visible = false;
      inherit description;
    };

  mkImplementationOption =
    description:
    lib.mkOption {
      type = serviceImplementationType;
      readOnly = true;
      visible = false;
      inherit description;
    };

  mkRuntimePrimitivesV1 = runtime: {
    version = 1;
    logLevel = {
      env = "LOG_LEVEL";
      aliases = [ "NIXFIED_LOG_LEVEL" ];
      values = [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      default = runtime.logging.levelDefault;
    };
    outputMode = {
      env = "OUTPUT_MODE";
      aliases = [ "NIXFIED_OUTPUT_MODE" ];
      values = [
        "stdout"
        "logs"
        "both"
      ];
      default = runtime.logging.outputDefault;
    };
  };

  mkObservabilityOperations =
    {
      service,
      summaryName ? service,
    }:
    {
      log = {
        runtimeOp = "log";
        hook = "LOG";
        summary = "Show ${summaryName} log";
        details = "Shows ${summaryName} runtime log for the current slot/environment.";
        usage = [ "nix run .#svc::${service}::log -- [--lines N] [--follow]" ];
      };

      events = {
        runtimeOp = "events";
        hook = "EVENTS";
        summary = "Show ${summaryName} lifecycle events";
        details = "Shows ${summaryName} lifecycle events from the global process registry for the current slot/environment.";
        usage = [ "nix run .#svc::${service}::events -- [--limit N]" ];
      };
    };

  mkPortVarName = portKey: "${normalizeToken portKey}_PORT";

  mkServiceDirExpr = dataDirName: "\${NIXFIED_SERVICE_ROOT}/${dataDirName}";
}
