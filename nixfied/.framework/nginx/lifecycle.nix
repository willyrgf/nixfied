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
  serviceScripts = import ../lib/managed-service-lifecycle.nix { inherit pkgs; };
  slotEnvRuntime = import ../lib/slot-env-runtime.nix { inherit pkgs; };
  processRegistry = import ../lib/process-registry.nix { inherit pkgs project; };
  observability = import ../lib/service-observability.nix {
    inherit
      pkgs
      slots
      processRegistry
      ;
  };
  nginx = templates.nginx;
  portVarHttp = slots.portVarName (config.portKeyHttp or "http");
  portVarHttps = slots.portVarName (config.portKeyHttps or "https");
  dataDirName = config.dataDirName or "nginx";
  nginxDirExpr = slots.getServiceDir dataDirName;
  emitHelper = observability.mkEmitServiceEventFunction "nginx";
  runtimePrelude = ''
    ${slotEnvRuntime.loadJsonFromCommand {
      outVar = "SLOT_INFO_JSON_OUT";
      command = toString slots.getSlotInfoJson;
      exportVars = false;
    }}

    HTTP_PORT_VAR="${portVarHttp}"
    HTTPS_PORT_VAR="${portVarHttps}"

    ${slotEnvRuntime.readPortFromJson {
      targetVar = "HTTP_PORT";
      jsonVar = "SLOT_INFO_JSON_OUT";
      keyExpr = "$HTTP_PORT_VAR";
    }}
    ${slotEnvRuntime.readPortFromJson {
      targetVar = "HTTPS_PORT";
      jsonVar = "SLOT_INFO_JSON_OUT";
      keyExpr = "$HTTPS_PORT_VAR";
    }}
    NGINX_DIR="${nginxDirExpr}"

    if [ -z "$HTTP_PORT" ] || [ -z "$HTTPS_PORT" ]; then
      log_error "nginx port variables are not set (http/https)"
      exit 1
    fi
  '';

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

  init = serviceScripts.mkWrappedScript {
    name = "nginx-init";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    body = ''
      mkdir -p "$NGINX_DIR/conf/sites-available"
      mkdir -p "$NGINX_DIR/conf/sites-enabled"
      mkdir -p "$NGINX_DIR/ssl/live/localhost"
      mkdir -p "$NGINX_DIR/logs"
      mkdir -p "$NGINX_DIR/html"
      mkdir -p "$NGINX_DIR/run"

      ${pkgs.gnused}/bin/sed \
        -e "s|NGINX_DIR|$NGINX_DIR|g" \
        -e "s|HTTP_PORT|$HTTP_PORT|g" \
        -e "s|HTTPS_PORT|$HTTPS_PORT|g" \
        "${templates.nginxConfTemplate}" > "$NGINX_DIR/conf/nginx.conf"

      ${generateSelfSignedCert} "localhost" "$NGINX_DIR/ssl"
    '';
  };

  start = serviceScripts.mkWrappedScript {
    name = "nginx-start";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    body = ''
      ${emitHelper}

      CONF="$NGINX_DIR/conf/nginx.conf"
      if [ ! -f "$CONF" ]; then
        log_error "Nginx not initialized. Run nginx-init first."
        exit 1
      fi

      emit_service_event service_starting starting --log-path "$NGINX_DIR/logs/error.log"

      ${nginx}/bin/nginx -c "$CONF" -g 'daemon off;'
    '';
  };

  stop = serviceScripts.mkWrappedScript {
    name = "nginx-stop";
    inherit
      loggingPrelude
      runtimePrelude
      ;
    body = ''
      ${emitHelper}
      PID_FILE="$NGINX_DIR/run/nginx.pid"

      if [ -f "$PID_FILE" ]; then
        PID=$(cat "$PID_FILE" 2>/dev/null || true)
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
          ${nginx}/bin/nginx -c "$NGINX_DIR/conf/nginx.conf" -s quit || true
          emit_service_event service_stopped stopped --pid "$PID" --log-path "$NGINX_DIR/logs/error.log"
        else
          rm -f "$PID_FILE"
          emit_service_event service_stopped stopped --log-path "$NGINX_DIR/logs/error.log"
        fi
      else
        emit_service_event service_stopped stopped --log-path "$NGINX_DIR/logs/error.log"
      fi
    '';
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
    init
    start
    stop
    reload
    generateSelfSignedCert
    listInstances
    ;
}
