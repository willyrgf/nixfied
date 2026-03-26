# Nginx lifecycle management - init, start, stop, reload
{
  pkgs,
  project,
  slots,
  config,
  templates,
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
    serviceName = "nginx";
    endpointMapping = {
      http = "$HTTP_PORT";
      https = "$HTTPS_PORT";
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

  # Nginx lifecycle historically named this `serviceScripts`
  serviceScripts = managedServiceLifecycle;

  nginx = templates.nginx;
  portVarHttp = slots.portVarName (config.portKeyHttp or "http");
  portVarHttps = slots.portVarName (config.portKeyHttps or "https");
  dataDirName = config.dataDirName or "nginx";
  nginxDirExpr = slots.getServiceDir dataDirName;

  startupProbeCommand = ''
    {
      service_source=${lib.escapeShellArg serviceSource}
      ${renderProbeStep "health" {
        kind = "tcp";
        endpoint = "http";
        serviceLabel = "nginx";
        phaseLabel = "health";
        successLabel = "healthy";
        failureLabel = "unhealthy";
      }}
    } >/dev/null 2>&1
  '';

  runtimePrelude = import ../service-runtime-prelude.nix {
    inherit slotEnvRuntime slots observability;
    serviceName = "nginx";
    serviceNameUpper = "NGINX";
    portVars = [
      {
        varName = "HTTP_PORT_VAR";
        portVar = portVarHttp;
        target = "HTTP_PORT";
      }
      {
        varName = "HTTPS_PORT_VAR";
        portVar = portVarHttps;
        target = "HTTPS_PORT";
      }
    ];
    dirExpr = nginxDirExpr;
    pidFileName = "nginx.pid";
    logFileName = "error.log";
    portValidation = ''
      if [ -z "$HTTP_PORT" ] || [ -z "$HTTPS_PORT" ]; then
        log_error "nginx port variables are not set (http/https)"
        exit 1
      fi
    '';
  };

  generateSelfSignedCert = serviceScripts.mkWrappedScript {
    name = "nginx-generate-self-signed";
    inherit loggingPrelude;
    runtimePrelude = "";
    body = ''
      DOMAIN="$1"
      SSL_DIR="$2"

      CERT_DIR="$SSL_DIR/live/$DOMAIN"
      mkdir -p "$CERT_DIR"

      if [ -f "$CERT_DIR/fullchain.pem" ] && [ -f "$CERT_DIR/privkey.pem" ]; then
        exit 0
      fi

      ${pkgs.openssl}/bin/openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
        -keyout "$CERT_DIR/privkey.pem" \
        -out "$CERT_DIR/fullchain.pem" \
        -subj "/CN=$DOMAIN" \
        2>/dev/null
      cp "$CERT_DIR/fullchain.pem" "$CERT_DIR/chain.pem"
    '';
  };

  managedLifecycle = serviceScripts.mkPidFileManagedLifecycle {
    service = "nginx";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    initBody = ''
      mkdir -p "$NGINX_DIR/conf"
      mkdir -p "$SERVICE_DIR/conf/sites-available"
      mkdir -p "$SERVICE_DIR/conf/sites-enabled"
      mkdir -p "$SERVICE_DIR/ssl/live/localhost"
      mkdir -p "$SERVICE_DIR/logs"
      mkdir -p "$SERVICE_DIR/html"
      mkdir -p "$SERVICE_DIR/run"

      ${pkgs.gnused}/bin/sed \
        -e "s|NGINX_DIR|$NGINX_DIR|g" \
        -e "s|HTTP_PORT|$HTTP_PORT|g" \
        -e "s|HTTPS_PORT|$HTTPS_PORT|g" \
        "${templates.nginxConfTemplate}" > "$NGINX_DIR/conf/nginx.conf"

      ${generateSelfSignedCert} "localhost" "$NGINX_DIR/ssl"
      log_ok "nginx initialized dir=$NGINX_DIR slot=$SLOT env=$ENV"
    '';
    checkConfigBody = ''
      CONF="$NGINX_DIR/conf/nginx.conf"

      if [ ! -f "$CONF" ]; then
        nixfied_exit_precondition "missing nginx config at $CONF"
      fi

      if ${nginx}/bin/nginx -c "$CONF" -t 2>&1; then
        log_ok "nginx configuration valid conf=$CONF"
        exit 0
      fi

      nixfied_exit_precondition "nginx configuration invalid conf=$CONF"
    '';
    startPreflightBody = ''
      CONF="$NGINX_DIR/conf/nginx.conf"

      if [ ! -f "$CONF" ]; then
        nixfied_exit_precondition "nginx not initialized at $NGINX_DIR (missing $CONF)"
      fi
    '';
    startPrepareBody = ''
      CONF="$NGINX_DIR/conf/nginx.conf"
    '';
    startCommand = ''
      ${nginx}/bin/nginx -c "$CONF" -g 'daemon off;' > "$LOG_FILE" 2>&1 &
    '';
    startAlreadyRunningBody = serviceScripts.mkReadyOutcomeBody {
      level = "ok";
      pidExpr = ''"$PID"'';
      message = "nginx already running pid=$PID http_port=$HTTP_PORT";
    };
    startPostLaunchBody = serviceScripts.mkStartupReadinessBody {
      probeCommand = startupProbeCommand;
      serviceLabel = "nginx";
      degradedWaitReason = "failed_readiness";
      degradedLastError = "nginx failed health check during startup";
      failureMessage = "nginx failed to become healthy";
      successMessage = "nginx started pid=$CHILD_PID http_port=$HTTP_PORT https_port=$HTTPS_PORT";
    };
    startExitFailureBody = serviceScripts.mkProcessExitFailureBody {
      waitReason = "nginx_process_exit code=$RC";
      lastError = "nginx process exited non-zero";
    };
    stopRequestBody = ''
      if [ -f "$NGINX_DIR/conf/nginx.conf" ]; then
        ${nginx}/bin/nginx -c "$NGINX_DIR/conf/nginx.conf" -s quit 2>/dev/null || kill "$PID" 2>/dev/null || true
      else
        kill "$PID" 2>/dev/null || true
      fi
    '';
    statusMergeBlock = observability.mkStatusMergeBlock {
      service = "nginx";
      defaultLogPathExpr = ''"$NGINX_LOG_FILE"'';
    };
    statusBody = observability.mkStatusLine {
      service = "nginx";
      afterPidFields = [
        "http_port=$HTTP_PORT"
        "https_port=$HTTPS_PORT"
      ];
    };
    healthBody = serviceScripts.mkPlanProbeBody {
      planBody = ''
        service_source=${lib.escapeShellArg serviceSource}
        ${healthPlanBody}
      '';
      skipMessage = "SKIP: nginx health check has no probe steps";
    };
    readyBody = serviceScripts.mkPlanProbeBody {
      planBody = ''
        service_source=${lib.escapeShellArg serviceSource}
        ${readyPlanBody}
      '';
      skipMessage = "SKIP: nginx readiness check has no probe steps";
      wait = readyPlan.wait or null;
      timeoutMessage = "nginx not ready after ${
        toString ((readyPlan.wait or { }).timeoutSeconds or runtimeDefaults.probes.wait.timeoutSeconds)
      } s";
      successBody = ''
        PID=$(cat "$NGINX_PID_FILE" 2>/dev/null || true)
        emit_service_event service_ready --pid "$PID" --log-path "$NGINX_LOG_FILE"
      '';
    };
    stopWaitAttempts = runtimeDefaults.probes.managedStop.extendedWaitAttempts;
    stopWaitInterval = runtimeDefaults.probes.managedStop.extendedWaitIntervalSeconds;
  };

  reload = serviceScripts.mkWrappedScript {
    name = "nginx-reload";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    body = ''
      CONF="$NGINX_DIR/conf/nginx.conf"

      if [ ! -f "$CONF" ]; then
        log_error "Nginx not initialized."
        exit 1
      fi

      # Test config before reload
      log_info "Testing nginx configuration"
      ${nginx}/bin/nginx -c "$CONF" -t 2>&1

      log_info "Reloading nginx"
      ${nginx}/bin/nginx -c "$CONF" -s reload
      log_ok "Nginx reloaded"
    '';
  };

  listInstances = serviceScripts.mkWrappedScript {
    name = "nginx-list-instances";
    inherit loggingPrelude;
    runtimePrelude = "";
    body = ''
      echo "Nginx instances:"
      echo ""
      for pidfile in $(find "''${XDG_DATA_HOME:-$HOME/.local/share}" -name "nginx.pid" 2>/dev/null || true); do
        DIR=$(dirname "$(dirname "$pidfile")")
        PID=$(cat "$pidfile" 2>/dev/null || echo "unknown")
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
          STATUS="running"
        else
          STATUS="stale"
        fi
        echo "  $DIR (PID: $PID, Status: $STATUS)"
      done
    '';
  };

in
{
  inherit
    nginx
    reload
    generateSelfSignedCert
    listInstances
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
