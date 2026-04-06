{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  cfg = config.nixfied.services.reth;
  probeLib = import ./probes.nix { inherit lib; };
  contractSchema = import ./contract-schema.nix { inherit lib; };
  operationContractBuilder = import ./operation-contract-builder.nix;
  sourceOptions = import ./source-options.nix { inherit lib; };
  sourceSpec = sourceOptions.mkSourceSpec { };
  serviceDir = contractSchema.mkServiceDirExpr cfg.dataDirName;
in
{
  options.nixfied.services.reth = {
    enable = lib.mkOption {
      type = t.bool;
      default = false;
    };
    portKeyHttp = lib.mkOption {
      type = t.str;
      default = "rethHttp";
    };
    portKeyWs = lib.mkOption {
      type = t.str;
      default = "rethWs";
    };
    portKeyAuth = lib.mkOption {
      type = t.str;
      default = "rethAuth";
    };
    dataDirName = lib.mkOption {
      type = t.str;
      default = "reth";
    };
    network = lib.mkOption {
      type = t.str;
      default = "local";
    };
    devMode = lib.mkOption {
      type = t.bool;
      default = false;
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
    probes = probeLib.probeOptions;
    contract = contractSchema.mkContractOption "Typed Reth public contract.";
    implementation = contractSchema.mkImplementationOption "Private Reth runtime implementation.";
  };

  config.nixfied.services.reth.contract = {
    version = 1;
    service = "reth";
    summary = "Reth service management API";
    details = "Public service contract for managing Reth across dev/prod/test/ci.";
    ownerFile = "nixfied/modules/services/reth.nix";
    artifacts = {
      httpPortVar = contractSchema.mkPortVarName cfg.portKeyHttp;
      wsPortVar = contractSchema.mkPortVarName cfg.portKeyWs;
      authPortVar = contractSchema.mkPortVarName cfg.portKeyAuth;
      serviceDir = serviceDir;
      dataDir = serviceDir;
      logFile = "${serviceDir}/logs/reth.log";
      pidFile = "${serviceDir}/run/reth.pid";
      network = cfg.network;
      devMode = cfg.devMode;
    };
    runtimePrimitives = contractSchema.mkRuntimePrimitivesV1 config.nixfied.runtime;
    operations =
      (operationContractBuilder {
        displayName = "Reth";
        extraOperations = {
          start = {
            runtimeOp = "start-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Start Reth node";
            details = "Starts Reth with HTTP, WS, and auth RPC listeners for the current slot/environment.";
          };

          ready = {
            runtimeOp = "ready";
            summary = "Wait for Reth readiness";
            details = "Checks that Reth responds on the configured HTTP RPC port.";
          };
        };
      })
      // contractSchema.mkObservabilityOperations {
        service = "reth";
        summaryName = "Reth";
      };
  };

  config.nixfied.services.reth.implementation = {
    version = 1;
    module = ./runtime/reth/default.nix;
  };
}
