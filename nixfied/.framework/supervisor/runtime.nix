# Shared supervisor runtime setup and wrapped script builders
{
  pkgs,
  project,
  slots,
  config,
  loggingPrelude,
}:

let
  serviceScripts = import ../lib/managed-service-lifecycle.nix { inherit pkgs; };
  slotEnvRuntime = import ../lib/slot-env-runtime.nix { inherit pkgs; };
  ports = project.ports or { };
  portNames = builtins.attrNames ports;

  slotPrelude = ''
    ${slotEnvRuntime.loadJsonFromCommand {
      outVar = "SLOT_INFO_JSON_OUT";
      command = toString slots.getSlotInfoJson;
      exportVars = false;
    }}
    ${slotEnvRuntime.readJsonField {
      targetVar = "RUN_DIR";
      jsonVar = "SLOT_INFO_JSON_OUT";
      jqExpr = ".directories.run";
    }}
  '';

  logDirPrelude = ''
    ${slotEnvRuntime.readJsonField {
      targetVar = "LOG_DIR";
      jsonVar = "SLOT_INFO_JSON_OUT";
      jqExpr = ".directories.log";
    }}
  '';

  portsPrelude = pkgs.lib.concatMapStringsSep "\n" (
    name:
    let
      portVar = slots.portVarName name;
    in
    ''
      ${slotEnvRuntime.readPortFromJson {
        targetVar = portVar;
        jsonVar = "SLOT_INFO_JSON_OUT";
        keyExpr = portVar;
      }}
    ''
  ) portNames;

  socketPrelude = ''
    SOCKET_HASH=$(printf '%s' "$RUN_DIR" | ${pkgs.coreutils}/bin/cksum | ${pkgs.coreutils}/bin/cut -d ' ' -f1)
    export PC_SOCKET_PATH="/tmp/nixfied-pc-$SOCKET_HASH.sock"
  '';

  configPrelude = ''
    CONFIG_FILE=$(${config.generateConfig})
    export PC_CONFIG_FILES="$CONFIG_FILE"
    export PC_DISABLE_TUI=1
  '';

  mkSupervisorScript =
    {
      name,
      body,
      includeLogDir ? false,
      includePorts ? false,
      includeConfig ? false,
      useLoggingPrelude ? true,
    }:
    serviceScripts.mkWrappedScript {
      inherit name body;
      loggingPrelude = if useLoggingPrelude then loggingPrelude else "";
      runtimePrelude =
        slotPrelude
        + pkgs.lib.optionalString includeLogDir logDirPrelude
        + pkgs.lib.optionalString includePorts portsPrelude
        + socketPrelude
        + pkgs.lib.optionalString includeConfig configPrelude;
    };
in
{
  inherit
    mkSupervisorScript
    slotPrelude
    logDirPrelude
    portsPrelude
    socketPrelude
    configPrelude
    ;
}
