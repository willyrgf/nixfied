{
  pkgs,
}:

let
  kernelPackage = import ../runtime/kernel { inherit pkgs; };
  runtimeDefaults = import ./runtime-defaults.nix;
  jsonRpcProbePlan =
    probeKind:
    pkgs.writeText "nixfied-probe-${probeKind}.json" (
      builtins.toJSON {
        kind = "nixfied-probe-plan";
        version = 1;
        inherit probeKind;
        exportVar = "NIXFIED_PROBE_RESULT";
      }
    );
  jsonRpcResultPresentPlan = jsonRpcProbePlan "jsonrpc-result-present";
  jsonRpcResultHexPlan = jsonRpcProbePlan "jsonrpc-result-hex";
  jsonRpcResultCompactPlan = jsonRpcProbePlan "jsonrpc-result-compact";
  jsonRpcResultBoolFalsePlan = jsonRpcProbePlan "jsonrpc-result-bool-false";

  netcatPkg =
    if pkgs ? netcat then
      pkgs.netcat
    else if pkgs ? netcat-openbsd then
      pkgs.netcat-openbsd
    else
      throw "probe-commands: netcat package is required";
in
rec {
  jsonRpcProbePlans = {
    resultPresent = jsonRpcResultPresentPlan;
    resultHex = jsonRpcResultHexPlan;
    resultCompact = jsonRpcResultCompactPlan;
    resultBoolFalse = jsonRpcResultBoolFalsePlan;
  };

  endpointUrlExpr =
    {
      portExpr,
      scheme ? "http",
      host ? runtimeDefaults.hosts.loopbackIp,
      path ? "",
    }:
    "${scheme}://${host}:${portExpr}${path}";

  localHttpUrlExpr = portExpr: endpointUrlExpr { inherit portExpr; };

  tcpOpenCmd =
    {
      portExpr,
      host ? runtimeDefaults.hosts.loopbackIp,
    }:
    ''
      ${netcatPkg}/bin/nc -z ${host} "${portExpr}" >/dev/null 2>&1
    '';

  httpGetOkCmd =
    {
      urlExpr,
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      ${pkgs.curl}/bin/curl -fsS --max-time ${toString maxTime} "${urlExpr}" >/dev/null 2>&1
    '';

  jsonRpcKernelCmd =
    {
      planFile,
      urlExpr,
      method,
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      ${kernelPackage}/bin/nixfied-kernel probe jsonrpc \
        ${pkgs.lib.escapeShellArg (toString planFile)} \
        ${pkgs.lib.escapeShellArg "${pkgs.curl}/bin/curl"} \
        "${urlExpr}" \
        ${pkgs.lib.escapeShellArg method} \
        ${toString maxTime}
    '';

  jsonRpcHasResultCmd =
    {
      urlExpr,
      method,
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      {
        ${jsonRpcKernelCmd {
          planFile = jsonRpcResultPresentPlan;
          inherit
            urlExpr
            method
            maxTime
            ;
        }}
      } >/dev/null 2>&1
    '';

  jsonRpcResultHexCmd =
    {
      urlExpr,
      method,
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      {
        ${jsonRpcKernelCmd {
          planFile = jsonRpcResultHexPlan;
          inherit
            urlExpr
            method
            maxTime
            ;
        }}
      } 2>/dev/null
    '';

  jsonRpcResultCompactCmd =
    {
      urlExpr,
      method,
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      {
        ${jsonRpcKernelCmd {
          planFile = jsonRpcResultCompactPlan;
          inherit
            urlExpr
            method
            maxTime
            ;
        }}
      } 2>/dev/null
    '';

  jsonRpcResultFalseCmd =
    {
      urlExpr,
      method,
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      {
        ${jsonRpcKernelCmd {
          planFile = jsonRpcResultBoolFalsePlan;
          inherit
            urlExpr
            method
            maxTime
            ;
        }}
      } >/dev/null 2>&1
    '';

  pgIsReadyCmd =
    {
      postgres,
      portExpr,
      host ? runtimeDefaults.hosts.localhost,
      user ? "postgres",
      quiet ? true,
    }:
    ''
      ${postgres}/bin/pg_isready -U ${user} -h ${host} -p "${portExpr}" ${if quiet then "-q" else ""} 2>/dev/null
    '';

  psqlQueryCmd =
    {
      postgres,
      portExpr,
      databaseExpr,
      query,
      host ? runtimeDefaults.hosts.localhost,
      user ? "postgres",
      extraArgs ? "-Atqc",
    }:
    ''
      ${postgres}/bin/psql -h ${host} -p "${portExpr}" -U ${user} -d "${databaseExpr}" ${extraArgs} ${pkgs.lib.escapeShellArg query}
    '';
}
