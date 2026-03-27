{
  pkgs,
  ...
}:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };

  basePort = 27200;

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { lib, ... }:
        {
          nixfied.services.postgres.enable = lib.mkForce false;
          nixfied.services.nginx.enable = lib.mkForce false;
          nixfied.services.minio.enable = lib.mkForce false;
          nixfied.services.reth.enable = lib.mkForce false;
          nixfied.services.helios = {
            enable = lib.mkForce true;
            executionRpcPortKey = lib.mkForce "heliosExec";
            sourceKeys = lib.mkForce [
              "real"
              "shim"
            ];
            defaultSource = lib.mkForce "shim";
            sourceKinds = lib.mkForce {
              real = "real";
              shim = "shim";
            };
            readiness = lib.mkForce {
              profile = "strict";
              requireNotSyncing = false;
              disallowSourceKinds = [ ];
            };
          };

          nixfied.runtime.ports = lib.mkForce {
            http = basePort + 0;
            https = basePort + 1;
            minioApi = basePort + 2;
            minioConsole = basePort + 3;
            postgres = basePort + 4;
            rethHttp = basePort + 5;
            rethWs = basePort + 6;
            rethAuth = basePort + 7;
            heliosRpc = basePort + 8;
            heliosExec = basePort + 9;
          };
        }
      )
    ];
    localOverrides = [ ];
  };

  readyTask = pkgs.writeShellScript "ready-helios-sync-gate-task" (
    compiled.model.tasks."task.ops.ready".runner.command
  );

  runtimeSlotStride = toString compiled.model.runtime.slot.stride;
  runtimeEnvDevOffset = toString (compiled.model.runtime.env.offsets.dev or 0);
in
pkgs.runCommand "ready-helios-sync-gate-smoke" { } ''
  set -euo pipefail

  READY_TASK="${readyTask}"
  ${shellHelpers.jsonRpcStub.shellLib}

  env_offset=${runtimeEnvDevOffset}
  slot_value=0

  compute_port() {
    local base="$1"
    echo $(( base + env_offset + (slot_value * ${runtimeSlotStride}) ))
  }

  require_contains() {
    local file="$1"
    local needle="$2"
    if ! ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$file"; then
      echo "missing expected text '$needle' in $file"
      echo "--- $file"
      cat "$file"
      exit 1
    fi
  }

  wait_for_http() {
    local port="$1"
    local method="$2"
    local tries=0
    while [ "$tries" -lt 100 ]; do
      if ${pkgs.curl}/bin/curl -fsS --max-time 2 \
        -H 'content-type: application/json' \
        --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":[]}" \
        "http://127.0.0.1:$port" \
        | ${pkgs.gnugrep}/bin/grep -q '"result"'; then
        return 0
      fi
      tries=$((tries + 1))
      sleep 0.1
    done
    echo "http probe did not become ready on port $port"
    exit 1
  }

  echo '"0x1"' > "$TMPDIR/helios-block-result.json"
  echo 'false' > "$TMPDIR/helios-syncing-result.json"

  helios_rpc_port="$(compute_port ${toString (basePort + 8)})"
  helios_execution_rpc_port="$(compute_port ${toString (basePort + 9)})"

  bg_pids=()
  shutdown_probes() {
    local pid
    for pid in "''${bg_pids[@]}"; do
      kill "$pid" >/dev/null 2>&1 || true
    done
    for pid in "''${bg_pids[@]}"; do
      wait "$pid" >/dev/null 2>&1 || true
    done
    bg_pids=()
  }

  cleanup() {
    local rc=$?
    shutdown_probes || true
    trap - EXIT
    exit "$rc"
  }
  trap cleanup EXIT

  export NIXFIED_JSONRPC_RESULT_FILE_ETH_BLOCKNUMBER="$TMPDIR/helios-block-result.json"
  export NIXFIED_JSONRPC_RESULT_FILE_ETH_SYNCING="$TMPDIR/helios-syncing-result.json"
  export NIXFIED_JSONRPC_RESULT_JSON_ETH_CHAINID='"0x1"'

  start_jsonrpc_stub "$helios_rpc_port"
  start_jsonrpc_stub "$helios_execution_rpc_port"

  wait_for_http "$helios_rpc_port" "eth_blockNumber"
  wait_for_http "$helios_execution_rpc_port" "eth_chainId"

  disallowed_log="$TMPDIR/ready-disallowed.log"
  set +e
  NIX_ENV="$slot_value" PROJECT_ENV="dev" \
    "$READY_TASK" --service helios > "$disallowed_log" 2>&1
  disallowed_rc="$?"
  set -e
  if [ "$disallowed_rc" -eq 0 ]; then
    echo "expected strict profile to reject shim source"
    cat "$disallowed_log"
    exit 1
  fi
  require_contains "$disallowed_log" "source=shim"
  require_contains "$disallowed_log" "source_kind=shim"
  require_contains "$disallowed_log" "source kind disallowed"

  echo '{"startingBlock":"0x0","currentBlock":"0x1","highestBlock":"0x2"}' > "$TMPDIR/helios-syncing-result.json"
  syncing_log="$TMPDIR/ready-syncing.log"
  set +e
  NIX_ENV="$slot_value" PROJECT_ENV="dev" \
    "$READY_TASK" --service helios --source real > "$syncing_log" 2>&1
  syncing_rc="$?"
  set -e
  if [ "$syncing_rc" -eq 0 ]; then
    echo "expected strict profile to reject syncing helios source"
    cat "$syncing_log"
    exit 1
  fi
  require_contains "$syncing_log" "source=real"
  require_contains "$syncing_log" "source_kind=real"
  require_contains "$syncing_log" "eth_syncing={"

  echo 'false' > "$TMPDIR/helios-syncing-result.json"
  ready_log="$TMPDIR/ready-ok.log"
  NIX_ENV="$slot_value" PROJECT_ENV="dev" \
    "$READY_TASK" --service helios --source real > "$ready_log" 2>&1
  require_contains "$ready_log" "OK: helios ready port=$helios_rpc_port block_number=0x1"
  require_contains "$ready_log" "OK: helios sync status ready port=$helios_rpc_port"
  require_contains "$ready_log" "OK: helios execution ready port=$helios_execution_rpc_port"
  require_contains "$ready_log" "OK: readiness checks passed services=2"

  echo "OK: helios strict readiness gates source kind and sync state" > "$out"
''
