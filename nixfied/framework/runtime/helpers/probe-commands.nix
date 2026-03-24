{
  pkgs,
}:

let
  kernelPackage = import ../kernel { inherit pkgs; };
  runtimeDefaults = import ../../core/runtime-defaults.nix;
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

  jsonRpcPayload =
    {
      method,
      params ? [ ],
    }:
    builtins.toJSON {
      jsonrpc = "2.0";
      id = 1;
      inherit
        method
        params
        ;
    };
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

  jsonRpcRequestCmd =
    {
      urlExpr,
      method,
      params ? [ ],
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      ${pkgs.curl}/bin/curl -fsS --max-time ${toString maxTime} \
        -H 'content-type: application/json' \
        --data '${jsonRpcPayload { inherit method params; }}' \
        "${urlExpr}"
    '';

  jsonRpcHasResultCmd =
    {
      urlExpr,
      method,
      params ? [ ],
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      (
        tmp_json="''${TMPDIR:-/tmp}/nixfied-probe-jsonrpc.$$.$RANDOM.json"
        (
          ${jsonRpcRequestCmd {
            inherit
              urlExpr
              method
              params
              maxTime
            ;
          }}
        ) > "$tmp_json"
        ${kernelPackage}/bin/nixfied-kernel probe evaluate ${pkgs.lib.escapeShellArg (toString jsonRpcResultPresentPlan)} "$tmp_json" >/dev/null
        rm -f "$tmp_json"
      )
    '';

  jsonRpcResultHexCmd =
    {
      urlExpr,
      method,
      params ? [ ],
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      (
        tmp_json="''${TMPDIR:-/tmp}/nixfied-probe-jsonrpc.$$.$RANDOM.json"
        export_file="''${TMPDIR:-/tmp}/nixfied-probe-jsonrpc-export.$$.$RANDOM.sh"
        (
          ${jsonRpcRequestCmd {
            inherit
              urlExpr
              method
              params
              maxTime
              ;
          }}
        ) > "$tmp_json"
        ${kernelPackage}/bin/nixfied-kernel probe evaluate \
          ${pkgs.lib.escapeShellArg (toString jsonRpcResultHexPlan)} \
          "$tmp_json" \
          "$export_file" >/dev/null
        . "$export_file"
        printf '%s' "''${NIXFIED_PROBE_RESULT:-}"
        rm -f "$tmp_json" "$export_file"
      )
    '';

  jsonRpcResultFalseCmd =
    {
      urlExpr,
      method,
      params ? [ ],
      maxTime ? runtimeDefaults.probes.httpMaxTimeSeconds,
    }:
    ''
      (
        tmp_json="''${TMPDIR:-/tmp}/nixfied-probe-jsonrpc.$$.$RANDOM.json"
        (
          ${jsonRpcRequestCmd {
            inherit
              urlExpr
              method
              params
              maxTime
              ;
          }}
        ) > "$tmp_json"
        ${kernelPackage}/bin/nixfied-kernel probe evaluate ${pkgs.lib.escapeShellArg (toString jsonRpcResultBoolFalsePlan)} "$tmp_json" >/dev/null
        rm -f "$tmp_json"
      )
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
