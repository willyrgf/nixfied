{
  lib,
  pkgs,
  probeCommands,
  postgresProbePkg,
}:

let
  runtimeDefaults = import ../../core/runtime-defaults.nix;
  kernelPackage = import ../kernel { inherit pkgs; };

  normalizeEnvToken =
    value: lib.toUpper (lib.replaceStrings [ "-" "." ":" "/" " " ] [ "_" "_" "_" "_" "_" ] value);

  endpointProtocol =
    endpoints: endpointName:
    let
      endpoint = endpoints.${endpointName} or { };
      protocol = endpoint.protocol or "http";
    in
    if protocol == "https" then "https" else "http";

  endpointPortEnvName = endpointName: "NIXFIED_PROBE_${normalizeEnvToken endpointName}_PORT";

  mkExecEnvBlock =
    {
      mode,
      serviceName,
      endpoints,
      portExprForEndpoint,
    }:
    let
      endpointNames = builtins.sort builtins.lessThan (builtins.attrNames endpoints);
      endpointExports = builtins.concatStringsSep "\n" (
        map (
          endpointName:
          let
            envName = endpointPortEnvName endpointName;
          in
          ''
            export ${envName}="${portExprForEndpoint endpointName}"
          ''
        ) endpointNames
      );
    in
    ''
      export NIXFIED_PROBE_MODE=${lib.escapeShellArg mode}
      export NIXFIED_PROBE_SERVICE=${lib.escapeShellArg serviceName}
      export NIXFIED_PROBE_SOURCE="''${service_source:-unspecified}"
      ${endpointExports}
    '';

  mkStepSpec =
    {
      endpoints,
      step,
    }:
    let
      base = {
        kind = step.kind;
        serviceLabel = step.serviceLabel;
        phaseLabel = step.phaseLabel;
        successLabel = step.successLabel;
        failureLabel = step.failureLabel;
      };
      portEnvVarFor = endpointName: endpointPortEnvName endpointName;
      hostDefault = runtimeDefaults.hosts.loopbackIp;
    in
    if step.kind == "tcp" then
      base
      // {
        host = hostDefault;
        portEnvVar = portEnvVarFor step.endpoint;
      }
    else if step.kind == "http" then
      base
      // {
        host = hostDefault;
        scheme = endpointProtocol endpoints step.endpoint;
        path = step.path;
        portEnvVar = portEnvVarFor step.endpoint;
        maxTimeSeconds = runtimeDefaults.probes.httpMaxTimeSeconds;
      }
    else if step.kind == "jsonrpc" then
      base
      // {
        host = hostDefault;
        scheme = endpointProtocol endpoints step.endpoint;
        method = step.method;
        portEnvVar = portEnvVarFor step.endpoint;
        maxTimeSeconds = runtimeDefaults.probes.httpMaxTimeSeconds;
      }
    else if step.kind == "postgres-pg-isready" then
      base
      // {
        host = step.host or hostDefault;
        portEnvVar = portEnvVarFor step.endpoint;
        failureSuffix = step.failureSuffix or "";
      }
    else if step.kind == "postgres-query" then
      base
      // {
        host = step.host or hostDefault;
        portEnvVar = portEnvVarFor step.endpoint;
        database = step.database;
        query = step.query;
        failureSuffix = step.failureSuffix or "";
      }
    else if step.kind == "helios-ready" then
      base
      // {
        host = hostDefault;
        portEnvVar = portEnvVarFor step.endpoint;
        executionPortEnvVar = portEnvVarFor step.executionEndpoint;
        sourceKinds = step.sourceKinds or { };
        readinessProfile = step.readinessProfile or "fast";
        requireNotSyncing = step.requireNotSyncing or false;
        allowLocalHealthFallback = step.allowLocalHealthFallback or false;
        disallowSourceKinds = step.disallowSourceKinds or [ ];
        maxTimeSeconds = runtimeDefaults.probes.httpMaxTimeSeconds;
      }
    else if step.kind == "exec" then
      base
      // {
        command = step.command;
      }
    else
      throw "probe-plan-runtime: unsupported probe kind '${step.kind}' for service '${base.serviceLabel}'";

  mkProbeExecutionPlan =
    {
      mode,
      serviceName,
      plan,
      endpoints,
    }:
    {
      kind = "nixfied-probe-execution-plan";
      version = 1;
      inherit
        mode
        serviceName
        ;
      sourceEnvVar = "NIXFIED_PROBE_SOURCE";
      curlBin = "${pkgs.curl}/bin/curl";
      runtimeShellBin = "${pkgs.runtimeShell}";
      pgIsReadyBin = "${postgresProbePkg}/bin/pg_isready";
      psqlBin = "${postgresProbePkg}/bin/psql";
      steps = map (
        step:
        mkStepSpec {
          inherit
            endpoints
            step
            ;
        }
      ) (plan.steps or [ ]);
    };

  renderExecutionBody =
    {
      mode,
      serviceName,
      plan,
      endpoints,
      portExprForEndpoint,
    }:
    let
      steps = plan.steps or [ ];
      planFile = pkgs.writeText "nixfied-probe-${serviceName}-${mode}.json" (
        builtins.toJSON (
          mkProbeExecutionPlan {
            inherit
              mode
              serviceName
              plan
              endpoints
              ;
          }
        )
      );
    in
    if steps == [ ] then
      ""
    else
      ''
        ${mkExecEnvBlock {
          inherit
            mode
            serviceName
            endpoints
            portExprForEndpoint
            ;
        }}
        ${kernelPackage}/bin/nixfied-kernel probe evaluate ${lib.escapeShellArg (toString planFile)}
      '';
in
{
  renderProbeStep =
    {
      mode,
      serviceName,
      step,
      endpoints,
      portExprForEndpoint,
    }:
    renderExecutionBody {
      inherit
        mode
        serviceName
        endpoints
        portExprForEndpoint
        ;
      plan = {
        steps = [ step ];
      };
    };

  renderPlanBody =
    {
      mode,
      serviceName,
      plan,
      endpoints,
      portExprForEndpoint,
    }:
    renderExecutionBody {
      inherit
        mode
        serviceName
        plan
        endpoints
        portExprForEndpoint
        ;
    };
}
