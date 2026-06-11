# Reth (Ethereum dev node) reference adapter.
#
# Compiles a dev-mode reth node into the generic model primitives. The runtime
# gains no Ethereum knowledge: start is a wrapper exec that derives the node's
# auxiliary ports and dev credentials, readiness and health are JSON-RPC
# protocol probes (`curl` posting `eth_blockNumber`), the smoke task is the
# same probe as a dependent task, and cleanup is the marker-gated runtime
# primitive that removes the slot state.
#
# Port headroom caveat: reth needs four listeners (http, ws, authrpc, p2p).
# The model assigns only the http port; the wrapper derives the others as
# +1/+2/+3, which the planner does NOT reserve. Give a reth project a dedicated
# `nixfied.placement.ports.base` so the derived ports cannot collide with other
# services in the same window; folding them into the plan needs model-level
# multi-endpoint support, which is out of scope for this adapter.
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
      state_dir=""
      host="127.0.0.1"

      while [[ $# -gt 0 ]]; do
        case "$1" in
          --http-port)
            http_port="''${2:?missing --http-port value}"
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

      if [[ -z "$http_port" || -z "$state_dir" ]]; then
        echo "missing required --http-port or --state-dir argument" >&2
        exit 64
      fi

      ws_port=$((http_port + 1))
      auth_port=$((http_port + 2))
      p2p_port=$((http_port + 3))
      reth_dir="$state_dir/reth"
      jwt_file="$reth_dir/config/jwt.hex"
      ipc_path="/tmp/nixfied-reth-$http_port.ipc"

      mkdir -p "$reth_dir/data" "$reth_dir/config"
      if [[ ! -s "$jwt_file" ]]; then
        printf '%064x\n' 0 > "$jwt_file"
      fi
      chmod 600 "$jwt_file" 2>/dev/null || true
      rm -f "$ipc_path"

      exec reth node \
        --datadir "$reth_dir/data" \
        --ipcpath "$ipc_path" \
        --port "$p2p_port" \
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
          "\${port}"
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
    endpoint = {
      endpointId = "reth-http";
    };
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
