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
}:
let
  listUtils = import ../framework/core/list-utils.nix;
  compileRuntimeMetadata = import ./compile-runtime-metadata.nix { inherit lib; };
  uniquePreserveOrder = listUtils.uniquePreserveOrder;
  uniqueSorted = listUtils.uniqueSorted;

  closureLib = import ./compile-closure-lib.nix { inherit lib; } {
    inherit tasks workflows;
  };

  appIds = builtins.sort builtins.lessThan (builtins.attrNames apps);

  # Strategy for collecting { taskIds, workflowIds } closures.
  idClosureEmpty = {
    taskIds = [ ];
    workflowIds = [ ];
  };

  idClosureMerge =
    results:
    let
      nonEmpty = builtins.filter (r: r.taskIds != [ ] || r.workflowIds != [ ]) results;
    in
    {
      taskIds = uniquePreserveOrder (builtins.concatLists (map (r: r.taskIds) nonEmpty));
      workflowIds = uniquePreserveOrder (builtins.concatLists (map (r: r.workflowIds) nonEmpty));
    };

  walker = closureLib.mkClosureWalker {
    empty = idClosureEmpty;
    merge = idClosureMerge;

    taskContrib = _taskId: _task: {
      taskIds = [ _taskId ];
      workflowIds = [ ];
    };

    taskWorkflowRef =
      seen: task: goWorkflowReference:
      if (task.runner.type or "") == "workflowRef" && (task.runner.workflowId or "") != "" then
        goWorkflowReference seen task.runner.workflowId
      else
        idClosureEmpty;

    taskRuntimeWorkflows =
      seen: task: goWorkflowExact:
      let
        closures = map (wfId: goWorkflowExact seen wfId) (task.runtime.references.workflowIds or [ ]);
      in
      idClosureMerge closures;

    workflowUnit =
      seen: unit: goTask:
      goTask seen (unit.taskId or "");

    workflowPhase =
      seen: workflow: goTask:
      let
        phaseTaskIds = (workflow.preRun.tasks or [ ]) ++ (workflow.postRun.tasks or [ ]);
        closures = map (taskId: goTask seen taskId) phaseTaskIds;
      in
      idClosureMerge closures;

    workflowPhaseServiceSets = _workflow: idClosureEmpty;

    workflowSelf = workflowId: inner: {
      taskIds = inner.taskIds;
      workflowIds = uniquePreserveOrder ([ workflowId ] ++ inner.workflowIds);
    };
  };

  inherit (walker)
    goTask
    goWorkflowExact
    ;

  manifestForApp =
    appId:
    let
      app = apps.${appId};
      closure =
        if (app.kind or "") == "workflowRef" then
          goWorkflowExact [ ] app.workflowId
        else
          goTask [ ] app.taskId;
      selectedServices =
        if (app.kind or "") == "workflowRef" then
          uniqueSorted (selectionIndex.workflowExactClosureServicesById.${app.workflowId} or [ ])
        else
          uniqueSorted (selectionIndex.taskClosureServicesById.${app.taskId} or [ ]);
      serviceCatalogFiltered = lib.filterAttrs (
        _: service: builtins.elem (service.name or service.id) selectedServices
      ) serviceCatalog;
      manifestTasks = lib.getAttrs closure.taskIds tasks;
      manifestWorkflows = lib.getAttrs closure.workflowIds workflows;
      manifestRuntimeMetadata = compileRuntimeMetadata {
        tasks = manifestTasks;
        workflows = manifestWorkflows;
        serviceCatalog = serviceCatalogFiltered;
        inherit
          apps
          selectionIndex
          ;
      };
      manifestEvalHash = canonical.hashCanonical {
        schema = {
          kind = "nixfied-app-execution-eval";
          version = 1;
        };
        runtime = runtime;
        state = state;
        serviceCatalog = serviceCatalogFiltered;
        tasks = manifestTasks;
        workflows = manifestWorkflows;
      };
      manifestIdentity = {
        projectId = resolvedIdentity.projectId;
        projectName = resolvedIdentity.projectName;
        description = resolvedIdentity.description;
        evalHash = manifestEvalHash;
      };
      manifestModel = canonical.canonicalize {
        schema = {
          kind = "nixfied-execution-manifest";
          version = 1;
        };
        identity = manifestIdentity;
        runtime = runtime;
        state = state;
        serviceCatalog = serviceCatalogFiltered;
        tasks = manifestTasks;
        workflows = manifestWorkflows;
        compiled = {
          runtimeMetadata = manifestRuntimeMetadata;
        };
      };
    in
    {
      id = appId;
      taskId = if (app.taskId or "") == "" then null else app.taskId;
      workflowId = if (app.workflowId or "") == "" then null else app.workflowId;
      taskIds = closure.taskIds;
      workflowIds = closure.workflowIds;
      selectedServices = selectedServices;
      model = manifestModel;
      evalHash = manifestEvalHash;
      modelHash = canonical.hashCanonical manifestModel;
    };
  manifestAppIds = builtins.filter (
    appId:
    let
      app = apps.${appId};
    in
    (
      ((app.kind or "") == "taskRef" && (app.taskId or "") != "")
      || ((app.kind or "") == "workflowRef" && (app.workflowId or "") != "")
    )
  ) appIds;
in
builtins.listToAttrs (
  map (appId: {
    name = appId;
    value = manifestForApp appId;
  }) manifestAppIds
)
