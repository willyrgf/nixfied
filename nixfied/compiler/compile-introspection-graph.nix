{
  lib,
  canonical,
}:
{
  projectRoot,
  resolved,
  statePolicy,
  runtime,
  apps,
  appExecutionManifests,
  tasks,
  workflows,
  serviceCatalog,
  serviceSurfaceCatalog,
  features,
  selectionIndex,
  localOverridesActive ? false,
  localOverrideCount ? 0,
}:
let
  workspaceMarker = import ../framework/workspace-marker.nix;
  workspaceMarkerPresent = workspaceMarker.isPresent projectRoot;

  uniqueSorted = values: builtins.sort builtins.lessThan (lib.unique values);

  featureOwnerFiles =
    featureId:
    if builtins.hasAttr featureId features then
      uniqueSorted (features.${featureId}.ownerFiles or [ ])
    else
      [ ];

  packageNameFromValue =
    value:
    let
      raw =
        if value == null then
          ""
        else if builtins.isString value then
          value
        else
          builtins.toString value;
      base = builtins.baseNameOf raw;
      nameMatch = builtins.match "^[^-]+-(.+)$" base;
      storeName = if nameMatch == null then base else builtins.elemAt nameMatch 0;
      versionMatch = builtins.match "^(.+)-[0-9].*$" storeName;
    in
    if raw == "" then
      ""
    else if versionMatch == null then
      storeName
    else
      builtins.elemAt versionMatch 0;

  mapHookPackageRefs =
    phase: hooks:
    builtins.concatLists (
      map (
        hookName:
        let
          hook = hooks.${hookName};
        in
        map (
          runtimeInput:
          let
            packageName = packageNameFromValue runtimeInput;
          in
          {
            name = packageName;
            kind = "hook-package";
            via = "${phase}:${hookName}";
            reason = "task ${phase} hook '${hookName}' runtimeInputs references package '${packageName}'";
          }
        ) (hook.runtimeInputs or [ ])
      ) (builtins.sort builtins.lessThan (builtins.attrNames hooks))
    );

  taskPackageRefs =
    taskId:
    let
      task = tasks.${taskId};
      runnerRefs = lib.optionals ((task.runner.package or null) != null) [
        {
          name = packageNameFromValue task.runner.package;
          kind = "runner-package";
          via = "runner.package";
          reason = "task runner package for '${taskId}' references package '${packageNameFromValue task.runner.package}'";
        }
      ];
      runtimeRefs = map (
        runtimeInput:
        let
          packageName = packageNameFromValue runtimeInput;
        in
        {
          name = packageName;
          kind = "runtime-package";
          via = "runtime.runtimeInputs";
          reason = "task runtimeInputs for '${taskId}' references package '${packageName}'";
        }
      ) (task.runtime.runtimeInputs or [ ]);
      hookRefs =
        mapHookPackageRefs "pre" (task.runtime.preHooks or { })
        ++ mapHookPackageRefs "post" (task.runtime.postHooks or { });
    in
    builtins.filter (ref: ref.name != "") (runnerRefs ++ runtimeRefs ++ hookRefs);

  taskIds = builtins.sort builtins.lessThan (builtins.attrNames tasks);
  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflows);
  appIds = builtins.sort builtins.lessThan (builtins.attrNames apps);
  serviceIds = builtins.sort builtins.lessThan (builtins.attrNames serviceCatalog);
  serviceNames = uniqueSorted (map (serviceId: serviceCatalog.${serviceId}.name) serviceIds);

  workflowTaskIds =
    workflowId:
    let
      workflow = workflows.${workflowId};
    in
    uniqueSorted (
      map (unit: unit.taskId) (workflow.plan or [ ])
      ++ (workflow.preRun.tasks or [ ])
      ++ (workflow.postRun.tasks or [ ])
    );

  packageNames = uniqueSorted (
    builtins.concatLists (map (taskId: map (ref: ref.name) (taskPackageRefs taskId)) taskIds)
  );

  publicAppsByServiceName = builtins.listToAttrs (
    map (serviceName: {
      name = serviceName;
      value = builtins.sort builtins.lessThan (
        builtins.filter (
          appName: (serviceSurfaceCatalog.appServiceByName.${appName} or "") == serviceName
        ) (builtins.attrNames (serviceSurfaceCatalog.appServiceByName or { }))
      );
    }) serviceNames
  );

  mkNode = node: {
    name = node.nodeId;
    value = canonical.canonicalize node;
  };

  appNodes = builtins.listToAttrs (
    map (
      appId:
      let
        app = apps.${appId};
        task = tasks.${app.taskId};
        manifest = appExecutionManifests.${appId} or null;
        workflowIdsForApp = lib.optionals (
          (task.runner.type or "") == "workflowRef" && (task.runner.workflowId or "") != ""
        ) [ task.runner.workflowId ];
      in
      mkNode {
        nodeId = "app:${appId}";
        kind = "app";
        id = appId;
        label = appId;
        summary = app.summary;
        description = app.description;
        ownerFiles = lib.optionals ((app.ownerFile or null) != null) [ app.ownerFile ];
        data = {
          category = app.category or "core";
          taskId = app.taskId;
          usage = app.usage or [ ];
          examples = app.examples or [ ];
        };
        execution = {
          launcherClass = "selected-app";
          launcherTarget = appId;
          runSurface = "run-task";
          mappedTaskIds = [ app.taskId ];
          mappedWorkflowIds = workflowIdsForApp;
          selectedServices =
            if manifest != null then
              manifest.selectedServices
            else
              selectionIndex.taskClosureServicesById.${app.taskId} or [ ];
          runtimeRoots = {
            policyId = statePolicy.id;
            policyKind = statePolicy.kind;
            workspaceId = statePolicy.workspaceId;
            runtimeBase = statePolicy.runtimeBase;
            registryRoot = statePolicy.registryRoot;
            artifactsRoot = statePolicy.artifactsRoot;
            manifestHash = if manifest == null then "" else manifest.modelHash;
          };
          workspaceMarkerPresent = workspaceMarkerPresent;
          localOverrides = {
            active = localOverridesActive;
            count = localOverrideCount;
          };
        };
        closure = {
          directTaskIds = if manifest == null then [ app.taskId ] else manifest.taskIds;
          directPackageNames = uniqueSorted (map (ref: ref.name) (taskPackageRefs app.taskId));
          selectedServices =
            if manifest != null then
              manifest.selectedServices
            else
              selectionIndex.taskClosureServicesById.${app.taskId} or [ ];
        };
      }
    ) appIds
  );

  taskNodes = builtins.listToAttrs (
    map (
      taskId:
      let
        task = tasks.${taskId};
        workflowIdsForTask = lib.optionals (
          (task.runner.type or "") == "workflowRef" && (task.runner.workflowId or "") != ""
        ) [ task.runner.workflowId ];
      in
      mkNode {
        nodeId = "task:${taskId}";
        kind = "task";
        id = taskId;
        label = taskId;
        summary = task.summary;
        description = task.description;
        ownerFiles = featureOwnerFiles taskId;
        data = {
          taskKind = task.kind or "command";
          runner = {
            type = task.runner.type or "shell";
            workflowId = task.runner.workflowId or "";
          };
        };
        execution = {
          launcherClass = "dispatcher";
          launcherTarget = "run-task";
          runSurface = "run-task";
          mappedTaskIds = [ taskId ];
          mappedWorkflowIds = workflowIdsForTask;
          selectedServices = selectionIndex.taskClosureServicesById.${taskId} or [ ];
        };
        closure = {
          directTaskDeps = uniqueSorted ((task.deps.needs or [ ]) ++ (task.deps.softNeeds or [ ]));
          directPackageNames = uniqueSorted (map (ref: ref.name) (taskPackageRefs taskId));
          directServices = selectionIndex.taskDirectServicesById.${taskId} or [ ];
          baseClosureServices = selectionIndex.taskBaseClosureServicesById.${taskId} or [ ];
          selectedServices = selectionIndex.taskClosureServicesById.${taskId} or [ ];
        };
      }
    ) taskIds
  );

  workflowNodes = builtins.listToAttrs (
    map (
      workflowId:
      let
        workflow = workflows.${workflowId};
      in
      mkNode {
        nodeId = "workflow:${workflowId}";
        kind = "workflow";
        id = workflowId;
        label = workflowId;
        summary = workflow.summary;
        description = workflow.description;
        ownerFiles = featureOwnerFiles workflowId;
        data = {
          mode = workflow.mode or "custom";
          unitNames = map (unit: unit.name) (workflow.plan or [ ]);
        };
        execution = {
          launcherClass = "dispatcher";
          launcherTarget = "run-workflow";
          runSurface = "run-workflow";
          mappedTaskIds = workflowTaskIds workflowId;
          mappedWorkflowIds = [ workflowId ];
          selectedServices = selectionIndex.workflowClosureServicesById.${workflowId} or [ ];
        };
        closure = {
          unitTaskIds = uniqueSorted (map (unit: unit.taskId) (workflow.plan or [ ]));
          preRunTaskIds = workflow.preRun.tasks or [ ];
          postRunTaskIds = workflow.postRun.tasks or [ ];
          selectedServices = selectionIndex.workflowClosureServicesById.${workflowId} or [ ];
        };
      }
    ) workflowIds
  );

  serviceNodes = builtins.listToAttrs (
    map (
      serviceId:
      let
        service = serviceCatalog.${serviceId};
        serviceName = service.name;
      in
      mkNode {
        nodeId = "service:${serviceName}";
        kind = "service";
        id = serviceName;
        label = serviceName;
        summary = "Service ${serviceName}";
        description = "Compiled service catalog entry for ${serviceName}.";
        ownerFiles = featureOwnerFiles serviceId;
        data = {
          serviceId = serviceId;
          enable = service.enable or false;
          defaultSource = service.config.defaultSource or "";
          sourceKeys = service.config.sourceKeys or [ ];
          probeModes = service.config.probeModes or [ ];
        };
        execution = {
          launcherClass =
            if (publicAppsByServiceName.${serviceName} or [ ]) == [ ] then "model-only" else "service-api";
          launcherTarget = null;
          runSurface = null;
          mappedTaskIds = [ ];
          mappedWorkflowIds = [ ];
          publicApps = publicAppsByServiceName.${serviceName} or [ ];
        };
        closure = {
          selectedServices = [ serviceName ];
        };
      }
    ) serviceIds
  );

  packageNodes = builtins.listToAttrs (
    map (
      packageName:
      mkNode {
        nodeId = "package:${packageName}";
        kind = "package";
        id = packageName;
        label = packageName;
        summary = "Package ${packageName}";
        description = "Package reference extracted from compiled runtime inputs and hooks.";
        ownerFiles = [ ];
        data = { };
        execution = null;
        closure = null;
      }
    ) packageNames
  );

  executionNodes = builtins.listToAttrs (
    (map (
      appId:
      mkNode {
        nodeId = "execution:selected-app-launcher:${appId}";
        kind = "execution";
        id = "selected-app-launcher:${appId}";
        label = "selected-app-launcher:${appId}";
        summary = "Selected app launcher for ${appId}";
        description = "Public app '${appId}' resolves through the selector launcher built from framework/launch/run-selected-app.nix.";
        ownerFiles = [
          "nixfied/framework/core/mkFlakeOutputs.nix"
          "nixfied/framework/launch/run-selected-app.nix"
        ];
        data = { };
        execution = null;
        closure = null;
      }
    ) appIds)
    ++ map (
      appId:
      let
        manifest = appExecutionManifests.${appId};
      in
      mkNode {
        nodeId = "execution:app-manifest:${appId}";
        kind = "execution";
        id = "app-manifest:${appId}";
        label = "app-manifest:${appId}";
        summary = "App execution manifest for ${appId}";
        description = "Selected-app execution serializes an app-scoped execution manifest for '${appId}'.";
        ownerFiles = [
          "nixfied/compiler/compile-app-execution-manifests.nix"
          "nixfied/framework/core/materializeExecution.nix"
          "nixfied/framework/runtime/executor.nix"
        ];
        data = {
          modelHash = manifest.modelHash;
          taskIds = manifest.taskIds;
          workflowIds = manifest.workflowIds;
          selectedServices = manifest.selectedServices;
        };
        execution = null;
        closure = null;
      }
    ) (builtins.sort builtins.lessThan (builtins.attrNames appExecutionManifests))
  );

  nodes = appNodes // taskNodes // workflowNodes // serviceNodes // packageNodes // executionNodes;

  mkEdge =
    {
      from,
      to,
      kind,
      reason,
      via ? null,
    }:
    canonical.canonicalize {
      inherit
        from
        to
        kind
        reason
        via
        ;
    };

  dedupeEdges =
    edges:
    let
      add =
        acc: edge:
        let
          edgeKey = "${edge.from}|${edge.to}|${edge.kind}|${
            if edge.via == null then "" else edge.via
          }|${edge.reason}";
        in
        acc
        // {
          ${edgeKey} = edge;
        };
    in
    builtins.attrValues (builtins.foldl' add { } edges);

  sortEdges =
    edges:
    builtins.sort (
      a: b:
      if a.from != b.from then
        a.from < b.from
      else if a.to != b.to then
        a.to < b.to
      else if a.kind != b.kind then
        a.kind < b.kind
      else
        a.reason < b.reason
    ) edges;

  appEdges = builtins.concatLists (
    map (
      appId:
      let
        app = apps.${appId};
      in
      [
        (mkEdge {
          from = "app:${appId}";
          to = "task:${app.taskId}";
          kind = "app-task";
          reason = "app '${appId}' resolves to task '${app.taskId}'";
        })
        (mkEdge {
          from = "app:${appId}";
          to = "execution:selected-app-launcher:${appId}";
          kind = "app-execution";
          reason = "app '${appId}' runs through the selected-app launcher";
        })
        (mkEdge {
          from = "execution:selected-app-launcher:${appId}";
          to = "execution:app-manifest:${appId}";
          kind = "execution-manifest";
          reason = "selected-app execution materializes the app-scoped execution manifest for '${appId}'";
        })
      ]
    ) appIds
  );

  taskEdges = builtins.concatLists (
    map (
      taskId:
      let
        task = tasks.${taskId};
        depEdges = map (
          depTaskId:
          mkEdge {
            from = "task:${taskId}";
            to = "task:${depTaskId}";
            kind = "task-dependency";
            reason = "task '${taskId}' depends on task '${depTaskId}'";
          }
        ) (task.deps.needs or [ ]);
        softDepEdges = map (
          depTaskId:
          mkEdge {
            from = "task:${taskId}";
            to = "task:${depTaskId}";
            kind = "task-soft-dependency";
            reason = "task '${taskId}' soft-depends on task '${depTaskId}'";
          }
        ) (task.deps.softNeeds or [ ]);
        workflowEdges =
          lib.optionals ((task.runner.type or "") == "workflowRef" && (task.runner.workflowId or "") != "")
            [
              (mkEdge {
                from = "task:${taskId}";
                to = "workflow:${task.runner.workflowId}";
                kind = "task-workflow";
                reason = "task '${taskId}' runs workflow '${task.runner.workflowId}'";
              })
            ];
        serviceEdges = map (
          serviceName:
          mkEdge {
            from = "task:${taskId}";
            to = "service:${serviceName}";
            kind = "task-service";
            reason = "task '${taskId}' directly requires service '${serviceName}'";
          }
        ) ((task.requirements or { }).services or [ ]);
        packageEdges = map (
          ref:
          mkEdge {
            from = "task:${taskId}";
            to = "package:${ref.name}";
            kind = ref.kind;
            via = ref.via;
            reason = ref.reason;
          }
        ) (taskPackageRefs taskId);
      in
      depEdges ++ softDepEdges ++ workflowEdges ++ serviceEdges ++ packageEdges
    ) taskIds
  );

  workflowEdges = builtins.concatLists (
    map (
      workflowId:
      let
        workflow = workflows.${workflowId};
        unitEdges = map (
          unit:
          mkEdge {
            from = "workflow:${workflowId}";
            to = "task:${unit.taskId}";
            kind = "workflow-unit-task";
            via = unit.name;
            reason = "workflow '${workflowId}' unit '${unit.name}' runs task '${unit.taskId}'";
          }
        ) (workflow.plan or [ ]);
        unitServiceEdges = builtins.concatLists (
          map (
            unit:
            map (
              serviceName:
              mkEdge {
                from = "workflow:${workflowId}";
                to = "service:${serviceName}";
                kind = "workflow-unit-service";
                via = unit.name;
                reason = "workflow '${workflowId}' unit '${unit.name}' directly requires service '${serviceName}'";
              }
            ) ((unit.requirements or { }).services or [ ])
          ) (workflow.plan or [ ])
        );
        preRunEdges = map (
          taskId:
          mkEdge {
            from = "workflow:${workflowId}";
            to = "task:${taskId}";
            kind = "workflow-prerun-task";
            via = "preRun";
            reason = "workflow '${workflowId}' preRun includes task '${taskId}'";
          }
        ) (workflow.preRun.tasks or [ ]);
        postRunEdges = map (
          taskId:
          mkEdge {
            from = "workflow:${workflowId}";
            to = "task:${taskId}";
            kind = "workflow-postrun-task";
            via = "postRun";
            reason = "workflow '${workflowId}' postRun includes task '${taskId}'";
          }
        ) (workflow.postRun.tasks or [ ]);
      in
      unitEdges ++ unitServiceEdges ++ preRunEdges ++ postRunEdges
    ) workflowIds
  );

  executionEdges = builtins.concatLists (
    map (
      appId:
      let
        manifest = appExecutionManifests.${appId};
      in
      (map (
        taskId:
        mkEdge {
          from = "execution:app-manifest:${appId}";
          to = "task:${taskId}";
          kind = "manifest-task";
          reason = "the app execution manifest for '${appId}' includes task '${taskId}'";
        }
      ) manifest.taskIds)
      ++ map (
        workflowId:
        mkEdge {
          from = "execution:app-manifest:${appId}";
          to = "workflow:${workflowId}";
          kind = "manifest-workflow";
          reason = "the app execution manifest for '${appId}' includes workflow '${workflowId}'";
        }
      ) manifest.workflowIds
    ) (builtins.sort builtins.lessThan (builtins.attrNames appExecutionManifests))
  );

  edges = sortEdges (dedupeEdges (appEdges ++ taskEdges ++ workflowEdges ++ executionEdges));
in
canonical.canonicalize {
  schema = {
    kind = "nixfied-introspection-graph";
    version = 1;
  };
  state = {
    policyId = statePolicy.id;
    policyKind = statePolicy.kind;
    policySource = statePolicy.source;
    ownerScope = statePolicy.ownerScope;
    discoveryScope = statePolicy.discoveryScope;
    workspaceId = statePolicy.workspaceId;
    runtimeBase = statePolicy.runtimeBase;
    registryRoot = statePolicy.registryRoot;
    artifactsRoot = statePolicy.artifactsRoot;
    workspaceMarkerPresent = workspaceMarkerPresent;
  };
  localOverrides = {
    active = localOverridesActive;
    count = localOverrideCount;
  };
  resolution = {
    appIds = appIds;
    taskIds = taskIds;
    workflowIds = workflowIds;
    serviceIds = serviceIds;
    serviceNames = serviceNames;
    packageNames = packageNames;
  };
  nodes = nodes;
  edges = edges;
}
