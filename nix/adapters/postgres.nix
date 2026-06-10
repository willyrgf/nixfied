# PostgreSQL reference adapter.
#
# Compiles a minimal Postgres service into the generic model primitives. The
# runtime gains no Postgres knowledge: data-dir init is a prepare exec
# (`initdb`), the server is a foreground start exec (`postgres`), readiness and
# health are TCP ownership probes, the smoke query is a dependent task (`psql`),
# and cleanup is the marker-gated runtime primitive that removes the slot state.
#
# The data directory lives under `${stateDir}/pgdata`, where `${stateDir}` is the
# runtime-materialised slot state root, so M2 marker-gated cleanup removes it.
{ pkgs, ... }:
let
  postgresql = pkgs.postgresql;
  pgdata = "\${stateDir}/pgdata";
in
{
  # One closure per executable in the postgresql package. Each exec resolves to
  # its closure's executable; admission requires that exact pairing.
  nixfied.closures.pg-initdb = {
    package = postgresql;
    executable = "bin/initdb";
    operationBindings = [ "service.postgres.prepare" ];
    effects = [
      "process"
      "file-write"
    ];
  };
  nixfied.closures.pg-server = {
    package = postgresql;
    executable = "bin/postgres";
    operationBindings = [ "service.postgres.start" ];
    effects = [
      "process"
      "network-listener"
      "file-write"
    ];
  };
  nixfied.closures.pg-psql = {
    package = postgresql;
    executable = "bin/psql";
    operationBindings = [ "task.smoke-query.run" ];
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.execs.pg-init = {
    closureId = "pg-initdb";
    timeoutMs = 60000;
  };
  nixfied.execs.pg-server = {
    closureId = "pg-server";
  };
  nixfied.execs.pg-smoke = {
    closureId = "pg-psql";
  };

  nixfied.services.postgres = {
    lifecycle = {
      prepare = {
        operationId = "service.postgres.prepare";
        execId = "pg-init";
        execArgs = [
          "-D"
          pgdata
          "-U"
          "postgres"
          "-A"
          "trust"
          "--no-locale"
          "--encoding=UTF8"
        ];
        terminal = {
          success = "initialized";
          failure = "failed";
        };
      };
      start = {
        operationId = "service.postgres.start";
        execId = "pg-server";
        execArgs = [
          "-D"
          pgdata
          # Connect over TCP only; disabling the Unix socket avoids the macOS
          # sun_path length limit under deep state directories.
          "-c"
          "unix_socket_directories="
          "-h"
          "127.0.0.1"
          "-p"
          "\${port}"
        ];
        terminal = {
          success = "spawned";
          failure = "failed";
        };
      };
      ready = {
        operationId = "service.postgres.ready";
        probe = {
          timeoutMs = 1000;
          retryIntervalMs = 200;
          maxAttempts = 60;
        };
        terminal = {
          success = "ready";
          failure = "not-ready";
        };
      };
      health = {
        operationId = "service.postgres.health";
        probe = {
          timeoutMs = 1000;
          retryIntervalMs = 200;
          maxAttempts = 60;
        };
        terminal = {
          success = "healthy";
          failure = "unhealthy";
        };
      };
      stop = {
        operationId = "service.postgres.stop";
        # Postgres fast shutdown: SIGINT rolls back in-flight transactions and
        # exits promptly, where SIGTERM (smart shutdown) waits for clients.
        signal = "INT";
        terminal = {
          success = "stopped";
          failure = "failed";
        };
      };
      clean = {
        operationId = "service.postgres.clean";
        terminal = {
          success = "cleaned";
          failure = "failed";
        };
      };
    };
    endpoint = {
      endpointId = "postgres-tcp";
    };
    stateRefs = [ "slot" ];
    logRefs = [ "service.postgres" ];
    containment = "process-tree";
  };

  nixfied.tasks.smoke-query = {
    operationId = "task.smoke-query.run";
    execId = "pg-smoke";
    args = [
      "-h"
      "127.0.0.1"
      "-p"
      "\${port}"
      "-U"
      "postgres"
      "-d"
      "postgres"
      "-w"
      "-tAc"
      "SELECT 1"
    ];
    dependsOnServicesReady = [ "postgres" ];
    logRefs = [ "task.smoke-query" ];
    summaryRefs = [ "summary" ];
  };

  nixfied.environments.dev = {
    services = [ "postgres" ];
    tasks = [ "smoke-query" ];
  };
}
