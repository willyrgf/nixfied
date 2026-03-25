{ lib }:
{
  tasks,
  workflows,
  serviceCatalog,
}:
let
  listUtils = import ../framework/core/list-utils.nix;

  closureLib = import ./compile-closure-lib.nix { inherit lib; } {
    inherit tasks workflows;
  };
  inherit (closureLib) workflowFamilyFromId uniquePreserveOrder;
  uniqueSorted = listUtils.uniqueSorted;

  taskSet = if tasks == null then { } else tasks;
  workflowSet = if workflows == null then { } else workflows;
  catalog = if serviceCatalog == null then { } else serviceCatalog;

  taskIds = builtins.sort builtins.lessThan (builtins.attrNames taskSet);
  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflowSet);

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

  # For the selection-index we need the family-keyed lookup (family -> list of
  # workflow ids), which differs from the closure-lib's per-workflowId lookup.
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

  # ---- service-list merge helpers ------------------------------------

  svcEmpty = [ ];
  svcMerge = results: uniquePreserveOrder (builtins.concatLists results);

  # ---- goTaskBase: service closure WITHOUT following workflow refs ----
  #
  # Uses the generic walker with workflow strategies that return [].
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

    # These won't be reached because workflows are never entered, but
    # they must be present for the strategy interface.
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

  # ---- full service closure walker -----------------------------------

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

  # ---- units-only workflow closure -----------------------------------
  #
  # Same as goWorkflow but skips phase tasks and phase service sets.
  # Uses a separate walker so it gets its own cycle-detection tokens.
  unitsOnlyWalker = closureLib.mkClosureWalker {
    empty = svcEmpty;
    merge = svcMerge;

    taskContrib = _taskId: task: (task.requirements or { }).services or [ ];

    taskWorkflowRef =
      seen: task: goWfRef:
      if (task.runner.type or "shell") == "workflowRef" && (task.runner.workflowId or "") != "" then
        goWfRef seen task.runner.workflowId
      else
        svcEmpty;

    taskRuntimeWorkflows =
      seen: task: goWfExact:
      svcMerge (map (wfId: goWfExact seen wfId) (task.runtime.references.workflowIds or [ ]));

    workflowUnit =
      seen: unit: goTsk:
      let
        taskId = unit.taskId or "";
      in
      uniquePreserveOrder (
        ((unit.requirements or { }).services or [ ]) ++ (lib.optionals (taskId != "") (goTsk seen taskId))
      );

    # Skip phases for units-only.
    workflowPhase =
      _seen: _workflow: _goTask:
      svcEmpty;
    workflowPhaseServiceSets = _workflow: svcEmpty;

    workflowSelf = _workflowId: inner: inner;
  };

  goWorkflowUnitsOnly = unitsOnlyWalker.goWorkflow;

  # ---- derived indexes -----------------------------------------------

  taskClosureServicesById = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = goTask [ ] taskId;
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

  taskBaseClosureServicesById = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = goTaskBase [ ] taskId;
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

  taskBaseClosureServicesCsvById = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = servicesToCsv (taskBaseClosureServicesById.${taskId} or [ ]);
    }) taskIds
  );

  workflowClosureServicesCsvById = builtins.listToAttrs (
    map (workflowId: {
      name = workflowId;
      value = servicesToCsv (workflowClosureServicesById.${workflowId} or [ ]);
    }) workflowIds
  );
in
{
  inherit
    enabledServices
    taskIds
    workflowIds
    workflowFamilies
    workflowIdsByFamily
    servicesToCsv
    taskDirectServicesById
    taskBaseClosureServicesById
    taskBaseClosureServicesCsvById
    taskClosureServicesById
    taskRunnerWorkflowIdById
    workflowModesByFamily
    workflowUnitClosureServicesById
    workflowClosureServicesById
    workflowExactClosureServicesById
    workflowClosureServicesCsvById
    workflowReferenceClosureServicesById
    ;
}
