# Service runtime prelude builder -- extracts the repeated slot-info JSON
# loading, port reading, and SERVICE_DIR / SERVICE_PID_FILE / SERVICE_LOG_FILE
# wiring shared across all pid-file managed services.
#
# Usage:
#   prelude = import ../service-runtime-prelude.nix {
#     inherit slotEnvRuntime slots observability;
#     serviceName = "reth";
#     serviceNameUpper = "RETH";
#     portVars = [
#       { varName = "HTTP_PORT_VAR"; portVar = httpPortVar; target = "RETH_HTTP_PORT"; }
#       { varName = "WS_PORT_VAR";   portVar = wsPortVar;   target = "RETH_WS_PORT"; }
#     ];
#     dirExpr = rethDirExpr;
#     pidFileName = "reth.pid";
#     logFileName = "reth.log";
#     portValidation = ''
#       if [ -z "$RETH_HTTP_PORT" ]; then ... fi
#     '';
#     extraPrelude = ''...'';
#   };
{
  slotEnvRuntime,
  slots,
  observability,
  serviceName,
  serviceNameUpper,
  portVars,
  dirExpr,
  pidFileName ? "${serviceName}.pid",
  logFileName ? "${serviceName}.log",
  portValidation ? "",
  extraPrelude ? "",
}:

let
  slotJsonLoad = slotEnvRuntime.loadJsonFromCommand {
    outVar = "SLOT_INFO_JSON_OUT";
    command = toString slots.getSlotInfo;
    exportVars = false;
  };

  portVarDecls = builtins.concatStringsSep "\n" (
    map (pv: ''${pv.varName}="${pv.portVar}"'') portVars
  );

  portReads = builtins.concatStringsSep "\n" (
    map (
      pv:
      slotEnvRuntime.readPortFromJson {
        targetVar = pv.target;
        jsonVar = "SLOT_INFO_JSON_OUT";
        keyExpr = "\$${pv.varName}";
      }
    ) portVars
  );

  svcDir = "${serviceNameUpper}_DIR";
  svcPidFile = "${serviceNameUpper}_PID_FILE";
  svcLogFile = "${serviceNameUpper}_LOG_FILE";
in
''
  ${slotJsonLoad}

  ${portVarDecls}

  ${portReads}
  ${svcDir}="${dirExpr}"
  ${svcPidFile}="$${svcDir}/run/${pidFileName}"
  ${svcLogFile}="$${svcDir}/logs/${logFileName}"
  SERVICE_DIR="$${svcDir}"
  SERVICE_PID_FILE="$${svcPidFile}"
  SERVICE_LOG_FILE="$${svcLogFile}"

  ${portValidation}

  ${observability.mkEmitServiceEventFunction serviceName}

  ${extraPrelude}
''
