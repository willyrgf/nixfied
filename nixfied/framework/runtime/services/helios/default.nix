# Helios module aggregator
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
  config = import ./config.nix {
    inherit
      pkgs
      project
      ;
  };
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
    displayName = "Helios";
    inherit lifecycle;
    extraOperations = {
      # Override start details to mention RPC
      start = {
        script = lifecycle.startLeaf;
        preOps = [
          "init"
          "check-config"
          "preflight-start"
        ];
        summary = "Start Helios node";
        details = "Starts Helios RPC for the current slot/environment.";
      };
      # Override ready with custom details and tunables
      ready = {
        script = lifecycle.ready;
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
  };
in
serviceModule.mkServiceModule {
  service = "helios";
  summaryName = "Helios";
  summary = "Helios service management API";
  details = "Public service contract for managing Helios across dev/prod/test/ci.";
  artifacts = {
    rpcPortVar = slots.portVarName config.portKeyRpc;
    executionPortVar = slots.portVarName config.executionRpcPortKey;
    serviceDir = slots.getServiceDir config.dataDirName;
    dataDir = slots.getServiceDir config.dataDirName;
    logFile = "${slots.getServiceDir config.dataDirName}/logs/helios.log";
    pidFile = "${slots.getServiceDir config.dataDirName}/run/helios.pid";
    network = config.network;
  };
  inherit
    config
    operations
    ;
  exported = {
    inherit (lifecycle)
      helios
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
