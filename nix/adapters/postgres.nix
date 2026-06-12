# PostgreSQL reference adapter.
#
# Compiles a production-shaped Postgres service into the generic model
# primitives. The runtime gains no Postgres knowledge: data-dir init is an
# idempotent prepare exec (a wrapper around `initdb` that adopts an existing
# cluster), the server is a foreground start exec (`postgres`), readiness and
# health are protocol probes (`pg_isready`), the smoke query is a dependent
# task (`psql`), and cleanup is the marker-gated runtime primitive that removes
# the slot state.
#
# The data directory lives under `${stateDir}/pgdata`, where `${stateDir}` is the
# runtime-materialised slot state root, so M2 marker-gated cleanup removes it.
{
  lib,
  pkgs,
  ...
}:
let
  postgresql = pkgs.postgresql;
  pgdata = "\${stateDir}/pgdata";
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;

  # Idempotent, repeat-run-safe prepare: a second run on the same slot adopts
  # the existing cluster instead of failing in `initdb`; a half-initialized
  # cluster (no PG_VERSION) is rebuilt. On Darwin the cluster must use mmap
  # shared memory — the default SysV segments exhaust the tiny macOS kernel
  # limits when slots run several clusters — so init pins it and adoption
  # verifies it, failing with an actionable message instead of an opaque
  # postmaster startup error.
  pgPrepare = pkgs.writeShellApplication {
    name = "nixfied-pg-prepare";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.gnugrep
      postgresql
    ];
    text = ''
      state_dir=""
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --state-dir)
            state_dir="''${2:?missing --state-dir value}"
            shift 2
            ;;
          *)
            echo "unknown postgres prepare argument: $1" >&2
            exit 64
            ;;
        esac
      done
      if [[ -z "$state_dir" || "$state_dir" == "/" ]]; then
        echo "missing or unsafe --state-dir argument" >&2
        exit 64
      fi

      pgdata="$state_dir/pgdata"
      if [[ -s "$pgdata/PG_VERSION" ]]; then
    ''
    + lib.optionalString isDarwin ''
      if ! grep -Eq '^[[:space:]]*shared_memory_type[[:space:]]*=[[:space:]]*mmap' \
           "$pgdata/postgresql.conf"; then
        echo "existing cluster at $pgdata does not pin shared_memory_type=mmap;" >&2
        echo "re-initialize it (nixfied clean) or set it in postgresql.conf" >&2
        exit 1
      fi
    ''
    + ''
        exit 0
      fi

      rm -rf "$pgdata"
      mkdir -p "$pgdata"
      initdb \
        -D "$pgdata" \
        -U postgres \
        -A trust \
        --no-locale \
        --encoding=UTF8
    ''
    + lib.optionalString isDarwin ''
      {
        echo "shared_memory_type = mmap"
        echo "dynamic_shared_memory_type = mmap"
      } >> "$pgdata/postgresql.conf"
    '';
  };
  pgReadyInvocation = {
    tools = [ "pg-isready" ];
    run = [
      "pg_isready"
      "-h"
      "127.0.0.1"
      "-p"
      "\${port}"
      "-U"
      "postgres"
      "-d"
      "postgres"
      "-t"
      "1"
    ];
  };
in
{
  # One closure per executable. Each exec resolves to its closure's executable;
  # admission requires that exact pairing, and the closure binds exactly the
  # operations it is authorized to run.
  nixfied.closures.pg-prepare = {
    package = pgPrepare;
    executable = "bin/nixfied-pg-prepare";
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
  nixfied.closures.pg-isready = {
    package = postgresql;
    executable = "bin/pg_isready";
    operationBindings = [
      "service.postgres.ready"
      "service.postgres.health"
    ];
    effects = [
      "process"
      "network-listener"
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

  nixfied.services.postgres = {
    lifecycle = {
      prepare = {
        operationId = "service.postgres.prepare";
        invocation = {
          tools = [ "pg-prepare" ];
          run = [
            "nixfied-pg-prepare"
            "--state-dir"
            "\${stateDir}"
          ];
          timeoutMs = 60000;
        };
        terminal = {
          success = "initialized";
          failure = "failed";
        };
      };
      start = {
        operationId = "service.postgres.start";
        invocation = {
          tools = [ "pg-server" ];
          run = [
            "postgres"
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
        };
        terminal = {
          success = "spawned";
          failure = "failed";
        };
      };
      ready = {
        operationId = "service.postgres.ready";
        # Protocol readiness: pg_isready completes a real handshake, so "ready"
        # means the postmaster accepts connections, not merely that the port is
        # bound (which postgres does well before recovery finishes).
        probe = {
          kind = "exec";
          invocation = pgReadyInvocation;
          timeoutMs = 2000;
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
          kind = "exec";
          invocation = pgReadyInvocation;
          timeoutMs = 2000;
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
    invocation = {
      tools = [ "pg-psql" ];
      run = [
        "psql"
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
    };
    requires = [ "postgres" ];
    logRefs = [ "task.smoke-query" ];
    summaryRefs = [ "summary" ];
  };

  nixfied.environments.dev = {
    services = [ "postgres" ];
    tasks = [ "smoke-query" ];
  };
}
