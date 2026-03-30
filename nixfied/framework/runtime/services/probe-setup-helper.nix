# Probe setup helper -- extracts the common probe-wiring imports and
# renderPlanBody / renderProbeStep / healthPlanBody / readyPlanBody
# that every service lifecycle.nix duplicates.
#
# Usage:
#   probeSetup = import ../probe-setup-helper.nix {
#     inherit pkgs project slots config;
#     serviceName = "reth";
#     endpointMapping = {
#       http  = "$RETH_HTTP_PORT";
#       ws    = "$RETH_WS_PORT";
#       auth  = "$RETH_AUTH_PORT";
#     };
#   };
{
  pkgs,
  project,
  slots,
  config,
  serviceName,
  endpointMapping,
}:

let
  lib = pkgs.lib;
  runtimeDefaults = import ../../core/runtime-defaults.nix;
  managedServiceLifecycle = import ../helpers/managed-service-lifecycle.nix { inherit pkgs; };
  probeCommands = import ../../core/probe-commands.nix { inherit pkgs; };
  probePlanRuntime = import ../../core/probe-plan-runtime.nix {
    inherit
      lib
      pkgs
      probeCommands
      ;
    postgresProbePkg = if pkgs ? postgresql_16 then pkgs.postgresql_16 else pkgs.postgresql;
  };
  slotEnvRuntime = import ../../core/slot-env-runtime.nix { inherit pkgs; };
  runtimeEvents = import ../../core/runtime-events.nix { inherit pkgs project; };
  observability = import ../../core/service-observability.nix {
    inherit
      pkgs
      slots
      runtimeEvents
      ;
  };

  serviceSource = if (config.defaultSource or "") == "" then "unspecified" else config.defaultSource;

  healthPlan = config.probePlans.health or { steps = [ ]; };
  readyPlan =
    config.probePlans.ready or {
      steps = [ ];
      wait = null;
    };

  portExprForEndpoint =
    endpointName:
    if builtins.hasAttr endpointName endpointMapping then
      endpointMapping.${endpointName}
    else
      throw "${serviceName} lifecycle: unsupported probe endpoint '${endpointName}'";

  renderPlanBody =
    mode: plan:
    probePlanRuntime.renderPlanBody {
      inherit
        mode
        plan
        serviceName
        portExprForEndpoint
        ;
      endpoints = config.resolvedEndpoints or { };
    };

  renderProbeStep =
    mode: step:
    probePlanRuntime.renderProbeStep {
      inherit
        mode
        step
        serviceName
        portExprForEndpoint
        ;
      endpoints = config.resolvedEndpoints or { };
    };

  healthPlanBody = renderPlanBody "health" healthPlan;
  readyPlanBody = renderPlanBody "ready" readyPlan;
in
{
  inherit
    lib
    runtimeDefaults
    managedServiceLifecycle
    probeCommands
    probePlanRuntime
    slotEnvRuntime
    runtimeEvents
    observability
    serviceSource
    healthPlan
    readyPlan
    renderPlanBody
    renderProbeStep
    healthPlanBody
    readyPlanBody
    portExprForEndpoint
    ;
}
