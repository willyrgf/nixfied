{
  lib,
  canonical,
}:
{
  resolvedIdentity,
  runtime,
  state,
  serviceCatalog,
  apps,
  tasks,
  workflows,
  selectionIndex,
  serviceSets ? { },
}:
let
  appExecutionManifests = import ./compile-app-execution-manifests.nix {
    inherit
      lib
      canonical
      ;
    inherit
      resolvedIdentity
      runtime
      state
      serviceCatalog
      apps
      tasks
      workflows
      selectionIndex
      ;
  };

  manifestIds = builtins.sort builtins.lessThan (builtins.attrNames appExecutionManifests);
  manifestsByServiceSet = builtins.mapAttrs (_name: serviceSet: {
    id = serviceSet.id or _name;
    name = serviceSet.name or _name;
    summary = serviceSet.summary or "";
    services = serviceSet.services or { };
    requiredServices = serviceSet.services.required or [ ];
    optionalServices = serviceSet.services.optional or [ ];
    allServices = serviceSet.services.all or [ ];
    defaultOperation = serviceSet.defaultOperation or "health";
  }) serviceSets;
in
{
  byApp = appExecutionManifests;
  byServiceSet = manifestsByServiceSet;
  manifestIds = manifestIds;
  catalog = canonical.canonicalize {
    schema = {
      kind = "nixfied-runtime-manifest-catalog";
      version = 1;
    };
    inherit resolvedIdentity runtime state;
    serviceCatalog = lib.attrNames serviceCatalog;
    appIds = manifestIds;
    taskIds = builtins.attrNames tasks;
    workflowIds = builtins.attrNames workflows;
    manifestCount = builtins.length manifestIds;
  };
}
