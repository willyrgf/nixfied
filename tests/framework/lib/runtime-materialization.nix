{
  pkgs,
  model,
  services,
}:
let
  serviceSurfaceCatalog = (model.compiled or { }).serviceSurfaceCatalog or { };

  mkRuntimeSurfaces =
    runtimeModel: selectedServices:
    import ../../../nixfied/framework/core/mkServiceRuntimeSurfaces.nix {
      inherit
        pkgs
        serviceSurfaceCatalog
        ;
      model = runtimeModel;
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
      resolvedServices = model.resolvedServices or { };
      serviceRuntimeSurfaces = serviceSetRuntimeSurfaces;
    }
  ) (model.serviceSets or { });
in
{
  serviceHookEnv = baseRuntimeSurfaces.serviceHookEnv;
  inherit serviceSetPrograms;
}
