{
  pkgs,
  project ? { },
  hooks ? { },
  runtimeBin ? null,
}:

let
  kernelExportRuntime = import ./kernel-export-runtime.nix { };
  loggingRuntime = import ./logging-runtime.nix { inherit pkgs; };
  envLoader = import ./env-loader.nix {
    inherit pkgs project;
    loggingPrelude = loggingPrelude;
  };
  hookEnv = hooks.env or { };
  runtimeBinExport =
    if runtimeBin == null then
      ""
    else
      ''
        export NIXFIED_RUNTIME_BIN="${toString runtimeBin}"
      '';
  hookExports = pkgs.lib.concatMapStringsSep "\n" (key: ''
    # Always pin framework hook paths for deterministic app behavior.
    # User shell/.env hook overrides can route commands to stale scripts.
    export ${key}="${toString hookEnv.${key}}"
  '') (pkgs.lib.sort (a: b: a < b) (builtins.attrNames hookEnv));

  loadEnv = envLoader.loadEnv;
  loadEnvFile = envLoader.loadEnvFile;
  loggingPrelude = loggingRuntime.loggingPrelude;

  commandHelpersScript = pkgs.writeShellScript "nixfied-command-helpers" ''
    ${loggingPrelude}
    ${kernelExportRuntime.kernelExportRuntime}
    ${runtimeBinExport}

    # svc SERVICE OP [args...]
    # - execute a compiled service operation through the runtime-owned ABI.
    svc() {
      local service="$1"
      local op="$2"
      shift 2 || true

      if [ -z "$service" ] || [ -z "$op" ]; then
        echo "usage: svc <service> <op> [args...]" >&2
        return 1
      fi

      if [ -z "''${NIXFIED_RUNTIME_BIN:-}" ]; then
        log_error "Runtime service invocation is unavailable: NIXFIED_RUNTIME_BIN is unset"
        return 1
      fi

      "$NIXFIED_RUNTIME_BIN" run-service "$service" "$op" "$@"
    }

    # require_env VAR [message]
    # - fail if VAR is unset/empty; prints message to stderr.
    require_env() {
      local var="$1"
      local msg="''${2:-Missing required env var: $var}"
      if [ -z "''${!var:-}" ]; then
        log_error "$msg"
        return 1
      fi
      return 0
    }

    # skip_if_missing VAR [reason]
    # - return 1 if VAR is missing so callers can skip work.
    skip_if_missing() {
      local var="$1"
      local reason="''${2:-Missing required env var: $var}"
      if [ -z "''${!var:-}" ]; then
        log_skip "$reason"
        return 1
      fi
      return 0
    }

    is_uint() {
      case "''${1:-}" in
        *[!0-9]*|"")
          return 1
          ;;
        *)
          return 0
          ;;
      esac
    }

    require_positive_int() {
      local name="$1"
      local value="$2"
      if ! is_uint "$value" || [ "$value" -le 0 ]; then
        log_error "$name must be a positive integer (got '$value')"
        return 1
      fi
      return 0
    }

    require_positive_number() {
      local name="$1"
      local value="$2"
      local whole=""
      local frac=""

      case "$value" in
        *[!0-9.]*|""|*.*.*|.*|*.)
          log_error "$name must be a positive number (got '$value')"
          return 1
          ;;
      esac

      if [ "''${value#*.}" = "$value" ]; then
        if ! is_uint "$value" || [ "$value" -le 0 ]; then
          log_error "$name must be a positive number (got '$value')"
          return 1
        fi
        return 0
      fi

      whole="''${value%%.*}"
      frac="''${value#*.}"
      if ! is_uint "$whole" || ! is_uint "$frac"; then
        log_error "$name must be a positive number (got '$value')"
        return 1
      fi
      if [ "$whole" -eq 0 ] && [ -z "''${frac//0/}" ]; then
        log_error "$name must be a positive number (got '$value')"
        return 1
      fi
      return 0
    }

    require_port() {
      local name="$1"
      local value="$2"
      if ! is_uint "$value" || [ "$value" -lt 1 ] || [ "$value" -gt 65535 ]; then
        log_error "$name must be a valid TCP port (1-65535, got '$value')"
        return 1
      fi
      return 0
    }

    # wait_until TIMEOUT INTERVAL CHECK_FN
    # - run CHECK_FN until success or timeout.
    wait_until() {
      local timeout="$1"
      local interval="$2"
      local check_fn="$3"
      local start
      start=$(date +%s)

      if [ -z "$check_fn" ]; then
        echo "usage: wait_until <timeout> <interval> <check_fn>" >&2
        return 1
      fi
      require_positive_int "timeout" "$timeout" || return 1
      require_positive_number "interval" "$interval" || return 1

      while true; do
        if "$check_fn"; then
          return 0
        fi
        if [ $(( $(date +%s) - start )) -ge "$timeout" ]; then
          return 1
        fi
        sleep "$interval"
      done
    }

    # wait_http URL [timeout] [interval]
    # - poll HTTP(S) endpoint until it responds 2xx/3xx or timeout.
    wait_http() {
      local url="$1"
      local timeout="''${2:-30}"
      local interval="''${3:-1}"

      if [ -z "$url" ]; then
        echo "usage: wait_http <url> [timeout] [interval]" >&2
        return 1
      fi

      _wait_http_probe() {
        ${pkgs.curl}/bin/curl -sSf "$url" >/dev/null 2>&1
      }

      wait_until "$timeout" "$interval" _wait_http_probe
    }

    # log_capture LOGFILE -- <command...>
    # - capture stdout/stderr to logfile (set LOG_TEE=1 to also stream to stdout).
    log_capture() {
      local logfile="$1"
      shift || true
      if [ "''${1:-}" = "--" ]; then
        shift
      fi
      if [ -z "$logfile" ] || [ "$#" -eq 0 ]; then
        echo "usage: log_capture <logfile> -- <command...>" >&2
        return 1
      fi
      if [ "''${LOG_TEE:-0}" = "1" ]; then
        "$@" 2>&1 | tee "$logfile"
      else
        "$@" > "$logfile" 2>&1
      fi
    }

    # artifact_dir
    # - return the current CI artifacts dir (CI_ARTIFACTS_DIR or the configured policy root).
    artifact_dir() {
      if [ -n "''${CI_ARTIFACTS_DIR:-}" ]; then
        echo "$CI_ARTIFACTS_DIR"
        return 0
      fi
      echo ${
        pkgs.lib.escapeShellArg (
          if project ? state && project.state ? policy && project.state.policy ? artifactsRoot then
            project.state.policy.artifactsRoot
          else
            "/tmp/ci-artifacts"
        )
      }
      return 0
    }

    # artifact_path NAME
    # - create artifacts dir if needed and echo a full path for NAME.
    artifact_path() {
      local name="$1"
      if [ -z "$name" ]; then
        echo "usage: artifact_path <name>" >&2
        return 1
      fi
      case "$name" in
        */*|*\\*|.|..)
          log_error "artifact name must be a single file name (got '$name')"
          return 1
          ;;
      esac
      local dir
      dir=$(artifact_dir)
      mkdir -p "$dir"
      echo "$dir/$name"
    }

    # run_hook ENV_VAR [args...]
    # - execute the command stored in ENV_VAR.
    run_hook() {
      local var="$1"
      shift || true
      if [ -z "$var" ]; then
        echo "usage: run_hook <ENV_VAR> [args...]" >&2
        return 1
      fi
      local cmd="''${!var:-}"
      if [ -z "$cmd" ]; then
        log_error "Hook not available: $var"
        return 1
      fi
      "$cmd" "$@"
    }

    # has_hook ENV_VAR
    # - check whether a hook variable is exported and non-empty.
    has_hook() {
      local var="$1"
      if [ -z "$var" ]; then
        return 1
      fi
      [ -n "''${!var:-}" ]
    }

    # wait_hook_ok ENV_VAR [timeout] [interval]
    # - poll a hook command until it succeeds (stdout/stderr suppressed).
    wait_hook_ok() {
      local var="$1"
      local timeout="''${2:-60}"
      local interval="''${3:-1}"

      if ! has_hook "$var"; then
        log_error "Hook not available: $var"
        return 1
      fi

      _wait_hook_probe() {
        run_hook "$var" >/dev/null 2>&1
      }

      wait_until "$timeout" "$interval" _wait_hook_probe
    }
  '';
in
{
  inherit
    loadEnv
    loadEnvFile
    loggingPrelude
    hookExports
    commandHelpersScript
    ;
}
