{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  slotsStub =
    let
      slotInfoJson = pkgs.writeShellScript "service-lifecycle-slot-info-json" ''
        printf '{"slot":"%s","env":"%s","ports":{"POSTGRES_PORT":%s,"HTTP_PORT":%s,"HTTPS_PORT":%s,"MINIO_API_PORT":%s,"MINIO_CONSOLE_PORT":%s,"RETH_HTTP_PORT":%s,"RETH_WS_PORT":%s,"RETH_AUTH_PORT":%s,"HELIOSRPC_PORT":%s},"directories":{"run":"%s","log":"%s","config":"%s"}}\n' \
          "''${SLOT:-0}" \
          "''${ENV:-test}" \
          "''${POSTGRES_PORT:-55433}" \
          "''${HTTP_PORT:-28080}" \
          "''${HTTPS_PORT:-28443}" \
          "''${MINIO_API_PORT:-29000}" \
          "''${MINIO_CONSOLE_PORT:-29001}" \
          "''${RETH_HTTP_PORT:-29100}" \
          "''${RETH_WS_PORT:-29101}" \
          "''${RETH_AUTH_PORT:-29102}" \
          "''${HELIOSRPC_PORT:-29200}" \
          "''${RUN_DIR:-/tmp}" \
          "''${LOG_DIR:-/tmp}" \
          "''${CONFIG_DIR:-/tmp}"
      '';
    in
    {
      getSlotInfoJson = slotInfoJson;
      getServiceDir = name: "\${SERVICE_ROOT}/${name}";
      portVarName =
        key:
        if key == "postgres" then
          "POSTGRES_PORT"
        else if key == "http" then
          "HTTP_PORT"
        else if key == "https" then
          "HTTPS_PORT"
        else if key == "minioApi" then
          "MINIO_API_PORT"
        else if key == "minioConsole" then
          "MINIO_CONSOLE_PORT"
        else if key == "rethHttp" then
          "RETH_HTTP_PORT"
        else if key == "rethWs" then
          "RETH_WS_PORT"
        else if key == "rethAuth" then
          "RETH_AUTH_PORT"
        else if key == "heliosRpc" then
          "HELIOSRPC_PORT"
        else
          throw "unsupported port key ${key}";
    };

  projectBase = {
    project = {
      id = "service-lifecycle-matrix";
    };
  };

  mkPackageWithScript =
    {
      name,
      binName,
      script,
      extraSetup ? "",
    }:
    pkgs.runCommand name { } ''
      mkdir -p "$out/bin"
      cp ${script} "$out/bin/${binName}"
      chmod +x "$out/bin/${binName}"
      ${extraSetup}
    '';

  nginxStubScript = pkgs.writeShellScript "nginx-stub" ''
        set -euo pipefail

        CONF=""
        SIGNAL=""
        TEST_ONLY=0
        while [ "$#" -gt 0 ]; do
          case "$1" in
            -c)
              CONF="$2"
              shift 2
              ;;
            -t)
              TEST_ONLY=1
              shift
              ;;
            -g)
              shift 2
              ;;
            -s)
              SIGNAL="$2"
              shift 2
              ;;
            *)
              shift
              ;;
          esac
        done

        if [ -z "$CONF" ]; then
          echo "missing nginx config" >&2
          exit 1
        fi

        PID_FILE="$(${pkgs.gawk}/bin/awk '/^[[:space:]]*pid[[:space:]]+/ { gsub(/;/, "", $2); print $2; exit }' "$CONF")"
        LISTEN_PORTS="$(${pkgs.gawk}/bin/awk '
          /^[[:space:]]*listen[[:space:]]+[0-9]+/ {
            if ($2 ~ /^[0-9]+;?$/) {
              gsub(/;/, "", $2)
              print $2
            }
          }
        ' "$CONF")"

        if [ -n "$SIGNAL" ]; then
          if [ -n "$PID_FILE" ] && [ -f "$PID_FILE" ]; then
            kill "$(cat "$PID_FILE")" 2>/dev/null || true
          fi
          exit 0
        fi

        if [ "$TEST_ONLY" -eq 1 ]; then
          [ -f "$CONF" ] || exit 1
          echo "nginx: configuration file $CONF test is successful"
          exit 0
        fi

        mkdir -p "$(dirname "$PID_FILE")"
        echo "$$" > "$PID_FILE"

        if [ -z "$LISTEN_PORTS" ]; then
          echo "missing nginx listen port" >&2
          exit 1
        fi

        exec ${pkgs.python3}/bin/python3 - $LISTEN_PORTS <<'PY'
    import http.server
    import socket
    import socketserver
    import threading
    import sys

    PORTS = [int(arg) for arg in sys.argv[1:]]

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")

        def log_message(self, format, *args):
            return

    class ReusableTCPServer(socketserver.TCPServer):
        allow_reuse_address = True

    servers = []

    try:
        for port in PORTS:
            httpd = ReusableTCPServer(("127.0.0.1", port), Handler)
            thread = threading.Thread(target=httpd.serve_forever, daemon=True)
            thread.start()
            servers.append((httpd, thread))

        threading.Event().wait()
    finally:
        for httpd, thread in servers:
            httpd.shutdown()
            httpd.server_close()
    PY
  '';

  nginxStub = mkPackageWithScript {
    name = "nginx-lifecycle-stub";
    binName = "nginx";
    script = nginxStubScript;
    extraSetup = ''
            mkdir -p "$out/conf"
            cat > "$out/conf/mime.types" <<'EOF'
      types {
        text/plain txt;
      }
      EOF
    '';
  };

  minioStubScript = pkgs.writeShellScript "minio-stub" ''
        set -euo pipefail

        if [ "''${1:-}" = "--help" ] || [ "$#" -eq 0 ]; then
          echo "minio stub"
          exit 0
        fi

        if [ "$1" != "server" ]; then
          echo "unsupported minio command: $1" >&2
          exit 1
        fi

        API_PORT=""
        CONSOLE_PORT=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --address)
              API_PORT="$(${pkgs.coreutils}/bin/cut -d: -f2 <<<"$2")"
              shift 2
              ;;
            --console-address)
              CONSOLE_PORT="$(${pkgs.coreutils}/bin/cut -d: -f2 <<<"$2")"
              shift 2
              ;;
            --config-dir)
              shift 2
              ;;
            *)
              shift
              ;;
          esac
        done

        exec ${pkgs.python3}/bin/python3 - "$API_PORT" "$CONSOLE_PORT" <<'PY'
    import http.server
    import socketserver
    import threading
    import sys

    PORTS = [int(arg) for arg in sys.argv[1:] if arg]

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path in ("/minio/health/live", "/minio/health/ready"):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"ok")
                return
            self.send_response(404)
            self.end_headers()

        def log_message(self, format, *args):
            return

    class ReusableTCPServer(socketserver.TCPServer):
        allow_reuse_address = True

    servers = []

    try:
        for port in PORTS:
            httpd = ReusableTCPServer(("127.0.0.1", port), Handler)
            thread = threading.Thread(target=httpd.serve_forever, daemon=True)
            thread.start()
            servers.append((httpd, thread))

        threading.Event().wait()
    finally:
        for httpd, thread in servers:
            httpd.shutdown()
            httpd.server_close()
    PY
  '';

  minioStub = mkPackageWithScript {
    name = "minio-lifecycle-stub";
    binName = "minio";
    script = minioStubScript;
  };

  rethStubScript = pkgs.writeShellScript "reth-stub" ''
        set -euo pipefail

        if [ "''${1:-}" = "--version" ]; then
          echo "reth-stub 0.0.0"
          exit 0
        fi

        HTTP_PORT=""
        WS_PORT=""
        AUTH_PORT=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --http.port)
              HTTP_PORT="$2"
              shift 2
              ;;
            --ws.port)
              WS_PORT="$2"
              shift 2
              ;;
            --authrpc.port)
              AUTH_PORT="$2"
              shift 2
              ;;
            *)
              shift
              ;;
          esac
        done

        exec ${pkgs.python3}/bin/python3 - "$HTTP_PORT" "$WS_PORT" "$AUTH_PORT" <<'PY'
    import http.server
    import json
    import socketserver
    import threading
    import sys

    PORTS = [int(arg) for arg in sys.argv[1:] if arg]

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            payload = json.loads(self.rfile.read(length) or b"{}")
            method = payload.get("method")
            if method == "web3_clientVersion":
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "result": "reth-stub"}
            elif method == "eth_chainId":
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "result": "0x1"}
            else:
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "error": {"code": -32601, "message": "method not found"}}
            body = json.dumps(response).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format, *args):
            return

    class ReusableTCPServer(socketserver.TCPServer):
        allow_reuse_address = True

    servers = []

    try:
        for port in PORTS:
            httpd = ReusableTCPServer(("127.0.0.1", port), Handler)
            thread = threading.Thread(target=httpd.serve_forever, daemon=True)
            thread.start()
            servers.append((httpd, thread))

        threading.Event().wait()
    finally:
        for httpd, thread in servers:
            httpd.shutdown()
            httpd.server_close()
    PY
  '';

  rethStub = mkPackageWithScript {
    name = "reth-lifecycle-stub";
    binName = "reth";
    script = rethStubScript;
  };

  heliosStubScript = pkgs.writeShellScript "helios-stub" ''
        set -euo pipefail

        if [ "''${1:-}" = "ethereum" ] && [ "''${2:-}" = "--help" ]; then
          echo "helios stub"
          exit 0
        fi

        RPC_PORT=""
        EXECUTION_RPC_URL=""
        while [ "$#" -gt 0 ]; do
          case "$1" in
            --rpc-port)
              RPC_PORT="$2"
              shift 2
              ;;
            --execution-rpc)
              EXECUTION_RPC_URL="$2"
              shift 2
              ;;
            *)
              shift
              ;;
          esac
        done

        EXECUTION_PORT=""
        case "$EXECUTION_RPC_URL" in
          http://127.0.0.1:*)
            EXECUTION_PORT="$(${pkgs.coreutils}/bin/printf '%s' "$EXECUTION_RPC_URL" | ${pkgs.gnused}/bin/sed -E 's#^http://127\.0\.0\.1:([0-9]+).*$#\1#')"
            ;;
        esac

        exec ${pkgs.python3}/bin/python3 - "$RPC_PORT" "$EXECUTION_PORT" <<'PY'
    import http.server
    import json
    import socketserver
    import threading
    import sys

    PORTS = [int(arg) for arg in sys.argv[1:] if arg]

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            payload = json.loads(self.rfile.read(length) or b"{}")
            method = payload.get("method")
            if method == "eth_chainId":
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "result": "0x539"}
            elif method == "eth_blockNumber":
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "result": "0x2a"}
            elif method == "eth_syncing":
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "result": False}
            elif method == "web3_clientVersion":
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "result": "helios-stub"}
            else:
                response = {"jsonrpc": "2.0", "id": payload.get("id"), "error": {"code": -32601, "message": "method not found"}}
            body = json.dumps(response).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format, *args):
            return

    class ReusableTCPServer(socketserver.TCPServer):
        allow_reuse_address = True

    servers = []

    try:
        for port in PORTS:
            httpd = ReusableTCPServer(("127.0.0.1", port), Handler)
            thread = threading.Thread(target=httpd.serve_forever, daemon=True)
            thread.start()
            servers.append((httpd, thread))

        threading.Event().wait()
    finally:
        for httpd, thread in servers:
            httpd.shutdown()
            httpd.server_close()
    PY
  '';

  heliosStub = mkPackageWithScript {
    name = "helios-lifecycle-stub";
    binName = "helios";
    script = heliosStubScript;
  };

  postgresService = import ../../nixfied/framework/runtime/services/postgres/default.nix {
    inherit pkgs;
    project = projectBase;
    slots = slotsStub;
  };

  nginxService = import ../../nixfied/framework/runtime/services/nginx/default.nix {
    inherit pkgs;
    project = projectBase // {
      services.nginx = {
        defaultSource = "stub";
        sources = {
          stub.package = nginxStub;
        };
      };
    };
    slots = slotsStub;
  };

  minioService = import ../../nixfied/framework/runtime/services/minio/default.nix {
    inherit pkgs;
    project = projectBase // {
      services.minio = {
        defaultSource = "stub";
        sources = {
          stub.package = minioStub;
        };
      };
    };
    slots = slotsStub;
  };

  rethService = import ../../nixfied/framework/runtime/services/reth/default.nix {
    inherit pkgs;
    project = projectBase // {
      services.reth = {
        defaultSource = "stub";
        sources = {
          stub.package = rethStub;
        };
      };
    };
    slots = slotsStub;
  };

  heliosService = import ../../nixfied/framework/runtime/services/helios/default.nix {
    inherit pkgs;
    project = projectBase // {
      services.helios = {
        defaultSource = "stub";
        sources = {
          stub.package = heliosStub;
        };
      };
    };
    slots = slotsStub;
  };
in
pkgs.runCommand "service-lifecycle-matrix-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.stderrShellPrelude}
  ${shellHelpers.postgresCapabilityPrelude}

  POSTGRES_LIVE_AVAILABLE=1
  if ! postgres_bootstrap_probe; then
    POSTGRES_LIVE_AVAILABLE=0
    echo "SKIP: postgres lifecycle case requires PostgreSQL bootstrap support in the build sandbox" >&2
  fi

  export HOME="$TMPDIR/home"
  export SLOT=0
  export ENV=test
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$HOME" "$REGISTRY_ROOT"

  wait_for_success() {
    local label="$1"
    local cmd="$2"
    local log_file="$3"
    local attempts="''${4:-80}"
    local interval="''${5:-0.25}"
    local rc=0

    for _ in $(seq 1 "$attempts"); do
      set +e
      "$cmd" > "$log_file" 2>&1
      rc="$?"
      set -e
      if [ "$rc" -eq 0 ]; then
        return 0
      fi
      sleep "$interval"
    done

    cat "$log_file" >&2
    echo "$label did not succeed" >&2
    return 1
  }

  require_failure() {
    local label="$1"
    local cmd="$2"
    local log_file="$3"
    set +e
    "$cmd" > "$log_file" 2>&1
    local rc="$?"
    set -e
    if [ "$rc" -eq 0 ]; then
      cat "$log_file" >&2
      fail "$label should fail"
    fi
  }

  wait_for_failure() {
    local label="$1"
    local cmd="$2"
    local log_file="$3"
    local attempts="''${4:-80}"
    local interval="''${5:-0.25}"
    local rc=0

    for _ in $(seq 1 "$attempts"); do
      set +e
      "$cmd" > "$log_file" 2>&1
      rc="$?"
      set -e
      if [ "$rc" -ne 0 ]; then
        return 0
      fi
      sleep "$interval"
    done

    cat "$log_file" >&2
    echo "$label did not fail" >&2
    return 1
  }

  wait_for_background_exit() {
    local pid="$1"
    if [ -n "$pid" ]; then
      for _ in $(seq 1 40); do
        if ! kill -0 "$pid" 2>/dev/null; then
          set +e
          wait "$pid" >/dev/null 2>&1
          set -e
          return 0
        fi
        sleep 0.25
      done

      set +e
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
      set -e
    fi
  }

  run_service_case() {
    local service_name="$1"
    local init_bin="$2"
    local check_bin="$3"
    local start_bin="$4"
    local status_bin="$5"
    local health_bin="$6"
    local ready_bin="$7"
    local restart_bin="$8"
    local stop_bin="$9"

    SERVICE_ROOT="$TMPDIR/$service_name-root"
    RUN_DIR="$TMPDIR/$service_name-run"
    LOG_DIR="$TMPDIR/$service_name-log"
    CONFIG_DIR="$TMPDIR/$service_name-config"
    export SERVICE_ROOT RUN_DIR LOG_DIR CONFIG_DIR
    mkdir -p "$SERVICE_ROOT" "$RUN_DIR" "$LOG_DIR" "$CONFIG_DIR"

    dump_service_debug() {
      local service="$1"
      local pg_log="$SERVICE_ROOT/$service/postgres.log"

      if [ -f "$TMPDIR/$service-start.out" ]; then
        echo "--- $service-start.out" >&2
        cat "$TMPDIR/$service-start.out" >&2
      fi

      if [ -f "$pg_log" ]; then
        echo "--- $pg_log" >&2
        cat "$pg_log" >&2
      fi
    }

    echo "INFO: lifecycle-smoke service=$service_name phase=init" >&2
    "$init_bin" > "$TMPDIR/$service_name-init.out" 2>&1 || {
      cat "$TMPDIR/$service_name-init.out" >&2
      fail "$service_name init should succeed"
    }

    echo "INFO: lifecycle-smoke service=$service_name phase=check-config" >&2
    "$check_bin" > "$TMPDIR/$service_name-check.out" 2>&1 || {
      cat "$TMPDIR/$service_name-check.out" >&2
      fail "$service_name check-config should succeed"
    }

    echo "INFO: lifecycle-smoke service=$service_name phase=start" >&2
    "$start_bin" > "$TMPDIR/$service_name-start.out" 2>&1 &
    START_WRAPPER_PID=$!

    echo "INFO: lifecycle-smoke service=$service_name phase=health-after-start" >&2
    if ! wait_for_success "$service_name health after start" "$health_bin" "$TMPDIR/$service_name-health.out"; then
      dump_service_debug "$service_name"
      exit 1
    fi
    echo "INFO: lifecycle-smoke service=$service_name phase=ready-after-start" >&2
    wait_for_success "$service_name ready after start" "$ready_bin" "$TMPDIR/$service_name-ready.out"
    echo "INFO: lifecycle-smoke service=$service_name phase=status-after-start" >&2
    "$status_bin" > "$TMPDIR/$service_name-status.out" 2>&1 || {
      cat "$TMPDIR/$service_name-status.out" >&2
      fail "$service_name status should report running state"
    }
    require_contains "$TMPDIR/$service_name-status.out" "service=$service_name"
    require_contains "$TMPDIR/$service_name-status.out" "running=true"

    echo "INFO: lifecycle-smoke service=$service_name phase=restart" >&2
    "$restart_bin" > "$TMPDIR/$service_name-restart.out" 2>&1 &
    RESTART_WRAPPER_PID=$!

    echo "INFO: lifecycle-smoke service=$service_name phase=health-after-restart" >&2
    wait_for_success "$service_name health after restart" "$health_bin" "$TMPDIR/$service_name-health-restart.out"
    echo "INFO: lifecycle-smoke service=$service_name phase=ready-after-restart" >&2
    wait_for_success "$service_name ready after restart" "$ready_bin" "$TMPDIR/$service_name-ready-restart.out"

    echo "INFO: lifecycle-smoke service=$service_name phase=stop" >&2
    "$stop_bin" > "$TMPDIR/$service_name-stop.out" 2>&1 || {
      cat "$TMPDIR/$service_name-stop.out" >&2
      fail "$service_name stop should succeed"
    }

    echo "INFO: lifecycle-smoke service=$service_name phase=wait-background-exit" >&2
    wait_for_background_exit "$START_WRAPPER_PID"
    wait_for_background_exit "$RESTART_WRAPPER_PID"

    echo "INFO: lifecycle-smoke service=$service_name phase=post-stop-verification" >&2
    wait_for_failure "$service_name health after stop" "$health_bin" "$TMPDIR/$service_name-health-stopped.out"
    wait_for_failure "$service_name ready after stop" "$ready_bin" "$TMPDIR/$service_name-ready-stopped.out"
    wait_for_failure "$service_name status after stop" "$status_bin" "$TMPDIR/$service_name-status-stopped.out"
    require_contains "$TMPDIR/$service_name-status-stopped.out" "running=false"
    echo "INFO: lifecycle-smoke service=$service_name phase=done" >&2
  }

  if [ "$POSTGRES_LIVE_AVAILABLE" -eq 1 ]; then
    export POSTGRES_PORT=55433
    run_service_case \
      postgres \
      "${postgresService.init}" \
      "${postgresService.checkConfig}" \
      "${postgresService.start}" \
      "${postgresService.status}" \
      "${postgresService.health}" \
      "${postgresService.ready}" \
      "${postgresService.restart}" \
      "${postgresService.stop}"
  fi

  export HTTP_PORT=28080 HTTPS_PORT=28443
  run_service_case \
    nginx \
    "${nginxService.init}" \
    "${nginxService.checkConfig}" \
    "${nginxService.start}" \
    "${nginxService.status}" \
    "${nginxService.health}" \
    "${nginxService.ready}" \
    "${nginxService.restart}" \
    "${nginxService.stop}"

  export MINIO_API_PORT=29000 MINIO_CONSOLE_PORT=29001
  run_service_case \
    minio \
    "${minioService.init}" \
    "${minioService.checkConfig}" \
    "${minioService.start}" \
    "${minioService.status}" \
    "${minioService.health}" \
    "${minioService.ready}" \
    "${minioService.restart}" \
    "${minioService.stop}"

  export RETH_HTTP_PORT=29100 RETH_WS_PORT=29101 RETH_AUTH_PORT=29102
  run_service_case \
    reth \
    "${rethService.init}" \
    "${rethService.checkConfig}" \
    "${rethService.start}" \
    "${rethService.status}" \
    "${rethService.health}" \
    "${rethService.ready}" \
    "${rethService.restart}" \
    "${rethService.stop}"

  export HELIOSRPC_PORT=29200 RETH_HTTP_PORT=29210
  export HELIOS_READY_TIMEOUT_SECS=3 HELIOS_READY_INTERVAL_SECS=0.2
  run_service_case \
    helios \
    "${heliosService.init}" \
    "${heliosService.checkConfig}" \
    "${heliosService.start}" \
    "${heliosService.status}" \
    "${heliosService.health}" \
    "${heliosService.ready}" \
    "${heliosService.restart}" \
    "${heliosService.stop}"

  echo "OK: public service modules cover direct start stop restart health and ready behavior" > "$out"
''
