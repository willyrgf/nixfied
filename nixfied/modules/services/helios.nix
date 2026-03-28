{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  cfg = config.nixfied.services.helios;
  probeLib = import ./probes.nix { inherit lib; };
  contractSchema = import ./contract-schema.nix { inherit lib; };
  operationContractBuilder = import ./operation-contract-builder.nix;
  sourceOptions = import ./source-options.nix { inherit lib; };
  sourceSpec = sourceOptions.mkSourceSpec { };
  sourceKindType = t.enum [
    "real"
    "shim"
    "mock"
  ];
  disallowedKindType = t.enum [
    "real"
    "shim"
    "mock"
    "unknown"
  ];
  serviceDir = contractSchema.mkServiceDirExpr cfg.dataDirName;
in
{
  options.nixfied.services.helios = {
    enable = lib.mkOption {
      type = t.bool;
      default = false;
    };
    portKeyRpc = lib.mkOption {
      type = t.str;
      default = "heliosRpc";
    };
    executionRpcPortKey = lib.mkOption {
      type = t.str;
      default = "rethHttp";
    };
    dataDirName = lib.mkOption {
      type = t.str;
      default = "helios";
    };
    network = lib.mkOption {
      type = t.str;
      default = "local";
    };
    executionRpcUrl = lib.mkOption {
      type = t.str;
      default = "";
    };
    consensusRpcUrl = lib.mkOption {
      type = t.str;
      default = "";
    };
    defaultConsensusRpcUrl = lib.mkOption {
      type = t.str;
      default = "https://www.lightclientdata.org";
    };
    checkpoint = lib.mkOption {
      type = t.str;
      default = "";
    };
    extraArgs = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
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

    readiness = {
      profile = lib.mkOption {
        type = t.enum [
          "fast"
          "strict"
        ];
        default = "fast";
      };

      requireNotSyncing = lib.mkOption {
        type = t.bool;
        default = false;
      };

      disallowSourceKinds = lib.mkOption {
        type = t.listOf disallowedKindType;
        default = [ ];
      };
    };

    probes = probeLib.probeOptions;
    contract = contractSchema.mkContractOption "Typed Helios public contract.";
  };

  config.nixfied.services.helios.contract = {
    version = 1;
    service = "helios";
    summary = "Helios service management API";
    details = "Public service contract for managing Helios across dev/prod/test/ci.";
    ownerFile = "nixfied/modules/services/helios.nix";
    adapter = {
      version = 1;
      module = ../../framework/runtime/services/helios/default.nix;
    };
    artifacts = {
      rpcPortVar = contractSchema.mkPortVarName cfg.portKeyRpc;
      executionPortVar = contractSchema.mkPortVarName cfg.executionRpcPortKey;
      serviceDir = serviceDir;
      dataDir = serviceDir;
      logFile = "${serviceDir}/logs/helios.log";
      pidFile = "${serviceDir}/run/helios.pid";
      network = cfg.network;
    };
    runtimePrimitives = contractSchema.mkRuntimePrimitivesV1 config.nixfied.runtime;
    operations =
      (operationContractBuilder {
        displayName = "Helios";
        extraOperations = {
          start = {
            runtimeOp = "start-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Start Helios node";
            details = "Starts Helios RPC for the current slot/environment.";
          };

          ready = {
            runtimeOp = "ready";
            hook = "READY";
            summary = "Wait for Helios readiness";
            details = ''
              Waits until Helios can answer `eth_blockNumber` successfully.

              Note: framework fixtures intentionally skip Helios start/readiness checks
              for `network=local` when a beacon consensus endpoint is unavailable.

              Tunables:
              - `HELIOS_READY_TIMEOUT_SECS` (default: 300)
              - `HELIOS_READY_INTERVAL_SECS` (default: 1)
            '';
          };
        };
      })
      // contractSchema.mkObservabilityOperations {
        service = "helios";
        summaryName = "Helios";
      };
  };
}
