{
  pkgs,
  projectRoot,
  compiledCore,
}:
let
  inherit (pkgs) lib;
  canonical = import ./canonical.nix { inherit lib; };
  registry = import ../runtime/registry { inherit pkgs; };
  compileServices = import ../../compiler/compile-services.nix { inherit lib; };
  listUtils = import ./list-utils.nix;

  compiledServiceSurfaceCatalog = compiledCore.model.compiled.serviceSurfaceCatalog or { };
  compiledExecution = compiledCore.model.compiled.execution or { };
  serviceDefinitions = compiledCore.resolved.services or { };
  executionServiceNames = listUtils.uniqueSorted (
    builtins.concatLists (
      (map (taskId: (compiledExecution.tasks.byId.${taskId}.closureSelectedServices or [ ])) (
        builtins.attrNames (compiledExecution.tasks.byId or { })
      ))
      ++ (map (
        workflowId: (compiledExecution.workflows.byId.${workflowId}.closureSelectedServices or [ ])
      ) (builtins.attrNames (compiledExecution.workflows.byId or { })))
      ++ (map (serviceSetId: (compiledExecution.serviceSets.byId.${serviceSetId}.allServices or [ ])) (
        builtins.attrNames (compiledExecution.serviceSets.byId or { })
      ))
    )
  );
  runtimeServiceNames = listUtils.uniqueSorted (
    executionServiceNames ++ builtins.attrNames (compiledServiceSurfaceCatalog.serviceApis or { })
  );

  services = compileServices {
    inherit pkgs;
    inherit (compiledCore) resolved;
    selectedServices = runtimeServiceNames;
  };

  runtimeHash = canonical.hashCanonical {
    schema = {
      kind = "nixfied-runtime";
      version = 1;
    };
    inherit services;
  };

  serviceRuntimeSurfaces = import ./mkServiceRuntimeSurfaces.nix {
    inherit
      pkgs
      serviceDefinitions
      services
      ;
    inherit (compiledCore) model;
    serviceApis = compiledServiceSurfaceCatalog.serviceApis or { };
    operationCatalog = compiledServiceSurfaceCatalog.operationCatalog or { };
  };

  serviceDispatcher = import ../runtime/service-dispatcher.nix {
    inherit pkgs;
    serviceSurfaceCatalog = compiledServiceSurfaceCatalog;
    serviceOperationPrograms = serviceRuntimeSurfaces.serviceAppPrograms;
  };

  serviceDispatcherProgram = "${serviceDispatcher}/bin/nixfied-service-dispatcher";

  orchestrator = import ../runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      projectRoot
      runtimeHash
      services
      ;
    inherit (compiledCore) model;
    inherit serviceDispatcherProgram;
    runtimeBin = serviceDispatcherProgram;
  };

  runtimeEngine = import ../runtime/engine.nix {
    inherit pkgs;
    orchestratorProgram = "${orchestrator}/bin/nixfied-orchestrator";
    inherit serviceDispatcherProgram;
  };
in
{
  inherit
    services
    runtimeHash
    orchestrator
    serviceDispatcher
    runtimeEngine
    ;
  serviceApis = compiledServiceSurfaceCatalog.serviceApis or { };
  serviceOperationPrograms = serviceRuntimeSurfaces.serviceAppPrograms;
}
