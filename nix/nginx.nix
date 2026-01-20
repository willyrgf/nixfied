# Nginx module (optional)
{
  pkgs,
  project,
  slots,
}:

let
  cfg = project.modules.nginx or { };
  nginx = pkgs.nginx;
  portVarHttp = slots.portVarName (cfg.portKeyHttp or "http");
  portVarHttps = slots.portVarName (cfg.portKeyHttps or "https");
  dataDirName = cfg.dataDirName or "nginx";
  nginxDirExpr = slots.getServiceDir dataDirName;

  nginxConfTemplate = pkgs.writeText "nginx.conf.template" ''
    worker_processes auto;
    error_log NGINX_DIR/logs/error.log warn;
    pid NGINX_DIR/run/nginx.pid;

    events {
        worker_connections 1024;
        multi_accept on;
    }

    http {
        include ${nginx}/conf/mime.types;
        default_type application/octet-stream;

        access_log NGINX_DIR/logs/access.log;

        sendfile on;
        tcp_nopush on;
        tcp_nodelay on;
        keepalive_timeout 65;

        server_tokens off;

        server {
            listen HTTP_PORT default_server;
            listen HTTPS_PORT ssl default_server;
            http2 on;
            server_name _;

            ssl_certificate NGINX_DIR/ssl/live/localhost/fullchain.pem;
            ssl_certificate_key NGINX_DIR/ssl/live/localhost/privkey.pem;

            root NGINX_DIR/html;

            location / {
                return 444;
            }
        }

        include NGINX_DIR/conf/sites-enabled/*.conf;
    }
  '';

  siteProxyTemplate = pkgs.writeText "site-proxy.conf.template" ''
    server {
        listen HTTP_PORT;
        server_name SITE_DOMAIN;

        location / {
            return 301 https://$server_name$request_uri;
        }
    }

    server {
        listen HTTPS_PORT ssl;
        http2 on;
        server_name SITE_DOMAIN;

        ssl_certificate NGINX_DIR/ssl/live/SITE_DOMAIN/fullchain.pem;
        ssl_certificate_key NGINX_DIR/ssl/live/SITE_DOMAIN/privkey.pem;

        location / {
            proxy_pass http://UPSTREAM_HOST:UPSTREAM_PORT;
            proxy_http_version 1.1;
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection "upgrade";
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
        }
    }
  '';

  siteStaticTemplate = pkgs.writeText "site-static.conf.template" ''
    server {
        listen HTTP_PORT;
        server_name SITE_DOMAIN;

        location / {
            return 301 https://$server_name$request_uri;
        }
    }

    server {
        listen HTTPS_PORT ssl;
        http2 on;
        server_name SITE_DOMAIN;

        ssl_certificate NGINX_DIR/ssl/live/SITE_DOMAIN/fullchain.pem;
        ssl_certificate_key NGINX_DIR/ssl/live/SITE_DOMAIN/privkey.pem;

        root SITE_ROOT;
        index index.html index.htm;

        location / {
            try_files $uri $uri/ =404;
        }
    }
  '';

  generateSelfSignedCert = pkgs.writeShellScript "nginx-generate-self-signed" ''
    set -euo pipefail
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

  init = pkgs.writeShellScript "nginx-init" ''
    set -euo pipefail
    eval "$(${slots.getSlotInfo})"

    HTTP_PORT_VAR="${portVarHttp}"
    HTTPS_PORT_VAR="${portVarHttps}"
    HTTP_PORT="''${!HTTP_PORT_VAR}"
    HTTPS_PORT="''${!HTTPS_PORT_VAR}"

    NGINX_DIR="${nginxDirExpr}"

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
      "${nginxConfTemplate}" > "$NGINX_DIR/conf/nginx.conf"

    ${generateSelfSignedCert} "localhost" "$NGINX_DIR/ssl"
  '';

  start = pkgs.writeShellScript "nginx-start" ''
    set -euo pipefail
    eval "$(${slots.getSlotInfo})"
    NGINX_DIR="${nginxDirExpr}"

    CONF="$NGINX_DIR/conf/nginx.conf"
    if [ ! -f "$CONF" ]; then
      echo "❌ Nginx not initialized. Run nginx-init first." >&2
      exit 1
    fi

    ${nginx}/bin/nginx -c "$CONF" -g 'daemon off;'
  '';

  stop = pkgs.writeShellScript "nginx-stop" ''
    set -euo pipefail
    eval "$(${slots.getSlotInfo})"
    NGINX_DIR="${nginxDirExpr}"
    PID_FILE="$NGINX_DIR/run/nginx.pid"
    if [ -f "$PID_FILE" ]; then
      ${nginx}/bin/nginx -c "$NGINX_DIR/conf/nginx.conf" -s quit || true
    fi
  '';

  writeProxySite = pkgs.writeShellScript "nginx-site-proxy" ''
    set -euo pipefail
    if [ $# -lt 3 ]; then
      echo "Usage: nginx-site-proxy <domain> <upstream_host> <upstream_port>" >&2
      exit 1
    fi

    DOMAIN="$1"
    UPSTREAM_HOST="$2"
    UPSTREAM_PORT="$3"

    eval "$(${slots.getSlotInfo})"
    HTTP_PORT_VAR="${portVarHttp}"
    HTTPS_PORT_VAR="${portVarHttps}"
    HTTP_PORT="''${!HTTP_PORT_VAR}"
    HTTPS_PORT="''${!HTTPS_PORT_VAR}"

    NGINX_DIR="${nginxDirExpr}"
    CONF="$NGINX_DIR/conf/sites-available/$DOMAIN.conf"

    ${pkgs.gnused}/bin/sed \
      -e "s|NGINX_DIR|$NGINX_DIR|g" \
      -e "s|HTTP_PORT|$HTTP_PORT|g" \
      -e "s|HTTPS_PORT|$HTTPS_PORT|g" \
      -e "s|SITE_DOMAIN|$DOMAIN|g" \
      -e "s|UPSTREAM_HOST|$UPSTREAM_HOST|g" \
      -e "s|UPSTREAM_PORT|$UPSTREAM_PORT|g" \
      "${siteProxyTemplate}" > "$CONF"

    ln -sf "$CONF" "$NGINX_DIR/conf/sites-enabled/$DOMAIN.conf"
    ${generateSelfSignedCert} "$DOMAIN" "$NGINX_DIR/ssl"
  '';

  writeStaticSite = pkgs.writeShellScript "nginx-site-static" ''
    set -euo pipefail
    if [ $# -lt 2 ]; then
      echo "Usage: nginx-site-static <domain> <site_root>" >&2
      exit 1
    fi

    DOMAIN="$1"
    SITE_ROOT="$2"

    eval "$(${slots.getSlotInfo})"
    HTTP_PORT_VAR="${portVarHttp}"
    HTTPS_PORT_VAR="${portVarHttps}"
    HTTP_PORT="''${!HTTP_PORT_VAR}"
    HTTPS_PORT="''${!HTTPS_PORT_VAR}"

    NGINX_DIR="${nginxDirExpr}"
    CONF="$NGINX_DIR/conf/sites-available/$DOMAIN.conf"

    ${pkgs.gnused}/bin/sed \
      -e "s|NGINX_DIR|$NGINX_DIR|g" \
      -e "s|HTTP_PORT|$HTTP_PORT|g" \
      -e "s|HTTPS_PORT|$HTTPS_PORT|g" \
      -e "s|SITE_DOMAIN|$DOMAIN|g" \
      -e "s|SITE_ROOT|$SITE_ROOT|g" \
      "${siteStaticTemplate}" > "$CONF"

    ln -sf "$CONF" "$NGINX_DIR/conf/sites-enabled/$DOMAIN.conf"
    ${generateSelfSignedCert} "$DOMAIN" "$NGINX_DIR/ssl"
  '';
in
{
  inherit
    nginx
    init
    start
    stop
    writeProxySite
    writeStaticSite
    generateSelfSignedCert
    ;
}
