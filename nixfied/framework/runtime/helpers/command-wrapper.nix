{
  pkgs,
  project,
  shellContract,
  loadEnv,
  helpersScript,
  hookExports,
}:

let
  inherit (pkgs.lib) concatMapStringsSep makeBinPath;

  runtimePackages = project.tooling.runtimePackages or [ ];
  runtimePath = if runtimePackages == [ ] then "" else makeBinPath runtimePackages;
  logging = project.logging or { };
  defaultLogLevel = toString (logging.level or "info");
  defaultOutputMode = toString (logging.output or "stdout");

  mkCommandWrappedScript =
    {
      name,
      script,
      env ? { },
      commandApi ? null,
      beforeContract ? "",
    }:
    let
      envExports = concatMapStringsSep "\n" (key: "export ${key}=${toString env.${key}}") (
        builtins.attrNames env
      );
      pathBlock = if runtimePath != "" then "export PATH=\"${runtimePath}:$PATH\"" else "";
      contractFile =
        if commandApi == null then
          null
        else
          pkgs.writeText "${name}-command-api.json" (builtins.toJSON commandApi);
      contractRuntime =
        if commandApi == null then
          null
        else
          shellContract.mkContractRuntime {
            inherit name;
            contract = commandApi;
            logLevelDefault = defaultLogLevel;
            outputModeDefault = defaultOutputMode;
          };
      contractPrelude =
        if commandApi == null then
          ""
        else
          ''
            NIXFIED_COMMAND_API_FILE="${toString contractFile}"
            NIXFIED_COMMAND_API_RUNTIME="${toString contractRuntime}"
            nixfied_contract_validate_env "$NIXFIED_COMMAND_API_FILE"
            nixfied_contract_validate_args "$NIXFIED_COMMAND_API_FILE" "$@"
          '';
      contractExitCheck =
        if commandApi == null then
          ""
        else
          ''
            _NIXFIED_CONTRACT_RC=0
            nixfied_contract_validate_exit "$NIXFIED_COMMAND_API_FILE" "$_NIXFIED_APP_RC" || _NIXFIED_CONTRACT_RC=$?
            if [ "$_NIXFIED_CONTRACT_RC" -ne 0 ]; then
              exit "$_NIXFIED_CONTRACT_RC"
            fi
          '';
    in
    pkgs.writeShellScript name ''
      set -euo pipefail
      ${pathBlock}
      export COMMAND_NAME="${name}"
      cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
      source ${loadEnv}
      source ${helpersScript}
      source ${toString shellContract.runtime}
      ${hookExports}
      ${envExports}
      export NIXFIED_LOG_TRACE="''${NIXFIED_LOG_TRACE:-0}"
      export COMMAND_NAME="''${COMMAND_NAME:-${name}}"
      if [ "''${OUTPUT_MODE}" != "stdout" ]; then
        if [ -z "''${NIXFIED_LOG_FILE:-}" ]; then
          _nixfied_slot="''${NIX_ENV:-0}"
          _nixfied_env="''${PROJECT_ENV:-default}"
          _nixfied_cmd="''${COMMAND_NAME:-app}"
          _nixfied_ts="$(date +%Y%m%dT%H%M%S)"
          NIXFIED_LOG_FILE="''${LOG_DIR:-/tmp}/nixfied-''${_nixfied_cmd}-''${_nixfied_env}-slot''${_nixfied_slot}-''${_nixfied_ts}-$$.log"
        fi
        export NIXFIED_LOG_FILE
        mkdir -p "$(dirname "$NIXFIED_LOG_FILE")" 2>/dev/null || true
      fi
      ${beforeContract}
      ${contractPrelude}
      _NIXFIED_APP_RC=0
      (
        set -euo pipefail
        export COMMAND_NAME="''${COMMAND_NAME:-${name}}"
        export NIXFIED_CLEANUP_OWNER_BASHPID="''${BASHPID:-}"
        if [ "''${LOG_LEVEL:-}" = "trace" ] && [ "''${NIXFIED_LOG_TRACE:-0}" = "1" ]; then
          if [ -z "''${NIXFIED_XTRACE_FILE:-}" ]; then
            NIXFIED_XTRACE_FILE="''${NIXFIED_LOG_FILE:-''${LOG_DIR:-/tmp}/nixfied-trace-''${COMMAND_NAME:-app}-''${PROJECT_ENV:-default}-slot''${NIX_ENV:-0}-$$.log}"
            export NIXFIED_XTRACE_FILE
          fi
          mkdir -p "$(dirname "$NIXFIED_XTRACE_FILE")" 2>/dev/null || true
          exec 19>>"$NIXFIED_XTRACE_FILE"
          export BASH_XTRACEFD=19
          set -x
        fi
        ${script}
      ) || _NIXFIED_APP_RC=$?
      ${contractExitCheck}
      exit "$_NIXFIED_APP_RC"
    '';
in
{
  inherit mkCommandWrappedScript;
}
