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

  basePort = 27100;

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { lib, ... }:
        {
          nixfied.services.postgres = {
            enable = lib.mkForce true;
            database = lib.mkForce "postgres";
          };
          nixfied.services.nginx.enable = lib.mkForce true;
          nixfied.services.minio.enable = lib.mkForce true;
          nixfied.services.reth.enable = lib.mkForce true;
          nixfied.services.helios = {
            enable = lib.mkForce true;
            executionRpcPortKey = lib.mkForce "heliosExec";
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
  netcatPkg = if pkgs ? netcat then pkgs.netcat else pkgs.netcat-openbsd;
  postgresPkg = if pkgs ? postgresql_16 then pkgs.postgresql_16 else pkgs.postgresql;
in
pkgs.runCommand "ready-health-shutdown-smoke" { } ''
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
      if ! ${pkgs.gnugrep}/bin/grep -Fq "$needle" "$file"; then
        echo "missing expected text '$needle' in $file"
        echo "--- $file"
        cat "$file"
        exit 1
      fi
    }

    require_not_contains() {
      local file="$1"
      local needle="$2"
      if ${pkgs.gnugrep}/bin/grep -Fq "$needle" "$file"; then
        echo "unexpected text '$needle' in $file"
        echo "--- $file"
        cat "$file"
        exit 1
      fi
    }

    wait_for_tcp() {
      local port="$1"
      local tries=0
      while [ "$tries" -lt 50 ]; do
        if ${netcatPkg}/bin/nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
          return 0
        fi
        tries=$((tries + 1))
        sleep 0.1
      done
      echo "listener did not become ready on port $port"
      exit 1
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

    wait_for_postgres() {
      local port="$1"
      local tries=0
      while [ "$tries" -lt 50 ]; do
        if ${postgresPkg}/bin/pg_isready -U postgres -h 127.0.0.1 -p "$port" -q >/dev/null 2>&1; then
          return 0
        fi
        tries=$((tries + 1))
        sleep 0.1
      done
      echo "postgres did not become ready on port $port"
      exit 1
    }

    start_tcp_listener() {
      local port="$1"
      ${pkgs.socat}/bin/socat "TCP-LISTEN:$port,bind=127.0.0.1,reuseaddr,fork" \
        "EXEC:${pkgs.coreutils}/bin/true" \
        >/dev/null 2>&1 &
      bg_pids+=("$!")
    }

    started_http_ports=","
    start_http_responder() {
      local port="$1"
      case "$started_http_ports" in
        *,"$port",*)
          return 0
          ;;
      esac
      started_http_ports="$started_http_ports$port,"

      ${pkgs.socat}/bin/socat "TCP-LISTEN:$port,bind=127.0.0.1,reuseaddr,fork" \
        "SYSTEM:${pkgs.coreutils}/bin/cat $TMPDIR/jsonrpc-response.http" \
        >/dev/null 2>&1 &
      bg_pids+=("$!")
    }

    shutdown_done=0
    pg_data=""
    bg_pids=()
    shutdown_probes() {
      if [ "$shutdown_done" -eq 1 ]; then
        return 0
      fi
      shutdown_done=1

      local pid
      for pid in "''${bg_pids[@]}"; do
        kill "$pid" >/dev/null 2>&1 || true
      done
      for pid in "''${bg_pids[@]}"; do
        wait "$pid" >/dev/null 2>&1 || true
      done
      bg_pids=()

      if [ -n "$pg_data" ]; then
        ${postgresPkg}/bin/pg_ctl -D "$pg_data" -m fast -w stop > "$TMPDIR/postgres-stop.log" 2>&1 || true
        pg_data=""
      fi
    }

    cleanup() {
      local rc=$?
      shutdown_probes || true
      trap - EXIT
      exit "$rc"
    }
    trap cleanup EXIT

    postgres_port="$(compute_port ${toString (basePort + 4)})"
    nginx_http_port="$(compute_port ${toString (basePort + 0)})"
    nginx_https_port="$(compute_port ${toString (basePort + 1)})"
    minio_api_port="$(compute_port ${toString (basePort + 2)})"
    minio_console_port="$(compute_port ${toString (basePort + 3)})"
    reth_http_port="$(compute_port ${toString (basePort + 5)})"
    reth_ws_port="$(compute_port ${toString (basePort + 6)})"
    reth_auth_port="$(compute_port ${toString (basePort + 7)})"
    helios_rpc_port="$(compute_port ${toString (basePort + 8)})"
    helios_execution_rpc_port="$(compute_port ${toString (basePort + 9)})"

    cat > "$TMPDIR/jsonrpc-response.http" <<'EOF_HTTP'
  HTTP/1.1 200 OK
  Content-Type: application/json
  Connection: close

  {"jsonrpc":"2.0","id":1,"result":"0x1"}
  EOF_HTTP

    pg_data="$TMPDIR/postgres-data"
    ${postgresPkg}/bin/initdb -D "$pg_data" --auth=trust --username=postgres --no-locale > "$TMPDIR/postgres-init.log" 2>&1
    ${postgresPkg}/bin/pg_ctl -D "$pg_data" -o "-h 127.0.0.1 -p $postgres_port -k $TMPDIR" -w start > "$TMPDIR/postgres-start.log" 2>&1

    start_tcp_listener "$nginx_http_port"
    start_tcp_listener "$nginx_https_port"
    start_tcp_listener "$minio_api_port"
    start_tcp_listener "$minio_console_port"
    start_tcp_listener "$reth_ws_port"
    start_tcp_listener "$reth_auth_port"

    start_http_responder "$reth_http_port"
    start_http_responder "$helios_rpc_port"
    start_http_responder "$helios_execution_rpc_port"

    wait_for_postgres "$postgres_port"
    wait_for_tcp "$nginx_http_port"
    wait_for_tcp "$nginx_https_port"
    wait_for_tcp "$minio_api_port"
    wait_for_tcp "$minio_console_port"
    wait_for_tcp "$reth_ws_port"
    wait_for_tcp "$reth_auth_port"
    wait_for_http "$reth_http_port" "web3_clientVersion"
    wait_for_http "$helios_rpc_port" "eth_blockNumber"
    wait_for_http "$helios_execution_rpc_port" "eth_chainId"

    ready_log="$TMPDIR/ready.log"
    health_log="$TMPDIR/health.log"

    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.ready > "$ready_log" 2>&1
    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.health > "$health_log" 2>&1

    require_contains "$ready_log" "OK: readiness checks passed services=10"
    require_contains "$health_log" "OK: health checks passed services=10"

    require_not_contains "$ready_log" "SKIP: postgres readiness check disabled"
    require_not_contains "$ready_log" "SKIP: nginx readiness check disabled"
    require_not_contains "$ready_log" "SKIP: minio readiness check disabled"
    require_not_contains "$ready_log" "SKIP: reth readiness check disabled"
    require_not_contains "$ready_log" "SKIP: helios readiness check disabled"
    require_not_contains "$ready_log" "SKIP: no enabled services for readiness checks"

    require_not_contains "$health_log" "SKIP: postgres health check disabled"
    require_not_contains "$health_log" "SKIP: nginx health check disabled"
    require_not_contains "$health_log" "SKIP: minio health check disabled"
    require_not_contains "$health_log" "SKIP: reth health check disabled"
    require_not_contains "$health_log" "SKIP: helios health check disabled"
    require_not_contains "$health_log" "SKIP: no enabled services for health checks"

    shutdown_probes

    ready_down_log="$TMPDIR/ready-after-shutdown.log"
    health_down_log="$TMPDIR/health-after-shutdown.log"

    set +e
    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.ready > "$ready_down_log" 2>&1
    ready_down_rc="$?"

    REGISTRY_ROOT="$REGISTRY_ROOT" NIX_ENV="$slot_value" PROJECT_ENV="dev" \
      "$EXECUTOR" run-task task.ops.health > "$health_down_log" 2>&1
    health_down_rc="$?"
    set -e

    if [ "$ready_down_rc" -eq 0 ] || [ "$health_down_rc" -eq 0 ]; then
      echo "expected readiness/health to fail after shutdown (ready=$ready_down_rc health=$health_down_rc)"
      cat "$ready_down_log"
      cat "$health_down_log"
      exit 1
    fi

    require_contains "$ready_down_log" "ERROR: postgres not ready port=$postgres_port"
    require_contains "$health_down_log" "ERROR: postgres unhealthy port=$postgres_port"

    echo "OK: readiness/health checks run with live services and fail after shutdown" > "$out"
''
