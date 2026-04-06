{
  lib,
  pkgs,
  probeCommands,
}:

let
  runtimeDefaults = import ./runtime-defaults.nix;
  kernelPackage = import ../runtime/kernel { inherit pkgs; };

  tokenLib = import ./normalize-token.nix { inherit lib; };
  normalizeEnvToken = tokenLib.normalizeToken;

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
      base = { inherit (step) kind serviceLabel phaseLabel successLabel failureLabel; };
      portEnvVarFor = endpointPortEnvName;
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
        inherit (step) path;
        portEnvVar = portEnvVarFor step.endpoint;
        maxTimeSeconds = runtimeDefaults.probes.httpMaxTimeSeconds;
      }
    else if step.kind == "jsonrpc" then
      base
      // {
        host = hostDefault;
        scheme = endpointProtocol endpoints step.endpoint;
        inherit (step) method;
        portEnvVar = portEnvVarFor step.endpoint;
        maxTimeSeconds = runtimeDefaults.probes.httpMaxTimeSeconds;
      }
    else if step.kind == "exec" then
      base
      // {
        inherit (step) command;
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
        builtins.toJSON (mkProbeExecutionPlan {
          inherit
            mode
            serviceName
            plan
            endpoints
            ;
        })
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
        (
          probe_output=""
          probe_rc=0
          set +e
          probe_output="$(${kernelPackage}/bin/nixfied-kernel probe evaluate ${lib.escapeShellArg (toString planFile)} 2>&1)"
          probe_rc="$?"
          set -e
          if [ -n "$probe_output" ]; then
            printf '%s\n' "$probe_output"
          fi
          exit "$probe_rc"
        )
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
