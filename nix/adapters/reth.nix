# Reth (Ethereum dev node) reference adapter.
#
# Compiles a dev-mode reth node into the generic model primitives. The runtime
# gains no Ethereum knowledge: start is a wrapper exec that derives the node's
# auxiliary ports and dev credentials, readiness and health are JSON-RPC
# protocol probes (`curl` posting `eth_blockNumber`), the smoke task is the
# same probe as a dependent task, and cleanup is the marker-gated runtime
# primitive that removes the slot state.
#
# Reth in `--dev` binds three TCP listeners (http, ws, authrpc) and no p2p socket
# (the dev chain is peerless, with discovery disabled). All three are modelled
# endpoints: the planner assigns each a port from the service's contiguous slot
# block, so every listener is reserved, conflict-checked against other
# services/slots, and ownership-verified after readiness. The wrapper receives the
# three planned ports as arguments and derives nothing.
#
# The data directory lives under `${stateDir}/reth`, so marker-gated cleanup
# removes it. The IPC socket lives under /tmp keyed by the http port: a deep
# state directory would exceed the Unix socket sun_path limit (notably on
# macOS), and a stale socket from an interrupted run is removed before start.
{ pkgs, ... }:
let
  rethNode = pkgs.writeShellApplication {
    name = "nixfied-reth";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.reth
    ];
    text = ''
      http_port=""
      ws_port=""
      auth_port=""
      state_dir=""
      host="127.0.0.1"

      while [[ $# -gt 0 ]]; do
        case "$1" in
          --http-port)
            http_port="''${2:?missing --http-port value}"
            shift 2
            ;;
          --ws-port)
            ws_port="''${2:?missing --ws-port value}"
            shift 2
            ;;
          --authrpc-port)
            auth_port="''${2:?missing --authrpc-port value}"
            shift 2
            ;;
          --state-dir)
            state_dir="''${2:?missing --state-dir value}"
            shift 2
            ;;
          --host)
            host="''${2:?missing --host value}"
            shift 2
            ;;
          *)
            echo "unknown reth argument: $1" >&2
            exit 64
            ;;
        esac
      done

      if [[ -z "$http_port" || -z "$ws_port" || -z "$auth_port" || -z "$state_dir" ]]; then
        echo "missing required --http-port/--ws-port/--authrpc-port/--state-dir argument" >&2
        exit 64
      fi

      reth_dir="$state_dir/reth"
      jwt_file="$reth_dir/config/jwt.hex"
      ipc_path="/tmp/nixfied-reth-$http_port.ipc"

      mkdir -p "$reth_dir/data" "$reth_dir/config"
      if [[ ! -s "$jwt_file" ]]; then
        printf '%064x\n' 0 > "$jwt_file"
      fi
      chmod 600 "$jwt_file" 2>/dev/null || true
      rm -f "$ipc_path"

      # `--dev` runs a peerless instant-seal chain, so reth binds no p2p TCP
      # listener; only the http, ws, and authrpc endpoints are modelled.
      exec reth node \
        --datadir "$reth_dir/data" \
        --ipcpath "$ipc_path" \
        --http \
        --http.addr "$host" \
        --http.port "$http_port" \
        --ws \
        --ws.addr "$host" \
        --ws.port "$ws_port" \
        --authrpc.addr "$host" \
        --authrpc.port "$auth_port" \
        --authrpc.jwtsecret "$jwt_file" \
        --dev
    '';
  };

  rpcProbeArgs = [
    "-sf"
    "-X"
    "POST"
    "-H"
    "Content-Type: application/json"
    "--data"
    ''{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}''
    "http://127.0.0.1:\${port}"
  ];
in
{
  nixfied.closures.reth-node = {
    package = rethNode;
    executable = "bin/nixfied-reth";
    operationBindings = [ "service.reth.start" ];
    effects = [
      "process"
      "network-listener"
      "file-write"
    ];
  };
  nixfied.closures.reth-rpc-probe = {
    package = pkgs.curl;
    executable = "bin/curl";
    operationBindings = [
      "service.reth.ready"
      "service.reth.health"
      "task.reth-smoke.run"
    ];
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.execs.reth-node = {
    closureId = "reth-node";
  };
  nixfied.execs.reth-rpc = {
    closureId = "reth-rpc-probe";
  };

  nixfied.services.reth = {
    lifecycle = {
      prepare = {
        operationId = "service.reth.prepare";
        # The start wrapper is self-preparing (datadir, jwt, stale IPC socket).
        terminal = {
          success = "prepared";
          failure = "failed";
        };
      };
      start = {
        operationId = "service.reth.start";
        execId = "reth-node";
        execArgs = [
          "--http-port"
          "\${port:reth-http}"
          "--ws-port"
          "\${port:reth-ws}"
          "--authrpc-port"
          "\${port:reth-authrpc}"
          "--state-dir"
          "\${stateDir}"
        ];
        terminal = {
          success = "spawned";
          failure = "failed";
        };
      };
      ready = {
        operationId = "service.reth.ready";
        # Protocol readiness: an answered eth_blockNumber call, not a bound
        # port — reth listens well before the RPC layer serves requests.
        probe = {
          kind = "exec";
          execId = "reth-rpc";
          execArgs = rpcProbeArgs;
          timeoutMs = 2000;
          retryIntervalMs = 500;
          maxAttempts = 120;
        };
        terminal = {
          success = "ready";
          failure = "not-ready";
        };
      };
      health = {
        operationId = "service.reth.health";
        probe = {
          kind = "exec";
          execId = "reth-rpc";
          execArgs = rpcProbeArgs;
          timeoutMs = 2000;
          retryIntervalMs = 500;
          maxAttempts = 120;
        };
        terminal = {
          success = "healthy";
          failure = "unhealthy";
        };
      };
      stop = {
        operationId = "service.reth.stop";
        signal = "TERM";
        timeoutMs = 10000;
        terminal = {
          success = "stopped";
          failure = "failed";
        };
      };
      clean = {
        operationId = "service.reth.clean";
        terminal = {
          success = "cleaned";
          failure = "failed";
        };
      };
    };
    endpoints = {
      reth-http = { };
      reth-ws = { };
      reth-authrpc = { };
    };
    primaryEndpoint = "reth-http";
    stateRefs = [ "slot" ];
    logRefs = [ "service.reth" ];
    containment = "process-tree";
  };

  nixfied.tasks.reth-smoke = {
    operationId = "task.reth-smoke.run";
    execId = "reth-rpc";
    args = rpcProbeArgs;
    dependsOnServicesReady = [ "reth" ];
    logRefs = [ "task.reth-smoke" ];
    summaryRefs = [ "summary" ];
  };

  nixfied.environments.dev = {
    services = [ "reth" ];
    tasks = [ "reth-smoke" ];
  };
}
