# Test fixture: enable optional modules
{
  pkgs ? null,
}:

rec {
  project = {
    name = "Nixfied Test Project";
    id = "nixfied-test-framework";
    description = "Framework test fixture";
    envVar = "PROJECT_ENV";
    slotVar = "NIX_ENV";
  };

  envs = {
    prod = {
      offset = 0;
    };
    dev = {
      offset = 10;
    };
    test = {
      offset = 20;
    };
  };

  ports = {
    backend = 3000;
    frontend = 3100;
    http = 8080;
    https = 8443;
    postgres = 5432;
    minioApi = 9000;
    minioConsole = 9001;
    rethHttp = 8545;
    rethWs = 8546;
    rethAuth = 8551;
    heliosRpc = 8547;
  };

  directories = {
    base = "\${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}";
  };

  tooling = {
    runtimePackages = [
      pkgs.coreutils
      pkgs.gnused
      pkgs.gnugrep
      pkgs.netcat
      pkgs.python3
    ];
    devShellPackages = [ ];
    devShellHook = "";
  };

  install = {
    deps = "";
  };

  supervisor = {
    enable = true;
    services = { };
  };

  modules = {
    postgres = {
      enable = true;
      database = "app";
      testDatabase = "app_test";
      extensions = [ ];
      package = if pkgs != null then pkgs.postgresql_16 else null;
      portKey = "postgres";
      dataDirName = "postgres";
      extraConfig = "";
    };
    nginx = {
      enable = true;
      portKeyHttp = "http";
      portKeyHttps = "https";
      dataDirName = "nginx";
    };
    minio = {
      enable = true;
      package = if pkgs != null then pkgs.minio else null;
      clientPackage = if pkgs != null then pkgs.minio-client else null;
      portKeyApi = "minioApi";
      portKeyConsole = "minioConsole";
      dataDirName = "minio";
      rootUser = "minioadmin";
      rootPassword = "minioadmin";
      browser = true;
    };
    reth = {
      enable = true;
      package =
        if pkgs != null then
          pkgs.writeShellScriptBin "reth" ''
            set -euo pipefail

            for arg in "$@"; do
              case "$arg" in
                --help|-h)
                  echo "reth-mock help"
                  exit 0
                  ;;
                --version|-V)
                  echo "reth-mock 1.0.0"
                  exit 0
                  ;;
              esac
            done

            PORT="8545"
            while [ "$#" -gt 0 ]; do
              case "$1" in
                            --http.port)
                              PORT="''${2:-$PORT}"
                              shift 2
                              ;;
                            --http.port=*)
                              PORT="''${1#*=}"
                              shift
                              ;;
                            *)
                              shift
                              ;;
                          esac
                        done

                        exec ${pkgs.python3}/bin/python3 - "$PORT" <<'PY'
            import http.server
            import json
            import socketserver
            import sys

            port = int(sys.argv[1])

            class Handler(http.server.BaseHTTPRequestHandler):
                def do_POST(self):
                    length = int(self.headers.get("Content-Length", "0"))
                    _ = self.rfile.read(length)
                    body = json.dumps({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": "reth-mock/1.0.0"
                    }).encode("utf-8")
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

                def log_message(self, _fmt, *_args):
                    return

            socketserver.TCPServer.allow_reuse_address = True
            with socketserver.TCPServer(("127.0.0.1", port), Handler) as srv:
                srv.serve_forever()
            PY
          ''
        else
          null;
      portKeyHttp = "rethHttp";
      portKeyWs = "rethWs";
      portKeyAuth = "rethAuth";
      dataDirName = "reth";
      network = "local";
      devMode = true;
      extraArgs = [ ];
    };
    helios = {
      enable = true;
      package =
        if pkgs != null then
          pkgs.writeShellScriptBin "helios" ''
            set -euo pipefail

            if [ "$#" -eq 0 ]; then
              echo "Usage: helios [OPTIONS] <COMMAND>" >&2
              exit 2
            fi

            case "$1" in
              --help|-h)
                echo "helios-mock"
                echo ""
                echo "Usage: helios [OPTIONS] <COMMAND>"
                echo ""
                echo "Commands:"
                echo "  ethereum"
                echo "  help"
                echo ""
                echo "Options:"
                echo "  -h, --help"
                echo "  -V, --version"
                exit 0
                ;;
              --version|-V)
                echo "helios-mock 1.0.0"
                exit 0
                ;;
              ethereum)
                shift
                if [ "$#" -gt 0 ] && { [ "$1" = "--help" ] || [ "$1" = "-h" ]; }; then
                  echo "Usage: helios ethereum [OPTIONS]"
                  echo ""
                  echo "Options:"
                  echo "  --network <NETWORK>"
                  echo "  --rpc-bind-ip <RPC_BIND_IP>"
                  echo "  --rpc-port <RPC_PORT>"
                  echo "  --data-dir <DATA_DIR>"
                  echo "  --execution-rpc <EXECUTION_RPC>"
                  echo "  --consensus-rpc <CONSENSUS_RPC>"
                  echo "  --checkpoint <CHECKPOINT>"
                  exit 0
                fi
                ;;
              *)
                if [ "''${1#-}" != "$1" ]; then
                  echo "error: unexpected argument '$1' found" >&2
                  echo "" >&2
                  echo "Usage: helios [OPTIONS] <COMMAND>" >&2
                  exit 2
                fi
                echo "error: unknown command '$1'" >&2
                exit 2
                ;;
            esac

            PORT="8547"
            while [ "$#" -gt 0 ]; do
              case "$1" in
                --rpc-port)
                  PORT="''${2:-$PORT}"
                  shift 2
                  ;;
                --rpc-port=*)
                  PORT="''${1#*=}"
                  shift
                  ;;
                *)
                  shift
                  ;;
              esac
            done

            exec ${pkgs.python3}/bin/python3 - "$PORT" <<'PY'
            import http.server
            import json
            import socketserver
            import sys

            port = int(sys.argv[1])

            class Handler(http.server.BaseHTTPRequestHandler):
                def do_POST(self):
                    length = int(self.headers.get("Content-Length", "0"))
                    _ = self.rfile.read(length)
                    body = json.dumps({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": "helios-mock/1.0.0"
                    }).encode("utf-8")
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

                def log_message(self, _fmt, *_args):
                    return

            socketserver.TCPServer.allow_reuse_address = True
            with socketserver.TCPServer(("127.0.0.1", port), Handler) as srv:
                srv.serve_forever()
            PY
          ''
        else
          null;
      portKeyRpc = "heliosRpc";
      dataDirName = "helios";
      network = "local";
      executionRpcPortKey = "rethHttp";
      executionRpcUrl = "";
      consensusRpcUrl = "";
      checkpoint = "";
      extraArgs = [ ];
    };
  };

  packages = { };
}
