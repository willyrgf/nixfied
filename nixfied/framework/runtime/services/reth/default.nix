# Reth module aggregator
{
  pkgs,
  project,
  slots,
}:

let
  summary = import ../../helpers/summary.nix { inherit pkgs project; };
  helpers = import ../../helpers/helpers.nix {
    inherit pkgs project;
    inherit (summary) summaryParser;
  };
  loggingPrelude = helpers.loggingPrelude;
  serviceModule = import ../../helpers/service-module.nix { inherit pkgs project slots; };
  config = import ./config.nix { inherit pkgs project; };
  lifecycle = import ./lifecycle.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };

  operations = import ../service-operations-builder.nix {
    displayName = "Reth";
    inherit lifecycle;
    extraOperations = {
      # Override start details to mention specific ports
      start = {
        script = lifecycle.startLeaf;
        preOps = [
          "init"
          "check-config"
          "preflight-start"
        ];
        summary = "Start Reth node";
        details = "Starts Reth with HTTP, WS, and auth RPC listeners for the current slot/environment.";
      };
      # Override ready details
      ready = {
        script = lifecycle.ready;
        hook = "READY";
        summary = "Wait for Reth readiness";
        details = "Checks that Reth responds on the configured HTTP RPC port.";
      };
    };
  };
in
serviceModule.mkServiceModule {
  service = "reth";
  summaryName = "Reth";
  summary = "Reth service management API";
  details = "Public service contract for managing Reth across dev/prod/test/ci.";
  artifacts = {
    httpPortVar = slots.portVarName config.portKeyHttp;
    wsPortVar = slots.portVarName config.portKeyWs;
    authPortVar = slots.portVarName config.portKeyAuth;
    serviceDir = slots.getServiceDir config.dataDirName;
    dataDir = slots.getServiceDir config.dataDirName;
    logFile = "${slots.getServiceDir config.dataDirName}/logs/reth.log";
    pidFile = "${slots.getServiceDir config.dataDirName}/run/reth.pid";
    network = config.network;
    devMode = config.devMode;
  };
  inherit
    config
    operations
    ;
  exported = {
    inherit (lifecycle)
      reth
      init
      start
      stop
      restart
      status
      health
      checkConfig
      ready
      fullStart
      fullStartTest
      ;
  };
}
