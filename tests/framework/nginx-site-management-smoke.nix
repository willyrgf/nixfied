{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  slotsStub =
    let
      slotInfo = pkgs.writeShellScript "nginx-site-slot-info" ''
        printf 'SLOT=%s\n' "''${SLOT:-0}"
        printf 'ENV=%s\n' "''${ENV:-dev}"
        printf 'HTTP_PORT=%s\n' "''${HTTP_PORT:-8080}"
        printf 'HTTPS_PORT=%s\n' "''${HTTPS_PORT:-8443}"
      '';
    in
    {
      getSlotInfo = slotInfo;
      getServiceDir = name: "\${NGINX_SERVICE_ROOT}/${name}";
      portVarName =
        key:
        if key == "http" then
          "HTTP_PORT"
        else if key == "https" then
          "HTTPS_PORT"
        else
          throw "unsupported port key ${key}";
    };

  lifecycleStub = {
    generateSelfSignedCert = pkgs.writeShellScript "nginx-generate-self-signed-cert" ''
      set -euo pipefail
      domain="$1"
      ssl_root="$2"
      mkdir -p "$ssl_root/live/$domain"
      : > "$ssl_root/live/$domain/fullchain.pem"
      : > "$ssl_root/live/$domain/privkey.pem"
    '';
  };

  siteMgmt = import ../../nixfied/modules/services/runtime/nginx/site-management.nix {
    inherit pkgs;
    project = { };
    slots = slotsStub;
    config = {
      portKeyHttp = "http";
      portKeyHttps = "https";
      dataDirName = "nginx";
    };
    templates = { };
    lifecycle = lifecycleStub;
    inherit (shellHelpers) loggingPrelude;
  };
in
pkgs.runCommand "nginx-site-management-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  PROXY_BIN="${siteMgmt.writeProxySite}"
  STATIC_BIN="${siteMgmt.writeStaticSite}"

  export SLOT=0
  export ENV=dev
  export HTTP_PORT=8080
  export HTTPS_PORT=8443
  export NGINX_SERVICE_ROOT="$TMPDIR/services"

  require_symlink() {
    local path="$1"
    [ -L "$path" ] || fail "missing symlink: $path"
  }

  mkdir -p "$NGINX_SERVICE_ROOT/nginx/conf/sites-available" "$NGINX_SERVICE_ROOT/nginx/conf/sites-enabled"

  "$PROXY_BIN" example.com localhost 3000
  proxy_conf="$NGINX_SERVICE_ROOT/nginx/conf/sites-available/example.com.conf"
  require_file "$proxy_conf"
  require_symlink "$NGINX_SERVICE_ROOT/nginx/conf/sites-enabled/example.com.conf"
  require_file "$NGINX_SERVICE_ROOT/nginx/ssl/live/example.com/fullchain.pem"
  require_contains "$proxy_conf" "listen 8080;"
  require_contains "$proxy_conf" "listen 8443 ssl;"
  require_contains "$proxy_conf" "server_name example.com;"
  require_contains "$proxy_conf" "proxy_pass http://localhost:3000;"
  require_contains "$proxy_conf" 'return 301 https://$server_name$request_uri;'
  require_contains "$proxy_conf" 'proxy_set_header Host $host;'

  "$STATIC_BIN" static.example.com /srv/www/site
  static_conf="$NGINX_SERVICE_ROOT/nginx/conf/sites-available/static.example.com.conf"
  require_file "$static_conf"
  require_symlink "$NGINX_SERVICE_ROOT/nginx/conf/sites-enabled/static.example.com.conf"
  require_contains "$static_conf" "root /srv/www/site;"
  require_contains "$static_conf" 'try_files $uri $uri/ =404;'

  set +e
  "$PROXY_BIN" bad..example localhost 3000 > "$TMPDIR/invalid-site.out" 2>&1
  invalid_rc="$?"
  set -e
  if [ "$invalid_rc" -eq 0 ]; then
    cat "$TMPDIR/invalid-site.out"
    fail "invalid domain should fail"
  fi
  require_contains "$TMPDIR/invalid-site.out" "ERROR: invalid domain value=bad..example"

  echo "OK: nginx site management uses structured writers and preserves config shape" > "$out"
''
