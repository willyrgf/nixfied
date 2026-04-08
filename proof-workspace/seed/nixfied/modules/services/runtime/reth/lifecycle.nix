# Reth lifecycle management - init, start, stop, restart, status, health, check-config
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
    serviceName = "reth";
    endpointMapping = {
      http = "$RETH_HTTP_PORT";
      ws = "$RETH_WS_PORT";
      auth = "$RETH_AUTH_PORT";
    };
  };

  inherit (probeSetup)
    lib
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

  reth = config.package or pkgs.reth;
  httpPortVar = slots.portVarName config.portKeyHttp;
  wsPortVar = slots.portVarName config.portKeyWs;
  authPortVar = slots.portVarName config.portKeyAuth;
  rethDirExpr = slots.getServiceDir config.dataDirName;
  useDevMode = config.devMode or false;
  extraArgs = lib.escapeShellArgs (config.extraArgs or [ ]);
  startupHealthCheck = ''
    {
      service_source=${lib.escapeShellArg serviceSource}
      ${renderProbeStep "health" {
        kind = "jsonrpc";
        endpoint = "http";
        serviceLabel = "reth";
        phaseLabel = "health";
        successLabel = "healthy";
        failureLabel = "unhealthy";
        method = "web3_clientVersion";
      }}
    } >/dev/null 2>&1
  '';
  runtimePrelude = import ../shared/service-runtime-prelude.nix {
    inherit slotEnvRuntime slots observability;
    serviceName = "reth";
    serviceNameUpper = "RETH";
    portVars = [
      {
        varName = "HTTP_PORT_VAR";
        portVar = httpPortVar;
        target = "RETH_HTTP_PORT";
      }
      {
        varName = "WS_PORT_VAR";
        portVar = wsPortVar;
        target = "RETH_WS_PORT";
      }
      {
        varName = "AUTH_PORT_VAR";
        portVar = authPortVar;
        target = "RETH_AUTH_PORT";
      }
    ];
    dirExpr = rethDirExpr;
    portValidation = ''
      RETH_NETWORK="''${RETH_NETWORK:-${config.network or "local"}}"
      RETH_USE_DEV="${if useDevMode then "1" else "0"}"

      if [ "$RETH_NETWORK" = "local" ]; then
        RETH_USE_DEV="1"
      fi

      if [ -z "$RETH_HTTP_PORT" ] || [ -z "$RETH_WS_PORT" ] || [ -z "$RETH_AUTH_PORT" ]; then
        log_error "reth port variables are not set (http/ws/auth)"
        exit 1
      fi
    '';
    extraPrelude = ''
      RETH_JWT_FILE="$RETH_DIR/config/jwt.hex"
    '';
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
        nixfied_exit_precondition "missing reth binary at ${reth}/bin/reth"
      fi

      mkdir -p "$RETH_DIR/config"
      if ${reth}/bin/reth --version >/dev/null 2>&1; then
        log_ok "reth configuration valid dir=$RETH_DIR network=$RETH_NETWORK"
        exit 0
      fi

      nixfied_exit_precondition "reth binary failed validation at ${reth}/bin/reth"
    '';
    startPreflightBody = ''
      if [ ! -f "$RETH_JWT_FILE" ]; then
        nixfied_exit_precondition "missing reth JWT secret at $RETH_JWT_FILE"
      fi

      if [ ! -s "$RETH_JWT_FILE" ]; then
        nixfied_exit_precondition "reth JWT secret is empty path=$RETH_JWT_FILE"
      fi
    '';
    startPrepareBody = ''
      ARGS=(
        node
        --datadir "$RETH_DIR/data"
        --ipcpath "$RETH_DIR/run/reth.ipc"
        --http
        --http.addr ${runtimeDefaults.hosts.loopbackIp}
        --http.port "$RETH_HTTP_PORT"
        --ws
        --ws.addr ${runtimeDefaults.hosts.loopbackIp}
        --ws.port "$RETH_WS_PORT"
        --authrpc.addr ${runtimeDefaults.hosts.loopbackIp}
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
    startAlreadyRunningBody = managedServiceLifecycle.mkReadyOutcomeBody {
      level = "ok";
      pidExpr = ''"$PID"'';
      message = "reth already running pid=$PID http_port=$RETH_HTTP_PORT";
    };
    startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {
      probeCommand = startupHealthCheck;
      serviceLabel = "reth";
      probeAttempts = runtimeDefaults.probes.startupReadiness.extendedAttempts;
      degradedWaitReason = "failed_readiness";
      degradedLastError = "reth failed health check during startup";
      failureMessage = "reth failed to become healthy";
      successMessage = "reth started pid=$CHILD_PID http_port=$RETH_HTTP_PORT ws_port=$RETH_WS_PORT auth_port=$RETH_AUTH_PORT";
    };
    startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {
      waitReason = "reth_process_exit code=$RC";
      lastError = "reth process exited non-zero";
    };
    statusMergeBlock = observability.mkStatusMergeBlock {
      service = "reth";
      defaultLogPathExpr = ''"$RETH_LOG_FILE"'';
    };
    statusBody = observability.mkStatusLine {
      service = "reth";
      afterPidFields = [
        "http_port=$RETH_HTTP_PORT"
        "ws_port=$RETH_WS_PORT"
        "auth_port=$RETH_AUTH_PORT"
        "network=$RETH_NETWORK"
      ];
    };
    healthBody = managedServiceLifecycle.mkPlanProbeBody {
      planBody = ''
        service_source=${lib.escapeShellArg serviceSource}
        ${healthPlanBody}
      '';
      skipMessage = "SKIP: reth health check has no probe steps";
    };
    readyBody = managedServiceLifecycle.mkPlanProbeBody {
      planBody = ''
        service_source=${lib.escapeShellArg serviceSource}
        ${readyPlanBody}
      '';
      skipMessage = "SKIP: reth readiness check has no probe steps";
      wait = readyPlan.wait or null;
      timeoutMessage = "reth not ready after ${
        toString ((readyPlan.wait or { }).timeoutSeconds or runtimeDefaults.probes.wait.timeoutSeconds)
      } s";
    };
    stopWaitAttempts = runtimeDefaults.probes.managedStop.extendedWaitAttempts;
    stopWaitInterval = runtimeDefaults.probes.managedStop.extendedWaitIntervalSeconds;
  };
in
{
  inherit reth;
  inherit (managedLifecycle)
    init
    preflightStart
    startLeaf
    start
    stop
    restart
    status
    health
    checkConfig
    ready
    fullStartLeaf
    fullStartTestLeaf
    fullStart
    fullStartTest
    ;
}
