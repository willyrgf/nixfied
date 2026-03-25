{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  slotsStub =
    let
      slotInfo = pkgs.writeShellScript "service-lifecycle-slot-info" ''
        printf 'SLOT=%q\n' "''${SLOT:-0}"
        printf 'ENV=%q\n' "''${ENV:-test}"
        printf 'RUN_DIR=%q\n' "''${RUN_DIR:-/tmp}"
        printf 'LOG_DIR=%q\n' "''${LOG_DIR:-/tmp}"
        printf 'CONFIG_DIR=%q\n' "''${CONFIG_DIR:-/tmp}"
        printf 'POSTGRES_PORT=%q\n' "''${POSTGRES_PORT:-55433}"
        printf 'HTTP_PORT=%q\n' "''${HTTP_PORT:-28080}"
        printf 'HTTPS_PORT=%q\n' "''${HTTPS_PORT:-28443}"
        printf 'MINIO_API_PORT=%q\n' "''${MINIO_API_PORT:-29000}"
        printf 'MINIO_CONSOLE_PORT=%q\n' "''${MINIO_CONSOLE_PORT:-29001}"
        printf 'RETH_HTTP_PORT=%q\n' "''${RETH_HTTP_PORT:-29100}"
        printf 'RETH_WS_PORT=%q\n' "''${RETH_WS_PORT:-29101}"
        printf 'RETH_AUTH_PORT=%q\n' "''${RETH_AUTH_PORT:-29102}"
        printf 'HELIOSRPC_PORT=%q\n' "''${HELIOSRPC_PORT:-29200}"
      '';
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
      getSlotInfo = slotInfo;
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

        ${shellHelpers.httpStub.shellLib}
        bg_pids=()
        cleanup() {
          local pid=""
          for pid in "''${bg_pids[@]:-}"; do
            kill "$pid" 2>/dev/null || true
          done
        }
        trap cleanup EXIT INT TERM

        export NIXFIED_HTTP_BODY_DEFAULT="ok"

        while IFS= read -r port; do
          [ -n "$port" ] || continue
          start_http_stub "$port"
        done <<EOF
    $LISTEN_PORTS
    EOF

        wait
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

    ${shellHelpers.httpStub.shellLib}
    bg_pids=()
    cleanup() {
      local pid=""
      for pid in "''${bg_pids[@]:-}"; do
        kill "$pid" 2>/dev/null || true
      done
    }
    trap cleanup EXIT INT TERM

    export NIXFIED_HTTP_STATUS_DEFAULT="404"
    export NIXFIED_HTTP_BODY_DEFAULT=""
    export NIXFIED_HTTP_STATUS_MINIO_HEALTH_LIVE="200"
    export NIXFIED_HTTP_BODY_MINIO_HEALTH_LIVE="ok"
    export NIXFIED_HTTP_STATUS_MINIO_HEALTH_READY="200"
    export NIXFIED_HTTP_BODY_MINIO_HEALTH_READY="ok"

    for port in "$API_PORT" "$CONSOLE_PORT"; do
      [ -n "$port" ] || continue
      start_http_stub "$port"
    done

    wait
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

    ${shellHelpers.jsonRpcStub.shellLib}
    bg_pids=()
    cleanup() {
      local pid=""
      for pid in "''${bg_pids[@]:-}"; do
        kill "$pid" 2>/dev/null || true
      done
    }
    trap cleanup EXIT INT TERM

    export NIXFIED_JSONRPC_RESULT_JSON_WEB3_CLIENTVERSION='"reth-stub"'
    export NIXFIED_JSONRPC_RESULT_JSON_ETH_CHAINID='"0x1"'

    for port in "$HTTP_PORT" "$WS_PORT" "$AUTH_PORT"; do
      [ -n "$port" ] || continue
      start_jsonrpc_stub "$port"
    done

    wait
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

    ${shellHelpers.jsonRpcStub.shellLib}
    bg_pids=()
    cleanup() {
      local pid=""
      for pid in "''${bg_pids[@]:-}"; do
        kill "$pid" 2>/dev/null || true
      done
    }
    trap cleanup EXIT INT TERM

    export NIXFIED_JSONRPC_RESULT_JSON_ETH_CHAINID='"0x539"'
    export NIXFIED_JSONRPC_RESULT_JSON_ETH_BLOCKNUMBER='"0x2a"'
    export NIXFIED_JSONRPC_RESULT_JSON_ETH_SYNCING='false'
    export NIXFIED_JSONRPC_RESULT_JSON_WEB3_CLIENTVERSION='"helios-stub"'

    for port in "$RPC_PORT" "$EXECUTION_PORT"; do
      [ -n "$port" ] || continue
      start_jsonrpc_stub "$port"
    done

    wait
  '';

  heliosStub = mkPackageWithScript {
    name = "helios-lifecycle-stub";
    binName = "helios";
    script = heliosStubScript;
  };

  nginxProject = projectBase // {
    services.nginx = {
      defaultSource = "stub";
      sources = {
        stub.package = nginxStub;
      };
    };
  };

  minioProject = projectBase // {
    services.minio = {
      defaultSource = "stub";
      sources = {
        stub.package = minioStub;
      };
    };
  };

  rethProject = projectBase // {
    services.reth = {
      defaultSource = "stub";
      sources = {
        stub.package = rethStub;
      };
    };
  };

  heliosProject = projectBase // {
    services.helios = {
      defaultSource = "stub";
      sources = {
        stub.package = heliosStub;
      };
    };
  };

  postgresService = import ../../nixfied/framework/runtime/services/postgres/default.nix {
    inherit pkgs;
    project = projectBase;
    slots = slotsStub;
  };

  nginxSummary = import ../../nixfied/framework/runtime/helpers/summary.nix {
    inherit pkgs;
    project = nginxProject;
  };

  nginxHelpers = import ../../nixfied/framework/runtime/helpers/helpers.nix {
    inherit pkgs;
    project = nginxProject;
    inherit (nginxSummary) summaryParser;
  };

  nginxConfig = import ../../nixfied/framework/runtime/services/nginx/config.nix {
    inherit pkgs;
    project = nginxProject;
  };

  nginxTemplates = import ../../nixfied/framework/runtime/services/nginx/templates.nix {
    inherit pkgs;
    package = nginxConfig.package or pkgs.nginx;
  };

  nginxLifecycle = import ../../nixfied/framework/runtime/services/nginx/lifecycle.nix {
    inherit pkgs;
    project = nginxProject;
    slots = slotsStub;
    config = nginxConfig;
    templates = nginxTemplates;
    loggingPrelude = nginxHelpers.loggingPrelude;
  };

  nginxService = import ../../nixfied/framework/runtime/services/nginx/default.nix {
    inherit pkgs;
    project = nginxProject;
    slots = slotsStub;
  };

  minioService = import ../../nixfied/framework/runtime/services/minio/default.nix {
    inherit pkgs;
    project = minioProject;
    slots = slotsStub;
  };

  rethService = import ../../nixfied/framework/runtime/services/reth/default.nix {
    inherit pkgs;
    project = rethProject;
    slots = slotsStub;
  };

  heliosService = import ../../nixfied/framework/runtime/services/helios/default.nix {
    inherit pkgs;
    project = heliosProject;
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
  FULL_START_TIMEOUT_SECS=15
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

  reset_service_case_dirs() {
    local service_name="$1"
    local case_name="$2"

    SERVICE_ROOT="$TMPDIR/$service_name-$case_name-root"
    RUN_DIR="$TMPDIR/$service_name-$case_name-run"
    LOG_DIR="$TMPDIR/$service_name-$case_name-log"
    CONFIG_DIR="$TMPDIR/$service_name-$case_name-config"
    export SERVICE_ROOT RUN_DIR LOG_DIR CONFIG_DIR
    mkdir -p "$SERVICE_ROOT" "$RUN_DIR" "$LOG_DIR" "$CONFIG_DIR"
  }

  dump_service_debug() {
    local service_name="$1"
    local phase_name="$2"
    local output_file="$TMPDIR/$service_name-$phase_name.out"
    local service_dir="$SERVICE_ROOT/$service_name"
    local log_path=""

    if [ -f "$output_file" ]; then
      echo "--- $output_file" >&2
      cat "$output_file" >&2
    fi

    if [ -f "$service_dir/postgres.log" ]; then
      echo "--- $service_dir/postgres.log" >&2
      cat "$service_dir/postgres.log" >&2
    fi

    for log_path in "$service_dir/logs"/*; do
      if [ -f "$log_path" ]; then
        echo "--- $log_path" >&2
        cat "$log_path" >&2
      fi
    done
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
      dump_service_debug "$service_name" "start"
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
    wait_for_background_exit "$RESTART_WRAPPER_PID"
    RESTART_WRAPPER_PID=""

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

  run_full_start_case() {
    local service_name="$1"
    local phase_name="$2"
    local full_start_bin="$3"
    local status_bin="$4"
    local health_bin="$5"
    local ready_bin="$6"
    local stop_bin="$7"
    local rc=0

    reset_service_case_dirs "$service_name" "$phase_name"

    echo "INFO: lifecycle-smoke service=$service_name phase=$phase_name" >&2
    set +e
    ${pkgs.coreutils}/bin/timeout "$FULL_START_TIMEOUT_SECS" "$full_start_bin" > "$TMPDIR/$service_name-$phase_name.out" 2>&1
    rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      dump_service_debug "$service_name" "$phase_name"
      fail "$service_name $phase_name should return after readiness"
    fi

    echo "INFO: lifecycle-smoke service=$service_name phase=status-after-$phase_name" >&2
    "$status_bin" > "$TMPDIR/$service_name-$phase_name-status.out" 2>&1 || {
      dump_service_debug "$service_name" "$phase_name"
      fail "$service_name $phase_name should leave the service running"
    }
    require_contains "$TMPDIR/$service_name-$phase_name-status.out" "service=$service_name"
    require_contains "$TMPDIR/$service_name-$phase_name-status.out" "running=true"

    echo "INFO: lifecycle-smoke service=$service_name phase=health-after-$phase_name" >&2
    if ! wait_for_success \
      "$service_name health after $phase_name" \
      "$health_bin" \
      "$TMPDIR/$service_name-$phase_name-health.out"
    then
      dump_service_debug "$service_name" "$phase_name"
      "$stop_bin" >/dev/null 2>&1 || true
      exit 1
    fi

    echo "INFO: lifecycle-smoke service=$service_name phase=ready-after-$phase_name" >&2
    if ! wait_for_success \
      "$service_name ready after $phase_name" \
      "$ready_bin" \
      "$TMPDIR/$service_name-$phase_name-ready.out"
    then
      dump_service_debug "$service_name" "$phase_name"
      "$stop_bin" >/dev/null 2>&1 || true
      exit 1
    fi

    echo "INFO: lifecycle-smoke service=$service_name phase=stop-after-$phase_name" >&2
    "$stop_bin" > "$TMPDIR/$service_name-$phase_name-stop.out" 2>&1 || {
      cat "$TMPDIR/$service_name-$phase_name-stop.out" >&2
      fail "$service_name stop after $phase_name should succeed"
    }

    wait_for_failure \
      "$service_name health after stop ($phase_name)" \
      "$health_bin" \
      "$TMPDIR/$service_name-$phase_name-health-stopped.out"
    wait_for_failure \
      "$service_name ready after stop ($phase_name)" \
      "$ready_bin" \
      "$TMPDIR/$service_name-$phase_name-ready-stopped.out"
    wait_for_failure \
      "$service_name status after stop ($phase_name)" \
      "$status_bin" \
      "$TMPDIR/$service_name-$phase_name-status-stopped.out"
    require_contains "$TMPDIR/$service_name-$phase_name-status-stopped.out" "running=false"
    echo "INFO: lifecycle-smoke service=$service_name phase=$phase_name-done" >&2
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
  run_full_start_case \
    nginx \
    full-start \
    "${nginxLifecycle.fullStart}" \
    "${nginxService.status}" \
    "${nginxService.health}" \
    "${nginxService.ready}" \
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
  run_full_start_case \
    minio \
    full-start \
    "${minioService.fullStart}" \
    "${minioService.status}" \
    "${minioService.health}" \
    "${minioService.ready}" \
    "${minioService.stop}"
  run_full_start_case \
    minio \
    full-start-test \
    "${minioService.fullStartTest}" \
    "${minioService.status}" \
    "${minioService.health}" \
    "${minioService.ready}" \
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
  run_full_start_case \
    reth \
    full-start \
    "${rethService.fullStart}" \
    "${rethService.status}" \
    "${rethService.health}" \
    "${rethService.ready}" \
    "${rethService.stop}"
  run_full_start_case \
    reth \
    full-start-test \
    "${rethService.fullStartTest}" \
    "${rethService.status}" \
    "${rethService.health}" \
    "${rethService.ready}" \
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
  run_full_start_case \
    helios \
    full-start \
    "${heliosService.fullStart}" \
    "${heliosService.status}" \
    "${heliosService.health}" \
    "${heliosService.ready}" \
    "${heliosService.stop}"
  run_full_start_case \
    helios \
    full-start-test \
    "${heliosService.fullStartTest}" \
    "${heliosService.status}" \
    "${heliosService.health}" \
    "${heliosService.ready}" \
    "${heliosService.stop}"

  echo "OK: public service modules cover direct start stop restart health ready and full-start return-after-readiness behavior" > "$out"
''
