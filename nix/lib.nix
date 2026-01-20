# Framework helpers for building Nix apps
{ pkgs, project }:

let
  inherit (pkgs.lib) concatMapStringsSep makeBinPath;

  runtimePackages = project.tooling.runtimePackages or [ ];
  runtimePath =
    if runtimePackages == [ ] then
      ""
    else
      makeBinPath runtimePackages;

  # Script to load .env if it exists (does not override existing env vars)
  loadEnv = pkgs.writeShellScript "load-env" ''
    if [ -f ".env" ]; then
      while IFS='=' read -r key value || [ -n "$key" ]; do
        case "$key" in
          \#*|"") continue ;;
        esac
        value=$(echo "$value" | sed -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//")
        if [ -z "''${!key:-}" ]; then
          export "$key=$value"
        fi
      done < .env
    fi
  '';

  # Timing wrapper - records and displays execution time
  withTiming = name: script: ''
    _TIMING_START=$(date +%s)
    _TIMING_EXIT=0

    (
      set -euo pipefail
      ${script}
    ) || _TIMING_EXIT=$?

    _TIMING_END=$(date +%s)
    _TIMING_DURATION=$((_TIMING_END - _TIMING_START))

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    if [ $_TIMING_DURATION -lt 60 ]; then
      echo "⏱️  ${name} completed in ''${_TIMING_DURATION}s"
    else
      _TIMING_MINS=$((_TIMING_DURATION / 60))
      _TIMING_SECS=$((_TIMING_DURATION % 60))
      echo "⏱️  ${name} completed in ''${_TIMING_MINS}m ''${_TIMING_SECS}s"
    fi
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    exit $_TIMING_EXIT
  '';

  # Helper to create app scripts
  mkApp =
    {
      name,
      script,
      env ? { },
      useDeps ? false,
      description ? null,
    }:
    let
      envExports = concatMapStringsSep "\n" (
        key: "export ${key}=${toString env.${key}}"
      ) (builtins.attrNames env);
      depsScript = project.install.deps or "";
      depsBlock =
        if useDeps && depsScript != "" then
          depsScript
        else
          "";
      pathBlock =
        if runtimePath != "" then
          "export PATH=\"${runtimePath}:$PATH\""
        else
          "";
    in
    {
      type = "app";
      meta = pkgs.lib.optionalAttrs (description != null) {
        inherit description;
      };
      program = toString (
        pkgs.writeShellScript name ''
          set -euo pipefail
          ${pathBlock}
          export COMMAND_NAME="${name}"
          cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
          source ${loadEnv}
          ${envExports}
          ${depsBlock}
          ${script}
        ''
      );
    };

  mkAppWithDeps = args: mkApp (args // { useDeps = true; });

  # Process management utilities
  mkSignalHandler =
    cleanupHook:
    pkgs.writeShellScript "signal-handler" ''
      shutdown() {
        echo ""
        echo "🛑 Shutting down..."
        ${cleanupHook}
        exit 0
      }

      trap shutdown SIGINT SIGTERM

      cleanup() {
        shutdown "$@"
      }
    '';

  mkProcessManager =
    {
      processName ? "",
      startupScript,
      cleanupHook ? "",
    }:
    pkgs.writeShellScript "process-manager" ''
      ${mkSignalHandler cleanupHook}

      echo "🚀 Starting ${processName}..."
      ${startupScript}
      wait
    '';

  # Port management utilities
  mkPortCleanup = pkgs.writeShellScript "port-cleanup" ''
    cleanup_port() {
      local port=$1
      local name=$2

      echo "🧹 Cleaning $name processes on port $port..."

      if command -v lsof >/dev/null 2>&1; then
        PIDS=$(lsof -ti:$port 2>/dev/null || true)
        if [ -n "$PIDS" ]; then
          echo "   Found processes: $PIDS"
          echo "$PIDS" | xargs kill -TERM 2>/dev/null || true
          sleep 2
          echo "$PIDS" | xargs kill -KILL 2>/dev/null || true
        fi
      else
        ${pkgs.procps}/bin/netstat -tlnp 2>/dev/null | grep ":$port " | awk '/LISTEN/ {print $7}' | cut -d'/' -f1 | xargs kill -TERM 2>/dev/null || true
        sleep 2
        ${pkgs.procps}/bin/netstat -tlnp 2>/dev/null | grep ":$port " | awk '/LISTEN/ {print $7}' | cut -d'/' -f1 | xargs kill -KILL 2>/dev/null || true
      fi
    }

    for port in "$@"; do
      cleanup_port "$port" "service"
    done
  '';

  mkPortConflictChecker = pkgs.writeShellScript "port-conflict-checker" ''
    check_port() {
      local port=$1
      local name=$2

      if command -v lsof >/dev/null 2>&1; then
        if lsof -i:$port >/dev/null 2>&1; then
          echo "❌ Port $port ($name) is already in use"
          return 1
        fi
      else
        if ${pkgs.procps}/bin/netstat -tln 2>/dev/null | grep ":$port " >/dev/null; then
          echo "❌ Port $port ($name) is already in use"
          return 1
        fi
      fi
      echo "✅ Port $port ($name) is available"
      return 0
    }

    CONFLICT=false
    for port in "$@"; do
      if ! check_port "$port" "service"; then
        CONFLICT=true
      fi
    done

    if [ "$CONFLICT" = "true" ]; then
      exit 1
    fi

    exit 0
  '';

  # Parallel execution utility
  mkParallelRunner =
    commands:
    pkgs.writeShellScript "parallel-runner" ''
      declare -a OUTPUT_FILES
      declare -a PIDS

      i=0
      ${pkgs.lib.concatMapStringsSep "\n" (cmd: ''
        OUTPUT_FILE=$(mktemp)
        OUTPUT_FILES[$i]=$OUTPUT_FILE

        echo "🚀 Starting command $((i+1)): ${cmd}"
        (
          eval "${cmd}" 2>&1
          echo $? > "$OUTPUT_FILE.exit"
        ) > "$OUTPUT_FILE" 2>&1 &

        PIDS[$i]=$!
        i=$((i+1))
      '') commands}

      i=0
      EXIT_CODE=0
      for pid in "''${PIDS[@]}"; do
        wait $pid
        CMD_EXIT=$(cat "''${OUTPUT_FILES[$i]}.exit" 2>/dev/null || echo "1")
        if [ "$CMD_EXIT" -ne 0 ]; then
          EXIT_CODE=$CMD_EXIT
        fi
        i=$((i+1))
      done

      i=0
      for cmd in ${pkgs.lib.concatMapStringsSep " " (c: "\"${c}\"") commands}; do
        echo ""
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo "📋 Command $((i+1)) output:"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        cat "''${OUTPUT_FILES[$i]}"
        i=$((i+1))
      done

      i=0
      for output_file in "''${OUTPUT_FILES[@]}"; do
        rm -f "$output_file" "$output_file.exit"
      done

      exit $EXIT_CODE
    '';
in
{
  inherit
    loadEnv
    withTiming
    mkApp
    mkAppWithDeps
    mkSignalHandler
    mkProcessManager
    mkPortCleanup
    mkPortConflictChecker
    mkParallelRunner
    ;
}
