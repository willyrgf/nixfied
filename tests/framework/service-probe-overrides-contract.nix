{ pkgs }:
let
  lib = pkgs.lib;
  serviceConfigLib = import ../../nixfied/framework/core/service-config.nix { inherit lib; };

  nginxConfig = serviceConfigLib.normalizeServiceConfig {
    name = "nginx";
    config = {
      sourceKeys = [ ];
      sources = { };
      defaultSource = "";
      dataDirName = "nginx";
      portKeyHttp = "http";
      portKeyHttps = "https";
      probes = {
        health = {
          strategy = "prepend";
          steps = [
            {
              kind = "exec";
              label = "nginx custom health";
              command = "true";
            }
          ];
        };
        ready = {
          strategy = "append";
          steps = [
            {
              kind = "exec";
              label = "nginx custom ready";
              command = "true";
            }
          ];
          wait = {
            enabled = true;
            timeoutSeconds = 9;
            intervalSeconds = 2;
          };
        };
      };
    };
  };

  postgresConfig = serviceConfigLib.normalizeServiceConfig {
    name = "postgres";
    config = {
      sourceKeys = [ ];
      sources = { };
      defaultSource = "";
      dataDirName = "postgres";
      portKey = "postgres";
      database = "app";
      testDatabase = "app_test";
      migrations = {
        dir = null;
        command = null;
        sourceDatabase = null;
      };
      probes.ready = {
        strategy = "replace";
        steps = [
          {
            kind = "exec";
            label = "postgres custom ready";
            command = "true";
          }
        ];
        wait = {
          enabled = true;
          timeoutSeconds = 12;
          intervalSeconds = 3;
        };
      };
    };
  };

  heliosConfig = serviceConfigLib.normalizeServiceConfig {
    name = "helios";
    config = {
      sourceKeys = [ "real" ];
      sources.real = { };
      sourceKinds.real = "real";
      defaultSource = "real";
      dataDirName = "helios";
      portKeyRpc = "heliosRpc";
      executionRpcPortKey = "rethHttp";
      network = "mainnet";
      readiness = {
        profile = "strict";
        requireNotSyncing = false;
        disallowSourceKinds = [ ];
      };
    };
  };

  heliosReadyStep = builtins.elemAt heliosConfig.resolved.probePlans.ready.steps 0;
in
assert
  serviceConfigLib.supportedServiceNames == [
    "postgres"
    "nginx"
    "minio"
    "reth"
    "helios"
  ];
assert nginxConfig.resolved.probes.health == "exec";
assert nginxConfig.resolved.probePlans.health.count == 3;
assert (builtins.elemAt nginxConfig.resolved.probePlans.health.steps 0).kind == "exec";
assert (builtins.elemAt nginxConfig.resolved.probePlans.health.steps 1).kind == "tcp";
assert nginxConfig.resolved.probes.ready == "exec";
assert nginxConfig.resolved.probePlans.ready.count == 3;
assert (builtins.elemAt nginxConfig.resolved.probePlans.ready.steps 2).kind == "exec";
assert nginxConfig.resolved.probePlans.ready.wait.enabled == true;
assert nginxConfig.resolved.probePlans.ready.wait.timeoutSeconds == 9;
assert nginxConfig.resolved.probePlans.ready.wait.intervalSeconds == 2;
assert postgresConfig.resolved.probes.ready == "exec";
assert postgresConfig.resolved.probePlans.ready.count == 1;
assert (builtins.elemAt postgresConfig.resolved.probePlans.ready.steps 0).kind == "exec";
assert postgresConfig.resolved.probePlans.ready.wait.enabled == true;
assert postgresConfig.resolved.probePlans.ready.wait.timeoutSeconds == 12;
assert postgresConfig.resolved.probePlans.ready.wait.intervalSeconds == 3;
assert heliosConfig.resolved.probePlans.ready.wait.enabled == true;
assert heliosConfig.resolved.probePlans.ready.wait.timeoutEnvVar == "HELIOS_READY_TIMEOUT_SECS";
assert heliosConfig.resolved.probePlans.ready.wait.intervalEnvVar == "HELIOS_READY_INTERVAL_SECS";
assert heliosReadyStep.kind == "helios-ready";
assert heliosReadyStep.requireNotSyncing == true;
assert builtins.elem "shim" heliosReadyStep.disallowSourceKinds;
assert builtins.elem "unknown" heliosReadyStep.disallowSourceKinds;
pkgs.runCommand "service-probe-overrides-contract" { } ''
  echo "OK: service probe overrides normalize and merge into canonical probe plans" > "$out"
''
