{ canonical, ... }:
{
  resolved ? null,
  tasks ? { },
  apps ? { },
  workflows ? { },
  serviceSets ? { },
  services ? { },
}:
let
  resolvedIdentity =
    if resolved == null || !(builtins.isAttrs resolved) then null else resolved.identity or null;

  taskCatalog = builtins.mapAttrs (_name: task: {
    inherit (task) id;
    kind = task.kind or "command";
    commandApi = task.commandApi or null;
  }) tasks;

  appCatalog = builtins.mapAttrs (_name: app: {
    inherit (app) id;
    kind = app.kind or "taskRef";
    commandApi = app.commandApi or null;
  }) apps;

  workflowCatalog = builtins.mapAttrs (_name: workflow: {
    inherit (workflow) id;
    kind = workflow.kind or "workflow";
    commandApi = workflow.commandApi or null;
  }) workflows;

  serviceCatalog = builtins.mapAttrs (_name: serviceSet: {
    id = serviceSet.id or _name;
    kind = "serviceSet";
    commandApi = serviceSet.commandApi or null;
  }) serviceSets;

  serviceRuntimeCatalog = builtins.mapAttrs (_name: service: {
    id = service.id or _name;
    kind = "service";
    commandApi = service.commandApi or null;
  }) services;

  apiByName = taskCatalog // appCatalog // workflowCatalog // serviceCatalog // serviceRuntimeCatalog;

  catalog = canonical.canonicalize {
    schema = {
      kind = "nixfied-api-catalog";
      version = 1;
    };
    inherit
      resolvedIdentity
      taskCatalog
      appCatalog
      workflowCatalog
      serviceCatalog
      serviceRuntimeCatalog
      ;
  };
in
{
  inherit
    catalog
    apiByName
    ;
  commandRefs = apiByName;
}
