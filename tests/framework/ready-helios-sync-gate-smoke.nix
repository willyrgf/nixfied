{
  pkgs,
  registry,
  ...
}:
let
  frameworkLib = import ../../nixfied/lib {
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

  executor = import ../../nixfied/runner/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = compiled.model;
    projectRoot = ../..;
  };

  runtimeSlotStride = toString compiled.model.runtime.slot.stride;
  runtimeEnvDevOffset = toString (compiled.model.runtime.env.offsets.dev or 0);
in
pkgs.runCommand "ready-helios-sync-gate-smoke" { } ''
    set -euo pipefail

    EXECUTOR="${executor}/bin/nixfied-executor"
    export REGISTRY_ROOT="$TMPDIR/registry"
    mkdir -p "$REGISTRY_ROOT"

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
      while [ "$tries" -lt 50 ]; do
        if ${pkgs.curl}/bin/curl -fsS --max-time 1 \
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

    cat > "$TMPDIR/helios-responder.sh" <<'EOF_SCRIPT'
  #!${pkgs.bash}/bin/bash
  set -euo pipefail
  content_length=0

  while IFS= read -r header_line; do
    header_line="''${header_line%$'\r'}"
    if [ -z "$header_line" ]; then
      break
    fi
    case "$header_line" in
      [Cc]ontent-[Ll]ength:*)
        parsed_length="$(printf '%s' "$header_line" | ${pkgs.gnused}/bin/sed -n 's/^[^:]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')"
        if [ -n "$parsed_length" ]; then
          content_length="$parsed_length"
        fi
        ;;
    esac
  done

  request_body=""
  if [ "$content_length" -gt 0 ]; then
    request_body="$(${pkgs.coreutils}/bin/head -c "$content_length")"
  fi

  method="$(printf '%s' "$request_body" | ${pkgs.gnused}/bin/sed -n 's/.*"method":"\([^"]*\)".*/\1/p')"

  case "$method" in
    eth_blockNumber)
      result="$(${pkgs.coreutils}/bin/cat "$TMPDIR/helios-block-result.json")"
      ;;
    eth_syncing)
      result="$(${pkgs.coreutils}/bin/cat "$TMPDIR/helios-syncing-result.json")"
      ;;
    eth_chainId)
      result='"0x1"'
      ;;
    *)
      result='null'
      ;;
  esac

  printf 'HTTP/1.1 200 OK\r\n'
  printf 'Content-Type: application/json\r\n'
  printf 'Connection: close\r\n'
  printf '\r\n'
  printf '{"jsonrpc":"2.0","id":1,"result":%s}\n' "$result"
  EOF_SCRIPT
    chmod +x "$TMPDIR/helios-responder.sh"

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

    ${pkgs.socat}/bin/socat "TCP-LISTEN:$helios_rpc_port,bind=127.0.0.1,reuseaddr,fork" \
      "EXEC:$TMPDIR/helios-responder.sh" \
      >/dev/null 2>&1 &
    bg_pids+=("$!")

    ${pkgs.socat}/bin/socat "TCP-LISTEN:$helios_execution_rpc_port,bind=127.0.0.1,reuseaddr,fork" \
      "EXEC:$TMPDIR/helios-responder.sh" \
      >/dev/null 2>&1 &
    bg_pids+=("$!")

    wait_for_http "$helios_rpc_port" "eth_blockNumber"
    wait_for_http "$helios_execution_rpc_port" "eth_chainId"

    disallowed_log="$TMPDIR/ready-disallowed.log"
    set +e
    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.ready --service helios > "$disallowed_log" 2>&1
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
    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.ready --service helios --source real > "$syncing_log" 2>&1
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
    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.ready --service helios --source real > "$ready_log" 2>&1
    require_contains "$ready_log" "OK: helios ready port=$helios_rpc_port block_number=0x1"
    require_contains "$ready_log" "OK: helios sync status ready port=$helios_rpc_port"
    require_contains "$ready_log" "OK: helios execution ready port=$helios_execution_rpc_port"
    require_contains "$ready_log" "OK: readiness checks passed services=2"

    echo "OK: helios strict readiness gates source kind and sync state" > "$out"
''
