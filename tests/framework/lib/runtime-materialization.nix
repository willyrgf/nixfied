{
  pkgs,
  model,
  services,
  serviceDefinitions ? null,
  resolvedServices ? null,
}:
let
  frameworkLib = import ../../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  lib = pkgs.lib;
  normalizedResolvedServices =
    if resolvedServices != null then
      resolvedServices
    else
      builtins.listToAttrs (
        map (
          serviceId:
          let
            service = services.${serviceId};
            serviceName =
              if lib.hasPrefix "service." serviceId then
                builtins.substring 8 ((builtins.stringLength serviceId) - 8) serviceId
              else
                service.name or serviceId;
          in
          {
            name = serviceName;
            value =
              if service ? config then
                (service.config or { })
                // {
                  enable = service.enable or false;
                }
              else
                service;
          }
        ) (builtins.attrNames services)
      );

  compiledCore = {
    inherit model;
    serviceSets = model.serviceSets or { };
    resolved = {
      services = if serviceDefinitions != null then serviceDefinitions else normalizedResolvedServices;
    };
    contractBundle = (model.compiled or { }).contractBundle or { };
  };

  materialized = frameworkLib.materializeExecution {
    projectRoot = ../../..;
    compiledCore = compiledCore;
  };
in
{
  serviceHookEnv = materialized.serviceHookEnv;
  inherit (materialized) serviceSetPrograms;
}
