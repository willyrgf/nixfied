# MinIO lifecycle management - init, start, stop, restart, status, health, check-config
{
  pkgs,
  project,
  slots,
  config,
  loggingPrelude,
}:

let
  probeSetup = import ../probe-setup-helper.nix {
    inherit
      pkgs
      project
      slots
      config
      ;
    serviceName = "minio";
    endpointMapping = {
      api = "$MINIO_API_PORT";
      console = "$MINIO_CONSOLE_PORT";
    };
  };

  inherit (probeSetup)
    lib
    runtimeDefaults
    managedServiceLifecycle
    probeCommands
    slotEnvRuntime
    observability
    serviceSource
    readyPlan
    healthPlanBody
    readyPlanBody
    ;

  minio = config.package or pkgs.minio;
  apiPortVar = slots.portVarName config.portKeyApi;
  consolePortVar = slots.portVarName config.portKeyConsole;
  minioDirExpr = slots.getServiceDir config.dataDirName;
  browserValue = if config.browser then "on" else "off";
  startupProbeCommand = ''
    {
      service_source=${lib.escapeShellArg serviceSource}
      ${healthPlanBody}
    } >/dev/null 2>&1
  '';

  runtimePrelude = import ../service-runtime-prelude.nix {
    inherit slotEnvRuntime slots observability;
    serviceName = "minio";
    serviceNameUpper = "MINIO";
    portVars = [
      {
        varName = "API_PORT_VAR";
        portVar = apiPortVar;
        target = "MINIO_API_PORT";
      }
      {
        varName = "CONSOLE_PORT_VAR";
        portVar = consolePortVar;
        target = "MINIO_CONSOLE_PORT";
      }
    ];
    dirExpr = minioDirExpr;
    portValidation = ''
      if [ -z "$MINIO_API_PORT" ] || [ -z "$MINIO_CONSOLE_PORT" ]; then
        log_error "minio port variables are not set (api/console)"
        exit 1
      fi
    '';
  };

  managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle {
    service = "minio";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    initBody = ''
      mkdir -p "$SERVICE_DIR/data"
      mkdir -p "$SERVICE_DIR/config"
      mkdir -p "$SERVICE_DIR/run"
      mkdir -p "$SERVICE_DIR/logs"

      log_ok "minio initialized dir=$MINIO_DIR slot=$SLOT env=$ENV"
    '';
    checkConfigBody = ''
      if [ ! -d "$MINIO_DIR/config" ]; then
        nixfied_exit_precondition "missing minio config directory at $MINIO_DIR/config"
      fi

      if ${minio}/bin/minio --help >/dev/null; then
        log_ok "minio configuration valid dir=$MINIO_DIR"
        exit 0
      fi

      nixfied_exit_precondition "minio binary failed validation at ${minio}/bin/minio"
    '';
    startPreflightBody = ''
      ROOT_USER="''${MINIO_ROOT_USER:-${config.rootUser}}"
      ROOT_PASSWORD="''${MINIO_ROOT_PASSWORD:-${config.rootPassword}}"

      if [ -z "$ROOT_USER" ]; then
        nixfied_exit_precondition "MINIO_ROOT_USER must not be empty"
      fi

      if [ -z "$ROOT_PASSWORD" ]; then
        nixfied_exit_precondition "MINIO_ROOT_PASSWORD must not be empty"
      fi
    '';
    startPrepareBody = ''
      ROOT_USER="''${MINIO_ROOT_USER:-${config.rootUser}}"
      ROOT_PASSWORD="''${MINIO_ROOT_PASSWORD:-${config.rootPassword}}"

      export MINIO_ROOT_USER="$ROOT_USER"
      export MINIO_ROOT_PASSWORD="$ROOT_PASSWORD"
      export MINIO_BROWSER="${browserValue}"
    '';
    startCommand = ''
      ${minio}/bin/minio server "$MINIO_DIR/data" \
        --address "${runtimeDefaults.hosts.loopbackIp}:$MINIO_API_PORT" \
        --console-address "${runtimeDefaults.hosts.loopbackIp}:$MINIO_CONSOLE_PORT" \
        --config-dir "$MINIO_DIR/config" \
        > "$LOG_FILE" 2>&1 &
    '';
    startAlreadyRunningBody = managedServiceLifecycle.mkReadyOutcomeBody {
      level = "ok";
      pidExpr = ''"$PID"'';
      message = "minio already running pid=$PID api_port=$MINIO_API_PORT";
    };
    startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {
      probeCommand = startupProbeCommand;
      serviceLabel = "minio";
      probeAttempts = runtimeDefaults.probes.startupReadiness.extendedAttempts;
      degradedWaitReason = "failed_readiness";
      degradedLastError = "minio failed health check during startup";
      failureMessage = "minio failed to become healthy";
      successMessage = "minio started pid=$CHILD_PID api_port=$MINIO_API_PORT console_port=$MINIO_CONSOLE_PORT";
    };
    startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {
      waitReason = "minio_process_exit code=$RC";
      lastError = "minio process exited non-zero";
    };
    stopMissingLogPathExpr = null;
    stopStateLogPathExpr = null;
    statusMergeBlock = observability.mkStatusMergeBlock {
      service = "minio";
      defaultLogPathExpr = ''"$MINIO_LOG_FILE"'';
    };
    statusBody = observability.mkStatusLine {
      service = "minio";
      afterPidFields = [
        "api_port=$MINIO_API_PORT"
        "console_port=$MINIO_CONSOLE_PORT"
      ];
    };
    healthBody = managedServiceLifecycle.mkPlanProbeBody {
      planBody = ''
        service_source=${lib.escapeShellArg serviceSource}
        ${healthPlanBody}
      '';
      skipMessage = "SKIP: minio health check has no probe steps";
    };
    readyBody = managedServiceLifecycle.mkPlanProbeBody {
      planBody = ''
        service_source=${lib.escapeShellArg serviceSource}
        ${readyPlanBody}
      '';
      skipMessage = "SKIP: minio readiness check has no probe steps";
      wait = readyPlan.wait or null;
      timeoutMessage = "minio not ready after ${
        toString ((readyPlan.wait or { }).timeoutSeconds or runtimeDefaults.probes.wait.timeoutSeconds)
      } s";
    };
  };

  exportS3Env = pkgs.writeShellScript "minio-export-s3-env" ''
    ${loggingPrelude}

    set -euo pipefail
    ${runtimePrelude}

    BUCKET="''${1:-''${MINIO_BUCKET:-}}"
    PREFIX="''${2:-''${MINIO_PREFIX:-}}"
    REGION="''${3:-''${MINIO_REGION:-us-east-1}}"

    ROOT_USER="''${MINIO_ROOT_USER:-${config.rootUser}}"
    ROOT_PASSWORD="''${MINIO_ROOT_PASSWORD:-${config.rootPassword}}"

    echo "export AWS_ACCESS_KEY_ID=\"$ROOT_USER\""
    echo "export AWS_SECRET_ACCESS_KEY=\"$ROOT_PASSWORD\""
    echo "export AWS_EC2_METADATA_DISABLED=\"true\""
    echo "export MINIO_ENDPOINT=\"${probeCommands.localHttpUrlExpr "$MINIO_API_PORT"}\""
    echo "export MINIO_REGION=\"$REGION\""
    echo "export MINIO_BUCKET=\"$BUCKET\""
    echo "export MINIO_PREFIX=\"$PREFIX\""
    echo "export S3_ENDPOINT=\"${probeCommands.localHttpUrlExpr "$MINIO_API_PORT"}\""
    echo "export S3_REGION=\"$REGION\""
    echo "export S3_BUCKET=\"$BUCKET\""
    echo "export S3_PREFIX=\"$PREFIX\""
  '';
in
{
  inherit
    minio
    exportS3Env
    ;
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
