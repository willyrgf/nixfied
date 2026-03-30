# Shell runtime helpers - sourced by all app scripts
{
  pkgs,
  project ? { },
  hooks ? { },
  summaryParser,
}:

let
  cleanupRuntime = import ./cleanup-runtime.nix { };
  commandRuntime = import ./command-runtime.nix {
    inherit
      pkgs
      project
      hooks
      ;
  };
  fixtureRuntime = import ./fixture-runtime.nix { };
  kernelPackage = import ../kernel { inherit pkgs; };
  inherit (commandRuntime)
    loadEnv
    loadEnvFile
    loggingPrelude
    hookExports
    ;

  helpersScript = pkgs.writeShellScript "framework-helpers" ''
    source ${commandRuntime.commandHelpersScript}

    # summary_parse LOGFILE DURATION EXIT_CODE
    # - print a compact run summary (used by CI --summary mode).
    summary_parse() {
      local logfile="$1"
      local duration="$2"
      local exit_code="$3"
      ${summaryParser} "$logfile" "$duration" "$exit_code"
    }

    ${cleanupRuntime.cleanupRuntime}

    ${fixtureRuntime.fixtureRuntime}

    # wait_port PORT [timeout] [interval]
    # - wait for a TCP port to listen (lsof or nc).
    wait_port() {
      local port="$1"
      local timeout="''${2:-30}"
      local interval="''${3:-1}"

      if [ -z "$port" ]; then
        echo "usage: wait_port <port> [timeout] [interval]" >&2
        return 1
      fi
      require_port "port" "$port" || return 1

      _wait_port_probe() {
        if command -v lsof >/dev/null 2>&1; then
          if lsof -iTCP:"$port" -sTCP:LISTEN -n -P >/dev/null 2>&1; then
            return 0
          fi
        elif command -v nc >/dev/null 2>&1; then
          if nc -z "$NIXFIED_LOCALHOST_NAME" "$port" >/dev/null 2>&1; then
            return 0
          fi
        fi
        return 1
      }

      wait_until "$timeout" "$interval" _wait_port_probe
    }

    # stop_service PID [name]
    # - terminate a background service and wait for it to exit.
    stop_service() {
      local pid="$1"
      local name="''${2:-service}"

      if [ -z "$pid" ]; then
        echo "usage: stop_service <pid> [name]" >&2
        return 1
      fi

      if kill -0 "$pid" 2>/dev/null; then
        log_stop "$name (PID $pid)"
        kill -TERM "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
      fi
    }

    _start_service_policy_any_set() {
      [ -n "''${SERVICE_OWNER_SCOPE:-}" ] || [ -n "''${SERVICE_REUSE_POLICY:-}" ] || [ -n "''${SERVICE_DISCOVERY_SCOPE:-}" ]
    }

    _start_service_is_truthy() {
      case "''${1:-}" in
        1|true|TRUE|yes|YES|on|ON)
          return 0
          ;;
        *)
          return 1
          ;;
      esac
    }

    load_start_service_policy_exports() {
      nixfied_load_kernel_exports "nixfied-start-service-policy" \
        ${kernelPackage}/bin/nixfied-kernel service-policy start-service \
        "''${SERVICE_REUSE_POLICY:-}" \
        "''${SERVICE_OWNER_SCOPE:-}" \
        "''${SERVICE_DISCOVERY_SCOPE:-}" \
        || return 1
    }

    # start_service_should_register_cleanup [explicit_mode]
    # - return 1 to register stop cleanup and 0 to preserve process after command exit.
    start_service_should_register_cleanup() {
      local explicit_mode="''${1:-auto}"
      local owner_scope=""
      local discovery_scope=""
      local reuse_policy=""

      case "$explicit_mode" in
        cleanup)
          echo "1"
          return 0
          ;;
        keep-running)
          echo "0"
          return 0
          ;;
        auto)
          ;;
        *)
          log_error "invalid start_service cleanup mode '$explicit_mode' (expected auto|cleanup|keep-running)"
          return 1
          ;;
      esac

      if _start_service_is_truthy "''${NIXFIED_START_SERVICE_MANAGED_CLEANUP:-0}"; then
        echo "0"
        return 0
      fi

      if ! _start_service_policy_any_set; then
        echo "1"
        return 0
      fi

      load_start_service_policy_exports || return 1
      owner_scope="$OWNER_SCOPE"
      discovery_scope="$DISCOVERY_SCOPE"
      reuse_policy="$REUSE_POLICY"
      echo "$REGISTER_CLEANUP"
      return 0
    }

    # start_service NAME [opts] -- <command...>
    # - run a background service with optional logging and readiness checks.
    start_service() {
      local name="$1"
      shift || true

      if [ -z "$name" ]; then
        echo "usage: start_service <name> [--log <file>] [--cwd <dir>] [--wait-http <url>] [--wait-port <port>] [--timeout <s>] [--interval <s>] [--keep-running|--cleanup] -- <command...>" >&2
        return 1
      fi

      local log=""
      local cwd=""
      local wait_http_url=""
      local wait_port_num=""
      local timeout="30"
      local interval="1"
      local cleanup_mode="auto"
      local register_cleanup="1"

      while [ "''$#" -gt 0 ]; do
        case "''$1" in
          --log)
            log="$2"
            shift 2
            ;;
          --cwd)
            cwd="$2"
            shift 2
            ;;
          --wait-http)
            wait_http_url="$2"
            shift 2
            ;;
          --wait-port)
            wait_port_num="$2"
            shift 2
            ;;
          --timeout)
            timeout="$2"
            shift 2
            ;;
          --interval)
            interval="$2"
            shift 2
            ;;
          --keep-running)
            if [ "$cleanup_mode" = "cleanup" ]; then
              log_error "start_service flags --keep-running and --cleanup are mutually exclusive"
              return 1
            fi
            cleanup_mode="keep-running"
            shift
            ;;
          --cleanup)
            if [ "$cleanup_mode" = "keep-running" ]; then
              log_error "start_service flags --keep-running and --cleanup are mutually exclusive"
              return 1
            fi
            cleanup_mode="cleanup"
            shift
            ;;
          --)
            shift
            break
            ;;
          *)
            break
            ;;
        esac
      done

      if [ "''$#" -eq 0 ]; then
        echo "start_service: missing command" >&2
        return 1
      fi
      require_positive_int "--timeout" "$timeout" || return 1
      require_positive_number "--interval" "$interval" || return 1
      if [ -n "$wait_port_num" ]; then
        require_port "--wait-port" "$wait_port_num" || return 1
      fi
      register_cleanup="$(start_service_should_register_cleanup "$cleanup_mode")" || return 1
      if [ "$register_cleanup" != "0" ] && [ "$register_cleanup" != "1" ]; then
        log_error "start_service cleanup decision must be 0 or 1 (got '$register_cleanup')"
        return 1
      fi

      local in_subshell="false"
      if [ "''${BASH_SUBSHELL:-0}" -gt 0 ]; then
        in_subshell="true"
      fi

      if [ "$in_subshell" = "true" ] && command -v nohup >/dev/null 2>&1; then
        if [ -n "$cwd" ]; then
          if [ -n "$log" ]; then
            (cd "$cwd" && nohup "$@" > "$log" 2>&1) &
          else
            (cd "$cwd" && nohup "$@" >/dev/null 2>&1) &
          fi
        else
          if [ -n "$log" ]; then
            nohup "$@" > "$log" 2>&1 &
          else
            nohup "$@" >/dev/null 2>&1 &
          fi
        fi
      else
        if [ -n "$cwd" ]; then
          if [ -n "$log" ]; then
            (cd "$cwd" && "$@" > "$log" 2>&1) &
          else
            (cd "$cwd" && "$@") &
          fi
        else
          if [ -n "$log" ]; then
            ("$@" > "$log" 2>&1) &
          else
            ("$@") &
          fi
        fi
      fi

      local pid=$!

      if [ -n "$wait_http_url" ]; then
        if ! wait_http "$wait_http_url" "$timeout" "$interval"; then
          log_error "$name failed readiness check (http)"
          stop_service "$pid" "$name" >/dev/null 2>&1 || true
          return 1
        fi
      fi

      if [ -n "$wait_port_num" ]; then
        if ! wait_port "$wait_port_num" "$timeout" "$interval"; then
          log_error "$name failed readiness check (port)"
          stop_service "$pid" "$name" >/dev/null 2>&1 || true
          return 1
        fi
      fi

      # Avoid registering cleanup in command substitution subshells (they exit immediately).
      # App wrappers that execute command bodies in a dedicated subshell set
      # NIXFIED_CLEANUP_OWNER_BASHPID to the app-body shell BASHPID.
      if [ "$register_cleanup" = "1" ] && { { [ -n "''${NIXFIED_CLEANUP_OWNER_BASHPID:-}" ] && [ -n "''${BASHPID:-}" ] && [ "''${NIXFIED_CLEANUP_OWNER_BASHPID}" = "''${BASHPID}" ]; } || { [ -n "''${BASHPID:-}" ] && [ "''${BASHPID}" = "$$" ]; }; }; then
        with_cleanup stop_service "$pid" "$name"
      fi

      echo "$pid"
    }

    # start_service_into PID_VAR NAME [opts] -- <command...>
    # - start a service and assign PID to PID_VAR in the current shell.
    start_service_into() {
      local pid_var="$1"
      shift || true

      if [ -z "$pid_var" ]; then
        echo "usage: start_service_into <pid_var> <name> [opts] -- <command...>" >&2
        return 1
      fi

      # Avoid command substitution so start_service runs in this shell context.
      local pid_file=""
      pid_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-start-service.XXXXXX")" || {
        echo "start_service_into: failed to allocate pid capture file" >&2
        return 1
      }

      local _pid=""
      if ! start_service "$@" >"$pid_file"; then
        rm -f "$pid_file" >/dev/null 2>&1 || true
        echo "start_service_into: failed to start service" >&2
        return 1
      fi

      local line=""
      while IFS= read -r line; do
        if [ -n "$line" ]; then
          _pid="$line"
        fi
      done <"$pid_file"
      rm -f "$pid_file" >/dev/null 2>&1 || true

      if [ -z "$_pid" ]; then
        echo "start_service_into: failed to start service" >&2
        return 1
      fi
      printf -v "$pid_var" '%s' "$_pid"
      return 0
    }

    # with_service NAME [start opts] -- <start command...> --run <command...>
    # - start service, then run command (cleanup handled automatically).
    with_service() {
      local name="$1"
      shift || true

      if [ -z "$name" ]; then
        echo "usage: with_service <name> [start options] -- <start command...> --run <command...>" >&2
        return 1
      fi

      local args=()
      local run_cmd=()
      local state="start"

      while [ "''$#" -gt 0 ]; do
        case "''$1" in
          --run)
            state="run"
            shift
            ;;
          *)
            if [ "$state" = "start" ]; then
              args+=("''$1")
            else
              run_cmd+=("''$1")
            fi
            shift
            ;;
        esac
      done

      if [ "''${#run_cmd[@]}" -eq 0 ]; then
        echo "with_service: missing --run <command...>" >&2
        return 1
      fi

      local pid=""
      local rc=0
      if ! start_service_into pid "$name" "''${args[@]}"; then
        echo "with_service: failed to start $name" >&2
        return 1
      fi
      set +e
      "''${run_cmd[@]}"
      rc=$?
      set -e
      stop_service "$pid" "$name"
      return "$rc"
    }
  '';
in
{
  inherit
    loadEnv
    loadEnvFile
    loggingPrelude
    helpersScript
    hookExports
    ;
}
