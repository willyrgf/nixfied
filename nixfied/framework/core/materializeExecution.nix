{
  pkgs,
  projectRoot,
  compiledCore,
  selectedServices ? null,
  frameworkSourceFlakeRef ? null,
}:
let
  lib = pkgs.lib;
  canonical = import ./canonical.nix { inherit lib; };
  registry = import ../runtime/registry { inherit pkgs; };
  compileServices = import ../../compiler/compile-services.nix { inherit lib; };

  enabledServiceFlags = builtins.listToAttrs (
    map (
      serviceId:
      let
        service = compiledCore.model.serviceCatalog.${serviceId};
      in
      {
        name = service.name or (lib.removePrefix "service." serviceId);
        value = service.enable or false;
      }
    ) (builtins.attrNames compiledCore.model.serviceCatalog)
  );

  services = compileServices {
    inherit
      pkgs
      selectedServices
      enabledServiceFlags
      ;
    resolved = compiledCore.resolved;
  };

  runtimeHash = canonical.hashCanonical {
    schema = {
      kind = "nixfied-runtime";
      version = 1;
    };
    services = services;
  };

  serviceRuntimeSurfaces = import ./mkServiceRuntimeSurfaces.nix {
    inherit
      pkgs
      selectedServices
      ;
    model = compiledCore.model;
    services = services;
  };

  taskAppPrograms = builtins.mapAttrs (
    appId: manifest:
    let
      appServices = compileServices {
        inherit
          pkgs
          enabledServiceFlags
          ;
        resolved = compiledCore.resolved;
        selectedServices = manifest.selectedServices;
      };

      appRuntimeHash = canonical.hashCanonical {
        schema = {
          kind = "nixfied-runtime";
          version = 1;
        };
        services = appServices;
      };

      appServiceRuntimeSurfaces = import ./mkServiceRuntimeSurfaces.nix {
        inherit
          pkgs
          ;
        model = manifest.model;
        selectedServices = manifest.selectedServices;
        services = appServices;
      };

      appOrchestrator = import ../runtime/orchestrator.nix {
        inherit
          pkgs
          registry
          projectRoot
          ;
        model = manifest.model;
        services = appServices;
        runtimeHash = appRuntimeHash;
        serviceHookEnv = appServiceRuntimeSurfaces.serviceHookEnv;
      };
    in
    "${appOrchestrator}/bin/nixfied-orchestrator"
  ) (compiledCore.appExecutionManifests or { });

  runner = import ../runtime {
    inherit
      pkgs
      projectRoot
      registry
      ;
  };

  baseApps = runner.mkApps {
    model = compiledCore.model;
    selectionIndex = compiledCore.selectionIndex;
    services = services;
    inherit
      runtimeHash
      frameworkSourceFlakeRef
      ;
    taskAppPrograms = taskAppPrograms;
    serviceApps = serviceRuntimeSurfaces.serviceApps;
    serviceHookEnv = serviceRuntimeSurfaces.serviceHookEnv;
  };
in
{
  inherit
    services
    runtimeHash
    baseApps
    ;
  serviceApis = serviceRuntimeSurfaces.serviceApis;
  serviceApps = serviceRuntimeSurfaces.serviceApps;
  serviceHookEnv = serviceRuntimeSurfaces.serviceHookEnv;
}
