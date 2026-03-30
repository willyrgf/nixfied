{ }:

let
  runtimeLogLevels = [
    "error"
    "warn"
    "info"
    "debug"
    "trace"
  ];

  runtimeOutputModes = [
    "stdout"
    "logs"
    "both"
  ];

  runtimeLogLevelEnvName = "LOG_LEVEL";
  runtimeLogLevelAliases = [ "NIXFIED_LOG_LEVEL" ];
  runtimeLogLevelDefault = "info";

  runtimeOutputModeEnvName = "OUTPUT_MODE";
  runtimeOutputModeAliases = [ "NIXFIED_OUTPUT_MODE" ];
  runtimeOutputModeDefault = "stdout";

  mkRuntimePrimitiveEnvSpecs =
    {
      logLevelDefault ? runtimeLogLevelDefault,
      outputModeDefault ? runtimeOutputModeDefault,
    }:
    [
      {
        name = runtimeLogLevelEnvName;
        type = "enum";
        required = false;
        aliases = runtimeLogLevelAliases;
        values = runtimeLogLevels;
        default = logLevelDefault;
      }
      {
        name = runtimeOutputModeEnvName;
        type = "enum";
        required = false;
        aliases = runtimeOutputModeAliases;
        values = runtimeOutputModes;
        default = outputModeDefault;
      }
    ];

  mkServiceRuntimePrimitivesV1 =
    {
      logLevelDefault ? runtimeLogLevelDefault,
      outputModeDefault ? runtimeOutputModeDefault,
    }:
    let
      _logDefaultCheck =
        if builtins.elem logLevelDefault runtimeLogLevels then
          null
        else
          throw "runtime primitive default invalid: logLevel.default must be one of ${builtins.concatStringsSep "|" runtimeLogLevels}";
      _outputDefaultCheck =
        if builtins.elem outputModeDefault runtimeOutputModes then
          null
        else
          throw "runtime primitive default invalid: outputMode.default must be one of ${builtins.concatStringsSep "|" runtimeOutputModes}";
    in
    builtins.seq _logDefaultCheck (
      builtins.seq _outputDefaultCheck {
        version = 1;
        logLevel = {
          env = runtimeLogLevelEnvName;
          aliases = runtimeLogLevelAliases;
          values = runtimeLogLevels;
          default = logLevelDefault;
        };
        outputMode = {
          env = runtimeOutputModeEnvName;
          aliases = runtimeOutputModeAliases;
          values = runtimeOutputModes;
          default = outputModeDefault;
        };
      }
    );
in
{
  inherit
    runtimeLogLevels
    runtimeOutputModes
    runtimeLogLevelEnvName
    runtimeLogLevelAliases
    runtimeLogLevelDefault
    runtimeOutputModeEnvName
    runtimeOutputModeAliases
    runtimeOutputModeDefault
    mkRuntimePrimitiveEnvSpecs
    mkServiceRuntimePrimitivesV1
    ;
}
