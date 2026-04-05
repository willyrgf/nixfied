# PostgreSQL lifecycle management - init, start, stop, setupDb, fullStart
{
  pkgs,
  project,
  slots,
  config,
  loggingPrelude,
}:

let
  probeSetup = import ../shared/probe-setup-helper.nix {
    inherit
      pkgs
      project
      slots
      config
      ;
    serviceName = "postgres";
    endpointMapping = {
      primary = "$PGPORT";
    };
  };

  inherit (probeSetup)
    runtimeDefaults
    managedServiceLifecycle
    slotEnvRuntime
    observability
    serviceSource
    readyPlan
    renderProbeStep
    healthPlanBody
    readyPlanBody
    ;

  postgres = config.package or pkgs.postgresql_16;
  portKey = config.portKey or "postgres";
  portVar = slots.portVarName portKey;
  dataDirName = config.dataDirName or "postgres";
  pgdataExpr = slots.getServiceDir dataDirName;
  database = config.database or "app";
  testDatabase = config.testDatabase or "${database}_test";
  extensions = config.extensions or [ ];
  renderQuietProbeStep = mode: step: ''
    {
      service_source=${pkgs.lib.escapeShellArg serviceSource}
      ${renderProbeStep mode step}
    } >/dev/null 2>&1
  '';
  startupPgIsReadyCommand = renderQuietProbeStep "health" {
    kind = "postgres-pg-isready";
    endpoint = "primary";
    serviceLabel = "postgres";
    phaseLabel = "health";
    successLabel = "healthy";
    failureLabel = "unhealthy";
    host = runtimeDefaults.hosts.localhost;
    failureSuffix = "";
  };
  readyTestPgIsReadyCommand = renderQuietProbeStep "ready" {
    kind = "postgres-pg-isready";
    endpoint = "primary";
    serviceLabel = "postgres";
    phaseLabel = "readiness";
    successLabel = "ready";
    failureLabel = "not ready";
    host = runtimeDefaults.hosts.localhost;
    failureSuffix = " (pg_isready failed)";
  };
  readyTestMaintenanceQueryCommand = renderQuietProbeStep "ready" {
    kind = "postgres-query";
    endpoint = "primary";
    serviceLabel = "postgres";
    phaseLabel = "readiness";
    successLabel = "ready";
    failureLabel = "not ready";
    host = runtimeDefaults.hosts.localhost;
    database = "postgres";
    query = "select 1;";
    failureSuffix = " (maintenance query failed)";
  };
  readyTestDatabaseQueryCommand = renderQuietProbeStep "ready" {
    kind = "postgres-query";
    endpoint = "primary";
    serviceLabel = "postgres";
    phaseLabel = "readiness";
    successLabel = "ready";
    failureLabel = "not ready";
    host = runtimeDefaults.hosts.localhost;
    database = testDatabase;
    query = "select 1;";
    failureSuffix = " (database query failed)";
  };
  mkWrappedScript =
    {
      name,
      runtimePrelude ? "",
      body,
    }:
    managedServiceLifecycle.mkWrappedScript {
      inherit
        name
        loggingPrelude
        runtimePrelude
        body
        ;
    };
  mkPgScript =
    {
      name,
      defaultDb ? database,
      body,
    }:
    mkWrappedScript {
      inherit name body;
      runtimePrelude = pgRuntimePrelude defaultDb;
    };
  pgRuntimePrelude = defaultDb: ''
    ${slotEnvRuntime.loadJsonFromCommand {
      outVar = "SLOT_INFO_JSON_OUT";
      command = toString slots.getSlotInfo;
      exportVars = false;
    }}
    ${slotEnvRuntime.readJsonField {
      targetVar = "RUN_DIR";
      jsonVar = "SLOT_INFO_JSON_OUT";
      fieldExpr = ".directories.run";
    }}

    PORT_VAR="${portVar}"
    ${slotEnvRuntime.readPortFromJson {
      targetVar = "_PGPORT_RESOLVED";
      jsonVar = "SLOT_INFO_JSON_OUT";
      keyExpr = "$PORT_VAR";
    }}
    export PGPORT="''${PGPORT:-$_PGPORT_RESOLVED}"
    export PGDATA="''${PGDATA:-${pgdataExpr}}"
    # Keep the unix socket path short. In CI (and on some systems with long TMPDIR paths),
    # putting sockets under $PGDATA can exceed the 107-byte sockaddr_un.sun_path limit and
    # prevent PostgreSQL from starting. Scope the directory by uid as well so separate Nix
    # sandbox users do not collide on an existing /tmp/nixfied-pg-* directory they cannot
    # write.
    SOCKET_HASH=$(printf '%s' "''${RUN_DIR:-$PGDATA}" | ${pkgs.coreutils}/bin/cksum | ${pkgs.coreutils}/bin/cut -d ' ' -f1)
    SOCKET_UID="$(${pkgs.coreutils}/bin/id -u)"
    export PGSOCKET_DIR="''${PGSOCKET_DIR:-/tmp/nixfied-pg-$SOCKET_UID-$SOCKET_HASH}"
    export PGDATABASE="''${PGDATABASE:-${defaultDb}}"

    if [ -z "''${PGPORT:-}" ] || [ -z "''${PGDATA:-}" ]; then
      log_error "Failed to resolve PostgreSQL runtime variables (PGPORT/PGDATA)"
      exit 1
    fi

    case "$PGPORT" in
      *[!0-9]*)
        log_error "PGPORT must be numeric (got '$PGPORT')"
        exit 1
        ;;
    esac

    ${observability.mkEmitServiceEventFunction "postgres"}
  '';

  ensureConfigPort = ''
    ensure_config_port() {
      local conf="$1"

      if [ ! -f "$conf" ]; then
        log_error "missing postgresql.conf path=$conf"
        exit 1
      fi

      if ${pkgs.gnugrep}/bin/grep -Eq '^[[:space:]]*port[[:space:]]*=' "$conf"; then
        ${pkgs.gnused}/bin/sed -i -E "s|^[[:space:]]*port[[:space:]]*=.*$|port = $PGPORT|g" "$conf"
      else
        printf '\nport = %s\n' "$PGPORT" >> "$conf"
      fi
    }
  '';

  selectConfigTemplate = ''
    select_config_template() {
      case "''${1:-dev}" in
        prod)
          printf '%s\n' '${config.prodConfFile}'
          ;;
        test)
          printf '%s\n' '${config.testConfFile}'
          ;;
        *)
          printf '%s\n' '${config.devConfFile}'
          ;;
      esac
    }
  '';

  preflightInit = mkPgScript {
    name = "postgres-preflight-init";
    body = ''
      ${selectConfigTemplate}

      CONF_ENV="''${ENV:-dev}"
      PGCONF_TEMPLATE="$(select_config_template "$CONF_ENV")"

      if [ ! -f "$PGCONF_TEMPLATE" ]; then
        nixfied_exit_precondition "missing PostgreSQL config template path=$PGCONF_TEMPLATE env=$CONF_ENV"
      fi

      if [ ! -f "${config.pgHbaConfFile}" ]; then
        nixfied_exit_precondition "missing PostgreSQL pg_hba.conf template path=${config.pgHbaConfFile}"
      fi
    '';
  };

  initLeaf = mkPgScript {
    name = "postgres-init-leaf";
    body = ''
      ${ensureConfigPort}
      ${selectConfigTemplate}

      mkdir -p "$PGDATA"

      if [ -f "$PGDATA/PG_VERSION" ]; then
        log_ok "PostgreSQL already initialized at $PGDATA"
        exit 0
      fi

      log_info "Initializing PostgreSQL at $PGDATA"
      ${postgres}/bin/initdb -D "$PGDATA" -U postgres --no-locale --encoding=UTF8 -A trust

      # Determine environment-specific config
      CONF_ENV="''${ENV:-dev}"
      PGCONF_TEMPLATE="$(select_config_template "$CONF_ENV")"
      ${pkgs.coreutils}/bin/install -m 600 "$PGCONF_TEMPLATE" "$PGDATA/postgresql.conf"

      ensure_config_port "$PGDATA/postgresql.conf"
      if ! ${postgres}/bin/postgres -D "$PGDATA" -C port >/dev/null 2>&1; then
        log_error "PostgreSQL configuration invalid after init pgdata=$PGDATA"
        exit 1
      fi

      ${pkgs.coreutils}/bin/install -m 600 ${config.pgHbaConfFile} "$PGDATA/pg_hba.conf"
    '';
  };

  init = mkPgScript {
    name = "postgres-init";
    body = ''
      ${preflightInit}
      exec ${initLeaf}
    '';
  };

  preflightStart = mkPgScript {
    name = "postgres-preflight-start";
    body = ''
      ${ensureConfigPort}

      if [ ! -f "$PGDATA/postgresql.conf" ]; then
        nixfied_exit_precondition "PostgreSQL not initialized at $PGDATA (missing postgresql.conf)"
      fi

      ensure_config_port "$PGDATA/postgresql.conf"

      if [ "$(${pkgs.coreutils}/bin/uname -s)" = "Darwin" ]; then
        SHARED_MEMORY_TYPE="$(${postgres}/bin/postgres -D "$PGDATA" -C shared_memory_type 2>/dev/null || true)"
        DYNAMIC_SHARED_MEMORY_TYPE="$(${postgres}/bin/postgres -D "$PGDATA" -C dynamic_shared_memory_type 2>/dev/null || true)"

        if [ "$SHARED_MEMORY_TYPE" != "mmap" ]; then
          nixfied_exit_precondition "Darwin PostgreSQL start requires shared_memory_type=mmap (effective '$SHARED_MEMORY_TYPE')"
        fi

        if [ "$DYNAMIC_SHARED_MEMORY_TYPE" != "mmap" ]; then
          nixfied_exit_precondition "Darwin PostgreSQL start requires dynamic_shared_memory_type=mmap (effective '$DYNAMIC_SHARED_MEMORY_TYPE')"
        fi
      fi
    '';
  };

  startLeaf = mkPgScript {
    name = "postgres-start-leaf";
    body = ''
      ${ensureConfigPort}

      if [ ! -f "$PGDATA/postgresql.conf" ]; then
        log_error "PostgreSQL not initialized at $PGDATA (missing postgresql.conf)"
        echo "   Run postgres init first: nix run .#svc::postgres::init" >&2
        exit 1
      fi
      ensure_config_port "$PGDATA/postgresql.conf"

      if ${startupPgIsReadyCommand}
      then
        # Verify the running instance is ours by checking PGDATA
        if [ -f "$PGDATA/postmaster.pid" ]; then
          RUN_PID=$(head -1 "$PGDATA/postmaster.pid" 2>/dev/null || true)
          emit_service_event service_ready --pid "$RUN_PID" --log-path "$PGDATA/postgres.log"
          log_ok "PostgreSQL already running on port $PGPORT"
          exit 0
        else
          log_warn "Port $PGPORT in use by a different PostgreSQL instance"
          if [ "''${CI:-}" = "true" ] || [ "''${AUTO_STOP_CONFLICTING:-}" = "1" ]; then
            echo "   Auto-stopping conflicting instance (CI mode)..." >&2
            lsof -ti:$PGPORT 2>/dev/null | xargs kill -TERM 2>/dev/null || true
            sleep 2
          else
            echo "   Use 'svc postgres check-port' to investigate" >&2
            exit 1
          fi
        fi
      fi

      # Clean up stale PID file
      if [ -f "$PGDATA/postmaster.pid" ]; then
        STALE_PID=$(head -1 "$PGDATA/postmaster.pid" 2>/dev/null || true)
        if [ -n "$STALE_PID" ] && ! kill -0 "$STALE_PID" 2>/dev/null; then
          log_info "Removing stale PID file (PID $STALE_PID not running)"
          rm -f "$PGDATA/postmaster.pid"
        fi
      fi

      if [ -z "''${PGSOCKET_DIR:-}" ]; then
        PGSOCKET_DIR="/tmp"
      fi
      mkdir -p "$PGSOCKET_DIR"
      chmod 700 "$PGSOCKET_DIR" 2>/dev/null || true

      log_info "Starting PostgreSQL on port $PGPORT"
      emit_service_event service_starting --log-path "$PGDATA/postgres.log"
      ${postgres}/bin/pg_ctl -D "$PGDATA" -l "$PGDATA/postgres.log" -o "-p $PGPORT -k $PGSOCKET_DIR" start

      for i in $(seq 1 60); do
        if ${startupPgIsReadyCommand}
        then
          RUN_PID=$(head -1 "$PGDATA/postmaster.pid" 2>/dev/null || true)
          emit_service_event service_ready --pid "$RUN_PID" --log-path "$PGDATA/postgres.log"
          log_ok "PostgreSQL ready on port $PGPORT"
          exit 0
        fi
        sleep 0.5
      done

      emit_service_event service_degraded \
        --log-path "$PGDATA/postgres.log" \
        --wait-reason "failed_readiness" \
        --last-error "postgres did not become ready in startup window"
      log_error "PostgreSQL failed to start. Check $PGDATA/postgres.log"
      print_log_tail "$PGDATA/postgres.log" 20 "postgres"
      exit 1
    '';
  };

  start = mkPgScript {
    name = "postgres-start";
    body = ''
      ${init}
      ${checkConfig}
      ${preflightStart}
      exec ${startLeaf}
    '';
  };

  stop = mkPgScript {
    name = "postgres-stop";
    body = ''
      if [ -n "''${PGDATA:-}" ] && [ -f "$PGDATA/postmaster.pid" ]; then
        RUN_PID=$(head -1 "$PGDATA/postmaster.pid" 2>/dev/null || true)
        log_stop "PostgreSQL at $PGDATA"
        if ! ${postgres}/bin/pg_ctl -D "$PGDATA" stop -m fast -w -t 60 >/dev/null 2>&1; then
          log_error "PostgreSQL failed to stop at $PGDATA"
          exit 1
        fi

        for i in $(seq 1 60); do
          if ${startupPgIsReadyCommand}
          then
            sleep 0.5
          else
            emit_service_event service_stopped --pid "$RUN_PID" --log-path "$PGDATA/postgres.log"
            exit 0
          fi
        done

        log_error "PostgreSQL still responds on port $PGPORT after stop"
        exit 1
      else
        emit_service_event service_stopped
        exit 0
      fi
    '';
  };

  restart = mkPgScript {
    name = "postgres-restart";
    body = ''
      ${stop}
      exec ${start}
    '';
  };

  status = managedServiceLifecycle.mkObservedStatusScript {
    name = "postgres-status";
    inherit loggingPrelude;
    runtimePrelude = pgRuntimePrelude database;
    runningStateBody = ''
      if ${startupPgIsReadyCommand}
      then
        RUNNING=true
      fi

      if [ -f "$PGDATA/postmaster.pid" ]; then
        PID=$(head -1 "$PGDATA/postmaster.pid" 2>/dev/null || true)
      fi
    '';
    statusMergeBlock = observability.mkStatusMergeBlock {
      service = "postgres";
      defaultLogPathExpr = ''"$PGDATA/postgres.log"'';
    };
    statusBody = observability.mkStatusLine {
      service = "postgres";
      beforeRunningFields = [
        "port=$PGPORT"
        "pgdata=$PGDATA"
      ];
    };
  };

  health = mkPgScript {
    name = "postgres-health";
    body = managedServiceLifecycle.mkPlanProbeBody {
      planBody = ''
        service_source=${pkgs.lib.escapeShellArg serviceSource}
        ${healthPlanBody}
      '';
      skipMessage = "SKIP: postgres health check has no probe steps";
    };
  };

  ready = mkPgScript {
    name = "postgres-ready";
    body = managedServiceLifecycle.mkPlanProbeBody {
      planBody = ''
        service_source=${pkgs.lib.escapeShellArg serviceSource}
        ${readyPlanBody}
      '';
      skipMessage = "SKIP: postgres readiness check has no probe steps";
      wait = readyPlan.wait or null;
      timeoutMessage = "PostgreSQL not ready after ${
        toString ((readyPlan.wait or { }).timeoutSeconds or runtimeDefaults.probes.wait.timeoutSeconds)
      } s";
    };
  };

  readyTest = mkPgScript {
    name = "postgres-ready-test";
    body = ''
      READY_TEST_DATABASE=${pkgs.lib.escapeShellArg testDatabase}

      if ! ${readyTestPgIsReadyCommand}
      then
        log_error "PostgreSQL not ready for test db port=$PGPORT database=$READY_TEST_DATABASE (pg_isready failed)"
        exit 1
      fi

      if ! ${readyTestMaintenanceQueryCommand}
      then
        log_error "PostgreSQL not ready for test db port=$PGPORT database=$READY_TEST_DATABASE (maintenance query failed)"
        exit 1
      fi

      if ${readyTestDatabaseQueryCommand}
      then
        log_ok "PostgreSQL ready for test db port=$PGPORT database=$READY_TEST_DATABASE"
        exit 0
      fi

      log_error "PostgreSQL not ready for test db port=$PGPORT database=$READY_TEST_DATABASE (database query failed)"
      exit 1
    '';
  };

  checkConfig = mkPgScript {
    name = "postgres-check-config";
    body = ''
      if [ ! -f "$PGDATA/postgresql.conf" ]; then
        nixfied_exit_precondition "missing postgresql.conf at $PGDATA"
      fi

      if ${postgres}/bin/postgres -D "$PGDATA" -C port >/dev/null 2>&1; then
        log_ok "PostgreSQL configuration valid pgdata=$PGDATA"
        exit 0
      fi

      nixfied_exit_precondition "PostgreSQL configuration invalid pgdata=$PGDATA"
    '';
  };

  setupDb = mkPgScript {
    name = "postgres-setup-db";
    body = ''
      log_info "Setting up database '$PGDATABASE'"

      ${postgres}/bin/psql -h ${runtimeDefaults.hosts.localhost} -p "$PGPORT" -U postgres -d postgres -c \
        "DO \$\$ BEGIN CREATE ROLE postgres WITH LOGIN SUPERUSER PASSWORD 'postgres'; EXCEPTION WHEN duplicate_object THEN NULL; END \$\$;" 2>/dev/null || true

      ${postgres}/bin/createdb -h ${runtimeDefaults.hosts.localhost} -p "$PGPORT" -U postgres "$PGDATABASE" 2>/dev/null || true

      if [ -n "${pkgs.lib.concatStringsSep " " extensions}" ]; then
        for ext in ${pkgs.lib.concatStringsSep " " extensions}; do
          ${postgres}/bin/psql -h ${runtimeDefaults.hosts.localhost} -p "$PGPORT" -U postgres -d "$PGDATABASE" \
            -c "CREATE EXTENSION IF NOT EXISTS $ext;" 2>/dev/null || true
        done
      fi

      log_ok "Database '$PGDATABASE' ready"
    '';
  };

  fullStartLeaf = mkPgScript {
    name = "postgres-full-start-leaf";
    body = ''
      log_info "Slot $SLOT, env $ENV (PGPORT=$PGPORT)"

      ${setupDb}

      echo "PGPORT=$PGPORT"
      echo "PGDATA=$PGDATA"
      echo "PGDATABASE=$PGDATABASE"
    '';
  };

  fullStart = mkPgScript {
    name = "postgres-full-start";
    body = ''
      ${start}
      exec ${fullStartLeaf}
    '';
  };

  fullStartTestLeaf = mkPgScript {
    name = "postgres-full-start-test-leaf";
    defaultDb = testDatabase;
    body = ''
      export PGDATABASE="''${PGDATABASE:-${testDatabase}}"

      ${setupDb}
    '';
  };

  fullStartTest = mkPgScript {
    name = "postgres-full-start-test";
    defaultDb = testDatabase;
    body = ''
      ${start}
      exec ${fullStartTestLeaf}
    '';
  };

  listInstances = mkWrappedScript {
    name = "postgres-list-instances";
    body = ''
      echo "PostgreSQL instances:"
      echo ""
      for pidfile in $(find "''${XDG_DATA_HOME:-$HOME/.local/share}" -name "postmaster.pid" 2>/dev/null || true); do
        PGDATA_DIR=$(dirname "$pidfile")
        PID=$(head -1 "$pidfile" 2>/dev/null || echo "unknown")
        PORT=$(sed -n '4p' "$pidfile" 2>/dev/null || echo "unknown")
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
          STATUS="running"
        else
          STATUS="stale"
        fi
        echo "  $PGDATA_DIR (PID: $PID, Port: $PORT, Status: $STATUS)"
      done
    '';
  };

in
{
  inherit
    postgres
    preflightInit
    initLeaf
    init
    preflightStart
    startLeaf
    start
    stop
    restart
    status
    health
    ready
    readyTest
    checkConfig
    setupDb
    fullStartLeaf
    fullStart
    fullStartTestLeaf
    fullStartTest
    listInstances
    ;
}
