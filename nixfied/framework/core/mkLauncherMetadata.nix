{
  lib,
  model,
  execution,
  workspaceMarkerPresent,
}:
let
  serviceSurfaceCatalog = (model.compiled or { }).serviceSurfaceCatalog or { };
  taskExecutionById = execution.tasks.byId or { };
  workflowExecutionById = execution.workflows.byId or { };
  viewAppNames = builtins.sort builtins.lessThan (builtins.attrNames (model.views.apps or { }));

  selectorDispatcherAppNames = [
    "run-task"
    "run-workflow"
    "run-workflow-parallel"
  ];

  nonSelectorAppNames = [
    "framework::install"
    "framework::upgrade"
  ];

  runtimeProxyAppNames = if workspaceMarkerPresent then [ ] else nonSelectorAppNames;

  viewWrappedAppNames = builtins.sort builtins.lessThan (
    builtins.filter (appName: !(builtins.elem appName nonSelectorAppNames)) (
      lib.unique (viewAppNames ++ selectorDispatcherAppNames)
    )
  );

  serviceWrappedAppNames = builtins.sort builtins.lessThan (serviceSurfaceCatalog.appNames or [ ]);

  runtimeAppNames = builtins.sort builtins.lessThan (
    lib.unique (
      runtimeProxyAppNames
      ++ builtins.filter (appName: builtins.elem appName nonSelectorAppNames) viewAppNames
    )
  );
in
{
  enabledServices = execution.enabledServices or [ ];
  taskIds =
    execution.taskIds or (builtins.sort builtins.lessThan (builtins.attrNames (model.tasks or { })));
  workflowIds =
    execution.workflowIds
      or (builtins.sort builtins.lessThan (builtins.attrNames (model.workflows or { })));
  workflowModesByFamily = execution.workflowModesByFamily or { };
  workflowFamilies =
    execution.workflowFamilies
      or (builtins.sort builtins.lessThan (builtins.attrNames (execution.workflowModesByFamily or { })));
  taskBaseClosureCsvById = builtins.mapAttrs (
    _: taskExecution: taskExecution.baseClosureServicesCsv or ""
  ) taskExecutionById;
  taskRunnerWorkflowIdById = builtins.mapAttrs (
    _: taskExecution: taskExecution.runnerWorkflowId or ""
  ) taskExecutionById;
  workflowClosureCsvById = builtins.mapAttrs (
    _: workflowExecution: workflowExecution.closureServicesCsv or ""
  ) workflowExecutionById;
  inherit
    selectorDispatcherAppNames
    nonSelectorAppNames
    runtimeProxyAppNames
    viewWrappedAppNames
    serviceWrappedAppNames
    runtimeAppNames
    ;
}
