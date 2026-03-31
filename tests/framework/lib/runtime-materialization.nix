{
  pkgs,
  model,
  services,
  serviceDefinitions ? null,
  resolvedServices ? null,
}:
let
  lib = pkgs.lib;
  serviceSurfaceCatalog = (model.compiled or { }).serviceSurfaceCatalog or { };
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
  normalizedServiceDefinitions =
    if serviceDefinitions != null then
      serviceDefinitions
    else if resolvedServices != null then
      resolvedServices
    else
      { };

  mkRuntimeSurfaces =
    runtimeModel: selectedServices:
    import ../../../nixfied/framework/core/mkServiceRuntimeSurfaces.nix {
      inherit
        pkgs
        serviceSurfaceCatalog
        ;
      model = runtimeModel;
      serviceDefinitions = normalizedServiceDefinitions;
      inherit
        services
        selectedServices
        ;
    };

  baseRuntimeSurfaces = mkRuntimeSurfaces model null;

  serviceSetPrograms = builtins.mapAttrs (
    _: serviceSet:
    let
      serviceSetModel = model // {
        runtime = model.runtime // {
          directories = (model.runtime.directories or { }) // {
            base = serviceSet.state.policy.runtimeBase;
          };
        };
        state = model.state // {
          policy = serviceSet.state.policy;
        };
      };
      serviceSetRuntimeSurfaces = mkRuntimeSurfaces serviceSetModel (serviceSet.services.all or [ ]);
    in
    import ../../../nixfied/framework/core/mkServiceSetPrograms.nix {
      inherit
        pkgs
        serviceSet
        ;
      model = serviceSetModel;
      resolvedServices = normalizedResolvedServices;
      serviceRuntimeSurfaces = serviceSetRuntimeSurfaces;
    }
  ) (model.serviceSets or { });
in
{
  serviceHookEnv = baseRuntimeSurfaces.serviceHookEnv;
  inherit serviceSetPrograms;
}
