{
  lib,
  canonical,
}:
{
  resolvedIdentity,
  runtime,
  state,
  serviceCatalog,
  serviceSets ? { },
  apps,
  tasks,
  workflows,
}:
let
  listUtils = import ../framework/core/list-utils.nix;
  compileRuntimeMetadata = import ./compile-runtime-metadata.nix { inherit lib; };
  uniquePreserveOrder = listUtils.uniquePreserveOrder;
  uniqueSorted = listUtils.uniqueSorted;

  closureLib = import ./compile-closure-lib.nix { inherit lib; } {
    inherit tasks workflows;
  };
  inherit (closureLib) workflowFamilyFromId;

  taskSet = if tasks == null then { } else tasks;
  workflowSet = if workflows == null then { } else workflows;
  appSet = if apps == null then { } else apps;
  catalog = if serviceCatalog == null then { } else serviceCatalog;
  serviceSetCatalog = if serviceSets == null then { } else serviceSets;

  taskIds = builtins.sort builtins.lessThan (builtins.attrNames taskSet);
  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflowSet);
  appIds = builtins.sort builtins.lessThan (builtins.attrNames appSet);
  serviceSetIds = builtins.sort builtins.lessThan (builtins.attrNames serviceSetCatalog);

  workflowModesByFamily = builtins.foldl' (
    acc: workflowId:
    let
      match = builtins.match "^workflow\\.([^.]+)\\.(.+)$" workflowId;
    in
    if match == null then
      acc
    else
      let
        family = builtins.elemAt match 0;
        mode = builtins.elemAt match 1;
        existing = acc.${family} or [ ];
      in
      acc
      // {
        ${family} = uniqueSorted (existing ++ [ mode ]);
      }
  ) { } workflowIds;

  workflowFamilies = uniqueSorted (builtins.attrNames workflowModesByFamily);

  workflowIdsByFamily = builtins.listToAttrs (
    map (family: {
      name = family;
      value = builtins.filter (workflowId: workflowFamilyFromId workflowId == family) workflowIds;
    }) workflowFamilies
  );

  enabledServices = uniqueSorted (
    builtins.map (
      serviceId:
      let
        service = catalog.${serviceId};
      in
      service.name or serviceId
    ) (builtins.filter (serviceId: catalog.${serviceId}.enable or false) (builtins.attrNames catalog))
  );

  taskDirectServicesById = builtins.mapAttrs (
    _: task: uniquePreserveOrder (((task.requirements or { }).services or [ ]))
  ) taskSet;

  svcEmpty = [ ];
  svcMerge = results: uniquePreserveOrder (builtins.concatLists results);

  baseWalker = closureLib.mkClosureWalker {
    empty = svcEmpty;
    merge = svcMerge;

    taskContrib = _taskId: task: (task.requirements or { }).services or [ ];

    taskWorkflowRef =
      _seen: _task: _goWorkflowReference:
      svcEmpty;
    taskRuntimeWorkflows =
      _seen: _task: _goWorkflowExact:
      svcEmpty;

    workflowUnit =
      _seen: _unit: _goTask:
      svcEmpty;
    workflowPhase =
      _seen: _workflow: _goTask:
      svcEmpty;
    workflowPhaseServiceSets = _workflow: svcEmpty;
    workflowSelf = _workflowId: inner: inner;
  };
  goTaskBase = baseWalker.goTask;

  fullWalker = closureLib.mkClosureWalker {
    empty = svcEmpty;
    merge = svcMerge;

    taskContrib = _taskId: task: (task.requirements or { }).services or [ ];

    taskWorkflowRef =
      seen: task: goWorkflowReference:
      if (task.runner.type or "shell") == "workflowRef" && (task.runner.workflowId or "") != "" then
        goWorkflowReference seen task.runner.workflowId
      else
        svcEmpty;

    taskRuntimeWorkflows =
      seen: task: goWorkflowExact:
      svcMerge (map (wfId: goWorkflowExact seen wfId) (task.runtime.references.workflowIds or [ ]));

    workflowUnit =
      seen: unit: goTask:
      let
        taskId = unit.taskId or "";
      in
      uniquePreserveOrder (
        ((unit.requirements or { }).services or [ ]) ++ (lib.optionals (taskId != "") (goTask seen taskId))
      );

    workflowPhase =
      seen: workflow: goTask:
      let
        phaseTaskIds = (workflow.preRun.tasks or [ ]) ++ (workflow.postRun.tasks or [ ]);
      in
      svcMerge (map (taskId: goTask seen taskId) phaseTaskIds);

    workflowPhaseServiceSets =
      workflow:
      builtins.concatLists (
        map (entry: entry.selectedServices or [ ]) (
          (workflow.preRun.serviceSets or [ ]) ++ (workflow.postRun.serviceSets or [ ])
        )
      );

    workflowSelf = _workflowId: inner: inner;
  };

  goTask = fullWalker.goTask;
  goWorkflow = fullWalker.goWorkflow;
  goWorkflowExact = fullWalker.goWorkflowExact;
  goWorkflowReference = fullWalker.goWorkflowReference;

  unitsOnlyWalker = closureLib.mkClosureWalker {
    empty = svcEmpty;
    merge = svcMerge;

    taskContrib = _taskId: task: (task.requirements or { }).services or [ ];

    taskWorkflowRef =
      seen: task: goWorkflowRef:
      if (task.runner.type or "shell") == "workflowRef" && (task.runner.workflowId or "") != "" then
        goWorkflowRef seen task.runner.workflowId
      else
        svcEmpty;

    taskRuntimeWorkflows =
      seen: task: goWorkflowExact':
      svcMerge (map (wfId: goWorkflowExact' seen wfId) (task.runtime.references.workflowIds or [ ]));

    workflowUnit =
      seen: unit: goTask':
      let
        taskId = unit.taskId or "";
      in
      uniquePreserveOrder (
        ((unit.requirements or { }).services or [ ]) ++ (lib.optionals (taskId != "") (goTask' seen taskId))
      );

    workflowPhase =
      _seen: _workflow: _goTask:
      svcEmpty;
    workflowPhaseServiceSets = _workflow: svcEmpty;

    workflowSelf = _workflowId: inner: inner;
  };
  goWorkflowUnitsOnly = unitsOnlyWalker.goWorkflow;

  taskBaseClosureServicesById = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = goTaskBase [ ] taskId;
    }) taskIds
  );

  _validateTaskRuntimeWorkflowReferences = map (
    taskId:
    let
      task = taskSet.${taskId};
      validateWorkflowRef =
        workflowId:
        if builtins.hasAttr workflowId workflowSet then
          true
        else
          throw "task '${taskId}' runtime references unknown workflow '${workflowId}'";
    in
    map validateWorkflowRef (task.runtime.references.workflowIds or [ ])
  ) taskIds;

  taskClosureServicesById = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = goTask [ ] taskId;
    }) taskIds
  );

  taskRunnerWorkflowIdById = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = taskSet.${taskId}.runner.workflowId or "";
    }) taskIds
  );

  workflowUnitClosureServicesById = builtins.listToAttrs (
    map (workflowId: {
      name = workflowId;
      value = goWorkflowUnitsOnly [ ] workflowId;
    }) workflowIds
  );

  workflowClosureServicesById = builtins.listToAttrs (
    map (workflowId: {
      name = workflowId;
      value = goWorkflow [ ] workflowId;
    }) workflowIds
  );

  workflowExactClosureServicesById = builtins.listToAttrs (
    map (workflowId: {
      name = workflowId;
      value = goWorkflowExact [ ] workflowId;
    }) workflowIds
  );

  workflowReferenceClosureServicesById = builtins.listToAttrs (
    map (workflowId: {
      name = workflowId;
      value = goWorkflowReference [ ] workflowId;
    }) workflowIds
  );

  servicesToCsv = serviceNames: builtins.concatStringsSep "," (uniqueSorted serviceNames);

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

  appWalker = closureLib.mkClosureWalker {
    empty = idClosureEmpty;
    merge = idClosureMerge;

    taskContrib = taskId: _task: {
      taskIds = [ taskId ];
      workflowIds = [ ];
    };

    taskWorkflowRef =
      seen: task: goWorkflowReference':
      if (task.runner.type or "") == "workflowRef" && (task.runner.workflowId or "") != "" then
        goWorkflowReference' seen task.runner.workflowId
      else
        idClosureEmpty;

    taskRuntimeWorkflows =
      seen: task: goWorkflowExact':
      idClosureMerge (map (wfId: goWorkflowExact' seen wfId) (task.runtime.references.workflowIds or [ ]));

    workflowUnit =
      seen: unit: goTask':
      goTask' seen (unit.taskId or "");

    workflowPhase =
      seen: workflow: goTask':
      let
        phaseTaskIds = (workflow.preRun.tasks or [ ]) ++ (workflow.postRun.tasks or [ ]);
      in
      idClosureMerge (map (taskId: goTask' seen taskId) phaseTaskIds);

    workflowPhaseServiceSets = _workflow: idClosureEmpty;

    workflowSelf = workflowId: inner: {
      taskIds = inner.taskIds;
      workflowIds = uniquePreserveOrder ([ workflowId ] ++ inner.workflowIds);
    };
  };
  goAppWorkflowExact = appWalker.goWorkflowExact;
  goAppTask = appWalker.goTask;

  manifestAppIds = builtins.filter (
    appId:
    let
      app = appSet.${appId};
      kind = app.kind or "";
    in
    (kind == "taskRef" && (app.taskId or "") != "")
    || (kind == "workflowRef" && (app.workflowId or "") != "")
  ) appIds;

  executionBase = {
    schema = {
      kind = "nixfied-execution";
      version = 1;
    };
    inherit
      enabledServices
      taskIds
      workflowIds
      workflowFamilies
      workflowIdsByFamily
      workflowModesByFamily
      ;
    tasks = {
      byId = builtins.listToAttrs (
        map (taskId: {
          name = taskId;
          value = {
            directServices = taskDirectServicesById.${taskId} or [ ];
            baseClosureSelectedServices = taskBaseClosureServicesById.${taskId} or [ ];
            baseClosureServicesCsv = servicesToCsv (taskBaseClosureServicesById.${taskId} or [ ]);
            closureSelectedServices = taskClosureServicesById.${taskId} or [ ];
            runnerWorkflowId = taskRunnerWorkflowIdById.${taskId} or "";
          };
        }) taskIds
      );
    };
    workflows = {
      byId = builtins.listToAttrs (
        map (workflowId: {
          name = workflowId;
          value = {
            family = workflowFamilyFromId workflowId;
            unitClosureSelectedServices = workflowUnitClosureServicesById.${workflowId} or [ ];
            closureSelectedServices = workflowClosureServicesById.${workflowId} or [ ];
            exactClosureSelectedServices = workflowExactClosureServicesById.${workflowId} or [ ];
            closureServicesCsv = servicesToCsv (workflowClosureServicesById.${workflowId} or [ ]);
            referenceClosureSelectedServices = workflowReferenceClosureServicesById.${workflowId} or [ ];
          };
        }) workflowIds
      );
    };
  };

  appExecutionById = builtins.listToAttrs (
    map (
      appId:
      let
        app = appSet.${appId};
        closure =
          if (app.kind or "") == "workflowRef" then
            goAppWorkflowExact [ ] app.workflowId
          else
            goAppTask [ ] app.taskId;
        selectedServices =
          if (app.kind or "") == "workflowRef" then
            uniqueSorted (
              executionBase.workflows.byId.${app.workflowId}.exactClosureSelectedServices or [ ]
            )
          else
            uniqueSorted (executionBase.tasks.byId.${app.taskId}.closureSelectedServices or [ ]);
        serviceCatalogFiltered = lib.filterAttrs (
          _: service: builtins.elem (service.name or service.id) selectedServices
        ) catalog;
        manifestTasks = lib.getAttrs closure.taskIds taskSet;
        manifestWorkflows = lib.getAttrs closure.workflowIds workflowSet;
        manifestRuntimeMetadata = compileRuntimeMetadata {
          apps = appSet;
          compiledExecution = executionBase;
          tasks = manifestTasks;
          workflows = manifestWorkflows;
          serviceCatalog = serviceCatalogFiltered;
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
        name = appId;
        value = {
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
      }
    ) manifestAppIds
  );

  serviceSetExecutionById = builtins.mapAttrs (
    name: serviceSet: {
      id = serviceSet.id or name;
      name = serviceSet.name or name;
      summary = serviceSet.summary or "";
      services = serviceSet.services or { };
      requiredServices = serviceSet.services.required or [ ];
      optionalServices = serviceSet.services.optional or [ ];
      allServices = serviceSet.services.all or [ ];
      defaultOperation = serviceSet.defaultOperation or "health";
    }
  ) serviceSetCatalog;
in
canonical.canonicalize (
  executionBase
  // {
    apps = {
      ids = manifestAppIds;
      byId = appExecutionById;
    };
    serviceSets = {
      ids = serviceSetIds;
      byId = serviceSetExecutionById;
    };
  }
)
