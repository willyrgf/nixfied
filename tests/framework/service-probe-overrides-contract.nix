{ pkgs }:
let
  inherit (pkgs) lib;
  serviceConfigLib = import ../../nixfied/framework/core/service-config.nix { inherit lib; };

  nginxConfig = serviceConfigLib.normalizeServiceConfig {
    name = "nginx";
    config = {
      sources = { };
      defaultSource = "";
      dataDirName = "nginx";
      portKeyHttp = "http";
      portKeyHttps = "https";
      checks = {
        health.steps = [
          {
            kind = "exec";
            label = "nginx custom health";
            command = "true";
          }
        ];
        ready = {
          wait = {
            enabled = true;
            timeoutSeconds = 9;
            intervalSeconds = 2;
          };
          steps = [
            {
              kind = "exec";
              label = "nginx custom ready";
              command = "true";
            }
          ];
        };
      };
    };
  };

  postgresConfig = serviceConfigLib.normalizeServiceConfig {
    name = "postgres";
    config = {
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
      checks.ready = {
        wait = {
          enabled = true;
          timeoutSeconds = 12;
          intervalSeconds = 3;
        };
        steps = [
          {
            kind = "exec";
            label = "postgres custom ready";
            command = "true";
          }
        ];
      };
    };
  };

  heliosConfig = serviceConfigLib.normalizeServiceConfig {
    name = "helios";
    config = {
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
      checks.ready = {
        wait = {
          enabled = true;
          timeoutEnvVar = "HELIOS_READY_TIMEOUT_SECS";
          intervalEnvVar = "HELIOS_READY_INTERVAL_SECS";
        };
        steps = [
          {
            kind = "exec";
            command = "true";
          }
          {
            kind = "jsonrpc";
            endpoint = "execution";
            method = "eth_chainId";
          }
        ];
      };
    };
  };
in
assert nginxConfig.resolved.checks.health == "exec";
assert nginxConfig.resolved.probePlans.health.count == 1;
assert (builtins.elemAt nginxConfig.resolved.probePlans.health.steps 0).kind == "exec";
assert nginxConfig.resolved.checks.ready == "exec";
assert nginxConfig.resolved.probePlans.ready.count == 1;
assert (builtins.elemAt nginxConfig.resolved.probePlans.ready.steps 0).kind == "exec";
assert nginxConfig.resolved.probePlans.ready.wait.enabled;
assert nginxConfig.resolved.probePlans.ready.wait.timeoutSeconds == 9;
assert nginxConfig.resolved.probePlans.ready.wait.intervalSeconds == 2;
assert postgresConfig.resolved.checks.ready == "exec";
assert postgresConfig.resolved.probePlans.ready.count == 1;
assert (builtins.elemAt postgresConfig.resolved.probePlans.ready.steps 0).kind == "exec";
assert postgresConfig.resolved.probePlans.ready.wait.enabled;
assert postgresConfig.resolved.probePlans.ready.wait.timeoutSeconds == 12;
assert postgresConfig.resolved.probePlans.ready.wait.intervalSeconds == 3;
assert heliosConfig.resolved.checks.ready == "composite";
assert heliosConfig.resolved.probePlans.ready.count == 2;
assert heliosConfig.resolved.probePlans.ready.wait.enabled;
assert heliosConfig.resolved.probePlans.ready.wait.timeoutEnvVar == "HELIOS_READY_TIMEOUT_SECS";
assert heliosConfig.resolved.probePlans.ready.wait.intervalEnvVar == "HELIOS_READY_INTERVAL_SECS";
assert (builtins.elemAt heliosConfig.resolved.probePlans.ready.steps 0).kind == "exec";
assert (builtins.elemAt heliosConfig.resolved.probePlans.ready.steps 1).kind == "jsonrpc";
pkgs.runCommand "service-probe-overrides-contract" { } ''
  echo "OK: generic service checks normalize into canonical probe plans" > "$out"
''
