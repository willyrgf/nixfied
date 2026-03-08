# Reth lifecycle management - init, start, stop, restart, status, health, check-config
{
  pkgs,
  project,
  slots,
  config,
  loggingPrelude,
}:

let
  lib = pkgs.lib;
  managedServiceLifecycle = import ../lib/managed-service-lifecycle.nix { inherit pkgs; };
  probeCommands = import ../lib/probe-commands.nix { inherit pkgs; };
  slotEnvRuntime = import ../lib/slot-env-runtime.nix { inherit pkgs; };
  processRegistry = import ../lib/process-registry.nix { inherit pkgs project; };
  observability = import ../lib/service-observability.nix {
    inherit
      pkgs
      slots
      processRegistry
      ;
  };
  reth = config.package or pkgs.reth;
  httpPortVar = slots.portVarName config.portKeyHttp;
  wsPortVar = slots.portVarName config.portKeyWs;
  authPortVar = slots.portVarName config.portKeyAuth;
  rethDirExpr = slots.getServiceDir config.dataDirName;
  useDevMode = config.devMode or false;
  extraArgs = lib.escapeShellArgs (config.extraArgs or [ ]);
  emitHelper = observability.mkEmitServiceEventFunction "reth";

  runtimePrelude = ''
    ${slotEnvRuntime.loadJsonFromCommand {
      outVar = "SLOT_INFO_JSON_OUT";
      command = toString slots.getSlotInfoJson;
      exportVars = false;
    }}

    HTTP_PORT_VAR="${httpPortVar}"
    WS_PORT_VAR="${wsPortVar}"
    AUTH_PORT_VAR="${authPortVar}"

    ${slotEnvRuntime.readPortFromJson {
      targetVar = "RETH_HTTP_PORT";
      jsonVar = "SLOT_INFO_JSON_OUT";
      keyExpr = "$HTTP_PORT_VAR";
    }}
    ${slotEnvRuntime.readPortFromJson {
      targetVar = "RETH_WS_PORT";
      jsonVar = "SLOT_INFO_JSON_OUT";
      keyExpr = "$WS_PORT_VAR";
    }}
    ${slotEnvRuntime.readPortFromJson {
      targetVar = "RETH_AUTH_PORT";
      jsonVar = "SLOT_INFO_JSON_OUT";
      keyExpr = "$AUTH_PORT_VAR";
    }}
    RETH_DIR="${rethDirExpr}"
    RETH_PID_FILE="$RETH_DIR/run/reth.pid"
    RETH_LOG_FILE="$RETH_DIR/logs/reth.log"
    RETH_JWT_FILE="$RETH_DIR/config/jwt.hex"
    SERVICE_DIR="$RETH_DIR"
    SERVICE_PID_FILE="$RETH_PID_FILE"
    SERVICE_LOG_FILE="$RETH_LOG_FILE"
    RETH_NETWORK="''${RETH_NETWORK:-${config.network or "local"}}"
    RETH_USE_DEV="${if useDevMode then "1" else "0"}"

    if [ "$RETH_NETWORK" = "local" ]; then
      RETH_USE_DEV="1"
    fi

    if [ -z "$RETH_HTTP_PORT" ] || [ -z "$RETH_WS_PORT" ] || [ -z "$RETH_AUTH_PORT" ]; then
      log_error "reth port variables are not set (http/ws/auth)"
      exit 1
    fi

    ${emitHelper}
  '';

  healthCheck = probeCommands.jsonRpcHasResultCmd {
    urlExpr = "http://127.0.0.1:$RETH_HTTP_PORT";
    method = "web3_clientVersion";
  };
  managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle {
    service = "reth";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    initBody = ''
      mkdir -p "$SERVICE_DIR/data"
      mkdir -p "$SERVICE_DIR/config"
      mkdir -p "$SERVICE_DIR/run"
      mkdir -p "$SERVICE_DIR/logs"

      if [ ! -f "$RETH_JWT_FILE" ]; then
        printf '%064x\n' 0 > "$RETH_JWT_FILE"
      fi
      chmod 600 "$RETH_JWT_FILE" 2>/dev/null || true

      log_ok "reth initialized dir=$RETH_DIR slot=$SLOT env=$ENV"
    '';
    checkConfigBody = ''
      if [ ! -x "${reth}/bin/reth" ]; then
        log_error "missing reth binary at ${reth}/bin/reth"
        exit 1
      fi

      mkdir -p "$RETH_DIR/config"
      ${reth}/bin/reth --version >/dev/null 2>&1
      log_ok "reth configuration valid dir=$RETH_DIR network=$RETH_NETWORK"
    '';
    startPreflight = ''
      if [ ! -x "${reth}/bin/reth" ]; then
        log_error "reth binary not executable at ${reth}/bin/reth"
        exit 1
      fi

      ARGS=(
        node
        --datadir "$RETH_DIR/data"
        --ipcpath "$RETH_DIR/run/reth.ipc"
        --http
        --http.addr 127.0.0.1
        --http.port "$RETH_HTTP_PORT"
        --ws
        --ws.addr 127.0.0.1
        --ws.port "$RETH_WS_PORT"
        --authrpc.addr 127.0.0.1
        --authrpc.port "$RETH_AUTH_PORT"
        --authrpc.jwtsecret "$RETH_JWT_FILE"
      )

      if [ "$RETH_USE_DEV" = "1" ]; then
        ARGS+=(--dev)
      else
        ARGS+=(--chain "$RETH_NETWORK")
      fi

      ${lib.optionalString ((config.extraArgs or [ ]) != [ ]) ''
        EXTRA_ARGS=(${extraArgs})
        ARGS+=("''${EXTRA_ARGS[@]}")
      ''}
    '';
    startCommand = ''
      "${reth}/bin/reth" "''${ARGS[@]}" > "$LOG_FILE" 2>&1 &
    '';
    startAlreadyRunningBody = ''
      emit_service_event service_ready ready --pid "$PID" --log-path "$LOG_FILE"
      log_ok "reth already running pid=$PID http_port=$RETH_HTTP_PORT"
    '';
    startPostLaunchBody = ''
      READY=0
      for _ in $(seq 1 80); do
        if ! kill -0 "$CHILD_PID" 2>/dev/null; then
          break
        fi
        if ${healthCheck}
        then
          READY=1
          break
        fi
        sleep 0.25
      done

      if [ "$READY" -ne 1 ]; then
        emit_service_event service_degraded degraded \
          --pid "$CHILD_PID" \
          --log-path "$LOG_FILE" \
          --wait-reason "failed_readiness" \
          --last-error "reth failed health check during startup"
        log_error "reth failed to become healthy. log=$LOG_FILE"
        if [ -f "$LOG_FILE" ]; then
          log_info "reth log tail path=$LOG_FILE lines=50"
          tail -50 "$LOG_FILE" >&2 || true
        else
          log_warn "reth log file missing path=$LOG_FILE"
        fi
        exit 1
      fi

      emit_service_event service_ready ready --pid "$CHILD_PID" --log-path "$LOG_FILE"
      log_info "reth started pid=$CHILD_PID http_port=$RETH_HTTP_PORT ws_port=$RETH_WS_PORT auth_port=$RETH_AUTH_PORT"
    '';
    startExitFailureBody = ''
      emit_service_event service_degraded degraded \
        --pid "$CHILD_PID" \
        --log-path "$LOG_FILE" \
        --wait-reason "reth_process_exit code=$RC" \
        --last-error "reth process exited non-zero"
    '';
    statusMergeBlock = observability.mkStatusMergeBlock {
      service = "reth";
      defaultLogPathExpr = ''"$RETH_LOG_FILE"'';
    };
    statusBody = ''
      echo "service=reth slot=$SLOT env=$ENV running=$RUNNING pid=''${PID:-unknown} http_port=$RETH_HTTP_PORT ws_port=$RETH_WS_PORT auth_port=$RETH_AUTH_PORT network=$RETH_NETWORK scope=$SCOPE owner_run_id=''${OWNER_RUN_ID:-unknown} owner_scope=''${OWNER_SCOPE:-unknown} ephemeral_root=''${EPHEMERAL_ROOT:-none} registry_state=''${REGISTRY_STATE:-unknown} slot_owner=''${SLOT_OWNER:-unknown} wait_reason=''${WAIT_REASON:-none} log_path=$EFFECTIVE_LOG_PATH"
    '';
    healthBody = ''
      if ${healthCheck}
      then
        log_ok "reth healthy http_port=$RETH_HTTP_PORT"
        exit 0
      fi

      log_error "reth unhealthy http_port=$RETH_HTTP_PORT"
      exit 1
    '';
    readyBody = ''
      if ${healthCheck}
      then
        log_ok "reth ready http_port=$RETH_HTTP_PORT"
        exit 0
      fi

      log_error "reth not ready http_port=$RETH_HTTP_PORT"
      exit 1
    '';
    stopWaitAttempts = 40;
    stopWaitInterval = "0.25";
  };
in
{
  inherit reth;
  inherit (managedLifecycle)
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
}
