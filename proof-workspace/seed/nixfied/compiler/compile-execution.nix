{
  lib,
  canonical,
}:
{
  serviceCatalog,
  serviceSets ? { },
  apps,
  tasks,
  workflows,
  ...
}:
let
  listUtils = import ../framework/core/list-utils.nix;
  inherit (listUtils) uniquePreserveOrder;
  inherit (listUtils) uniqueSorted;

  closureLib = import ./compile-closure-lib.nix { inherit lib; } {
    inherit tasks workflows;
  };
  inherit (closureLib) workflowFamilyFromId;
  workflowModeFromId =
    workflowId:
    let
      match = builtins.match "^workflow\\.([^.]+)\\.(.+)$" workflowId;
    in
    if match == null then "" else builtins.elemAt match 1;

  taskSet = if tasks == null then { } else tasks;
  workflowSet = if workflows == null then { } else workflows;
  appSet = if apps == null then { } else apps;
  catalog = if serviceCatalog == null then { } else serviceCatalog;
  serviceSetCatalog = if serviceSets == null then { } else serviceSets;

  taskIds = builtins.sort builtins.lessThan (builtins.attrNames taskSet);
  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflowSet);
  serviceSetIds = builtins.sort builtins.lessThan (builtins.attrNames serviceSetCatalog);

  buildExecutionDescriptors =
    {
      tasks,
      workflows,
      apps ? { },
      compiledExecution,
    }:
    let
      taskIds = uniqueSorted (builtins.attrNames tasks);
      workflowIds = uniqueSorted (builtins.attrNames workflows);

      normalizeTaskArgSpec =
        spec:
        let
          hasLong = (spec ? long) && spec.long != null && spec.long != "";
          hasShort = (spec ? short) && spec.short != null && spec.short != "";
          kind =
            if (spec ? kind) && spec.kind != null then
              spec.kind
            else if hasLong || hasShort then
              "option"
            else
              "positional";
        in
        {
          inherit kind;
          name = spec.name or "";
          required = spec.required or false;
          long = if hasLong then spec.long else "";
          short = if hasShort then spec.short else "";
          type = if (spec ? type) && spec.type != null && spec.type != "" then toString spec.type else "";
          values = if (spec ? values) && spec.values != null then map toString spec.values else [ ];
          description = if (spec ? description) && spec.description != null then spec.description else "";
        };

      formatTaskArgHelpLine =
        spec:
        let
          tokens =
            (lib.optionals (spec.short != "") [ spec.short ])
            ++ (lib.optionals (spec.long != "") [ spec.long ]);
          valueLabel =
            if spec.kind != "option" then
              ""
            else if spec.values != [ ] then
              "<${builtins.concatStringsSep "|" spec.values}>"
            else if spec.type != "" then
              "<${spec.type}>"
            else
              "<value>";
          descriptionSuffix = if spec.description != "" then ": ${spec.description}" else "";
        in
        "  ${builtins.concatStringsSep ", " tokens}${
            lib.optionalString (valueLabel != "") " ${valueLabel}"
          }${descriptionSuffix}";

      valueToString =
        value:
        if value == null then
          ""
        else if builtins.isBool value then
          if value then "1" else "0"
        else if builtins.isAttrs value || builtins.isList value then
          builtins.toJSON value
        else
          toString value;

      mergeTaskRuntimeWithRunnerPackage =
        task:
        let
          packagePath = task.runner.package or null;
        in
        task.runtime
        // {
          runtimeInputs =
            (task.runtime.runtimeInputs or [ ])
            ++ lib.optionals (packagePath != null && packagePath != "") [ packagePath ];
        };

      mergeHookRuntime =
        task: hook:
        let
          taskRuntime = task.runtime;
          hookWorkdir = hook.workdir or null;
          hookCustomWorkdir = hook.customWorkdir or null;
          taskCustomWorkdir = taskRuntime.customWorkdir or null;
        in
        {
          inherit (taskRuntime) slotEnv;
          workdir = if hookWorkdir == null then taskRuntime.workdir else hookWorkdir;
          customWorkdir =
            if hookCustomWorkdir != null then
              hookCustomWorkdir
            else if hookWorkdir == null then
              taskCustomWorkdir
            else if hookWorkdir == "custom" then
              taskCustomWorkdir
            else
              null;
          inherit (taskRuntime) hermetic;
          runtimeInputs = (taskRuntime.runtimeInputs or [ ]) ++ (hook.runtimeInputs or [ ]);
          passThroughEnv = (taskRuntime.passThroughEnv or [ ]) ++ (hook.passThroughEnv or [ ]);
          allowSensitivePassThrough = taskRuntime.allowSensitivePassThrough or false;
          env = (taskRuntime.env or { }) // (hook.env or { });
          umask = taskRuntime.umask or "022";
          locale = taskRuntime.locale or "C.UTF-8";
          timezone = taskRuntime.timezone or "UTC";
        };

      renderRuntimePlanShell =
        runtime:
        let
          env = runtime.env or { };
          envNames = builtins.sort builtins.lessThan (builtins.attrNames env);
          runtimeEnvTsv = builtins.concatStringsSep "\n" (
            map (name: "${name}\t${valueToString env.${name}}") envNames
          );
          passThroughEnvTsv = builtins.concatStringsSep "\n" (runtime.passThroughEnv or [ ]);
        in
        builtins.concatStringsSep "\n" [
          "runtime_inputs_path=${
            lib.escapeShellArg (
              builtins.concatStringsSep ":" (map (path: "${path}/bin") (runtime.runtimeInputs or [ ]))
            )
          }"
          "locale=${lib.escapeShellArg (runtime.locale or "C.UTF-8")}"
          "timezone=${lib.escapeShellArg (runtime.timezone or "UTC")}"
          "umask_value=${lib.escapeShellArg (runtime.umask or "022")}"
          "allow_sensitive_pass_through=${
            lib.escapeShellArg (if runtime.allowSensitivePassThrough or false then "1" else "0")
          }"
          "workdir_kind=${lib.escapeShellArg (runtime.workdir or "projectRoot")}"
          "custom_workdir=${lib.escapeShellArg (runtime.customWorkdir or "")}"
          "task_log_level_default=${lib.escapeShellArg (runtime.logging.levelDefault or "")}"
          "task_output_mode_default=${lib.escapeShellArg (runtime.logging.outputDefault or "")}"
          "runtime_env_tsv=${lib.escapeShellArg runtimeEnvTsv}"
          "pass_through_env_tsv=${lib.escapeShellArg passThroughEnvTsv}"
          "runtime_log_level_set=${lib.escapeShellArg (if env ? LOG_LEVEL then "1" else "0")}"
          "runtime_log_level_alias_set=${lib.escapeShellArg (if env ? NIXFIED_LOG_LEVEL then "1" else "0")}"
          "runtime_log_level_value=${
            lib.escapeShellArg (if env ? LOG_LEVEL then valueToString env.LOG_LEVEL else "")
          }"
          "runtime_log_level_alias_value=${
            lib.escapeShellArg (if env ? NIXFIED_LOG_LEVEL then valueToString env.NIXFIED_LOG_LEVEL else "")
          }"
          "runtime_output_mode_set=${lib.escapeShellArg (if env ? OUTPUT_MODE then "1" else "0")}"
          "runtime_output_mode_alias_set=${
            lib.escapeShellArg (if env ? NIXFIED_OUTPUT_MODE then "1" else "0")
          }"
          "runtime_output_mode_value=${
            lib.escapeShellArg (if env ? OUTPUT_MODE then valueToString env.OUTPUT_MODE else "")
          }"
          "runtime_output_mode_alias_value=${
            lib.escapeShellArg (if env ? NIXFIED_OUTPUT_MODE then valueToString env.NIXFIED_OUTPUT_MODE else "")
          }"
          "runtime_log_file_set=${lib.escapeShellArg (if env ? NIXFIED_LOG_FILE then "1" else "0")}"
          "runtime_log_file_value=${
            lib.escapeShellArg (if env ? NIXFIED_LOG_FILE then valueToString env.NIXFIED_LOG_FILE else "")
          }"
          "runtime_has_rust_log=${lib.escapeShellArg (if env ? RUST_LOG then "1" else "0")}"
          "runtime_has_mfm_log=${lib.escapeShellArg (if env ? MFM_LOG then "1" else "0")}"
          "runtime_has_mfm_test_log_filter=${
            lib.escapeShellArg (if env ? MFM_TEST_LOG_FILTER then "1" else "0")
          }"
          "runtime_has_mfm_test_log=${lib.escapeShellArg (if env ? MFM_TEST_LOG then "1" else "0")}"
        ];

      taskAppIds =
        taskId:
        builtins.sort builtins.lessThan (
          builtins.filter (
            appId:
            let
              app = apps.${appId};
            in
            (app.kind or "") == "taskRef" && (app.taskId or "") == taskId
          ) (builtins.attrNames apps)
        );

      preferredTaskApp =
        taskId:
        let
          appIds = taskAppIds taskId;
        in
        if appIds == [ ] then null else apps.${builtins.head appIds};

      taskDescriptorById = builtins.listToAttrs (
        map (
          taskId:
          let
            task = tasks.${taskId};
            app = preferredTaskApp taskId;
            commandApi = task.commandApi or { };
            specs = map normalizeTaskArgSpec (commandApi.args or [ ]);
            preHookIds = uniqueSorted (builtins.attrNames (task.runtime.preHooks or { }));
            postHookIds = uniqueSorted (builtins.attrNames (task.runtime.postHooks or { }));
            requiredServices = uniquePreserveOrder (task.requirements.services or [ ]);
            displayName = if app == null then taskId else app.id or taskId;
            usageLines =
              let
                configuredUsage = if app == null then [ ] else app.usage or [ ];
              in
              if configuredUsage != [ ] then configuredUsage else [ "nix run .#run-task -- ${taskId} [-- ...]" ];
            exampleLines = if app == null then [ ] else app.examples or [ ];
            helpSummary =
              if app == null then
                commandApi.summary or task.summary
              else
                app.summary or commandApi.summary or task.summary;
            helpDescription =
              if app == null then
                commandApi.details or task.description or ""
              else
                app.description or commandApi.details or task.description or "";
            runtimePlan = mergeTaskRuntimeWithRunnerPackage task;
            hooksForPhase =
              _phase: hooks:
              builtins.listToAttrs (
                map (
                  hookId:
                  let
                    hook = hooks.${hookId};
                    hookRuntime = mergeHookRuntime task hook;
                  in
                  {
                    name = hookId;
                    value = {
                      inherit (hook) command;
                      runtimePlanShell = renderRuntimePlanShell hookRuntime;
                      passThroughEnvNames = hookRuntime.passThroughEnv or [ ];
                    };
                  }
                ) (uniqueSorted (builtins.attrNames hooks))
              );
          in
          {
            name = taskId;
            value = {
              parser = commandApi.commandClass or "typed";
              allowUnknown = (commandApi.commandClass or "typed") == "passthrough";
              hasPositional = builtins.any (spec: spec.kind == "positional") specs;
              inherit requiredServices;
              closureSelectedServices = (compiledExecution.tasks.byId or { }).${taskId}.closureSelectedServices or [ ];
              baseClosureSelectedServices = (compiledExecution.tasks.byId or { }).${taskId}.baseClosureSelectedServices or [ ];
              runner = {
                type = task.runner.type or "shell";
                command = if (task.runner.command or null) == null then "" else task.runner.command;
                package = if (task.runner.package or null) == null then "" else task.runner.package;
                workflowId = task.runner.workflowId or "";
              };
              runtimePlanShell = renderRuntimePlanShell runtimePlan;
              passThroughEnvNames = runtimePlan.passThroughEnv or [ ];
              produces = {
                artifacts = task.produces.artifacts or [ ];
                stateKeys = task.produces.stateKeys or [ ];
              };
              maxAttempts =
                let
                  attempts = task.scheduling.maxAttempts or 1;
                in
                if attempts < 1 then 1 else attempts;
              retryBackoffValues = task.scheduling.retryBackoffSec or [ ];
              deps = {
                needs = task.deps.needs or [ ];
                softNeeds = task.deps.softNeeds or [ ];
              };
              args = {
                inherit specs;
                longKinds = builtins.listToAttrs (
                  builtins.concatLists (
                    map (
                      spec:
                      lib.optionals (spec.long != "") [
                        {
                          name = spec.long;
                          value = spec.kind;
                        }
                      ]
                    ) specs
                  )
                );
                shortKinds = builtins.listToAttrs (
                  builtins.concatLists (
                    map (
                      spec:
                      lib.optionals (spec.short != "") [
                        {
                          name = spec.short;
                          value = spec.kind;
                        }
                      ]
                    ) specs
                  )
                );
              };
              hooks = {
                pre = hooksForPhase "pre" (task.runtime.preHooks or { });
                post = hooksForPhase "post" (task.runtime.postHooks or { });
                preIds = preHookIds;
                postIds = postHookIds;
                count = builtins.length preHookIds + builtins.length postHookIds;
              };
              help = {
                inherit displayName;
                lines = [
                  "${displayName} - ${helpSummary}"
                ]
                ++ lib.optionals (helpDescription != "") [
                  ""
                  helpDescription
                ]
                ++ [
                  ""
                  "Usage:"
                ]
                ++ map (line: "  ${line}") usageLines
                ++ [
                  ""
                  "Options:"
                ]
                ++ map formatTaskArgHelpLine specs
                ++ [
                  "  -h, --help: Show this help."
                ]
                ++ lib.optionals (exampleLines != [ ]) [
                  ""
                  "Examples:"
                ]
                ++ map (line: "  ${line}") exampleLines;
              };
            };
          }
        ) taskIds
      );

      workflowDescriptorById = builtins.listToAttrs (
        map (
          workflowId:
          let
            workflow = workflows.${workflowId};
            workflowExecution = workflow.execution or { };
            ephemeral = workflowExecution.ephemeral or { };
            artifacts = workflow.artifacts or { };
            logging = workflow.logging or { };
            postRun = workflow.postRun or { };
            mode = workflow.mode or "custom";
            explicitEphemeral =
              if (ephemeral ? enable) && ephemeral.enable != null then ephemeral.enable else null;
            effectiveEphemeral =
              if explicitEphemeral != null then
                explicitEphemeral
              else
                builtins.elem mode [
                  "ci"
                  "test"
                ];
            renderWorkflowUnit =
              unit:
              let
                taskId = unit.taskId or "";
                task = if taskId != "" && builtins.hasAttr taskId tasks then tasks.${taskId} else { };
                taskRunner = task.runner or { };
                runnerType = taskRunner.type or "shell";
                runnerWorkflowId = if runnerType == "workflowRef" then taskRunner.workflowId or "" else "";
                unitRequiredServices = unit.requirements.services or [ ];
                taskBaseClosureServices =
                  if taskId != "" then
                    ((compiledExecution.tasks.byId or { }).${taskId}.baseClosureSelectedServices or [ ])
                  else
                    [ ];
                runnerWorkflowClosureServices =
                  if runnerWorkflowId != "" then
                    ((compiledExecution.workflows.byId or { }).${runnerWorkflowId}.closureSelectedServices or [ ])
                  else
                    [ ];
                produces = unit.produces or { };
                when = unit.when or { };
              in
              {
                name = unit.name or "";
                inherit taskId;
                needs = unit.needs or [ ];
                locks = unit.locks or [ ];
                requiredServices = unitRequiredServices;
                skipIfMissingEnv = unit.skipIfMissingEnv or [ ];
                when = {
                  envPresent = when.envPresent or [ ];
                  envEquals = when.envEquals or { };
                };
                selectedServices = uniqueSorted (
                  uniquePreserveOrder (
                    unitRequiredServices ++ taskBaseClosureServices ++ runnerWorkflowClosureServices
                  )
                );
                produces = {
                  artifacts = produces.artifacts or [ ];
                  stateKeys = produces.stateKeys or [ ];
                };
              };
            renderPhaseServiceSet = entry: {
              serviceSetId = entry.serviceSetId or "";
              serviceSetName = entry.serviceSetName or (entry.serviceSetId or "");
              operation = entry.operation or "";
              selectedServices = uniqueSorted (entry.selectedServices or [ ]);
            };
          in
          {
            name = workflowId;
            value = {
              inherit mode;
              family =
                if builtins.match "^workflow\\.([^.]+)\\..+$" workflowId != null then
                  builtins.elemAt (builtins.match "^workflow\\.([^.]+)\\..+$" workflowId) 0
                else
                  "";
              artifactsRoot = artifacts.root or "";
              ephemeralEnabled = effectiveEphemeral;
              logging = {
                levelDefault = logging.levelDefault or "";
                outputDefault = logging.outputDefault or "";
              };
              failFast = workflowExecution.failFast or false;
              parallelEnabled = workflowExecution.parallel or false;
              maxWorkers = workflow.maxWorkers or 1;
              lockPolicy = workflowExecution.lockPolicy or "exclusive";
              writeSummary = artifacts.writeSummary or false;
              postRunAlways = postRun.alwaysRun or false;
              closureSelectedServices = (compiledExecution.workflows.byId or { }).${workflowId}.closureSelectedServices or [ ];
              unitClosureSelectedServices = (compiledExecution.workflows.byId or { }).${workflowId}.unitClosureSelectedServices or [ ];
              referenceClosureSelectedServices = (compiledExecution.workflows.byId or { }).${workflowId}.referenceClosureSelectedServices or [ ];
              plan = map renderWorkflowUnit (workflow.plan or [ ]);
              phases = {
                preRun = {
                  tasks = workflow.preRun.tasks or [ ];
                  serviceSets = map renderPhaseServiceSet (workflow.preRun.serviceSets or [ ]);
                };
                postRun = {
                  tasks = workflow.postRun.tasks or [ ];
                  serviceSets = map renderPhaseServiceSet (workflow.postRun.serviceSets or [ ]);
                };
              };
            };
          }
        ) workflowIds
      );
    in
    {
      tasks = taskDescriptorById;
      workflows = workflowDescriptorById;
    };

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
    _: task: uniquePreserveOrder ((task.requirements or { }).services or [ ])
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

  inherit (fullWalker) goTask;
  inherit (fullWalker) goWorkflow;
  inherit (fullWalker) goWorkflowExact;
  inherit (fullWalker) goWorkflowReference;

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

  mkExecutionProjection =
    {
      projectionTaskIds,
      projectionWorkflowIds,
      projectionTasks,
      projectionWorkflows,
      projectionServiceCatalog,
    }:
    let
      projectionWorkflowFamilies = uniqueSorted (
        builtins.filter (family: family != "") (map workflowFamilyFromId projectionWorkflowIds)
      );
      projectionWorkflowIdsByFamily = builtins.listToAttrs (
        map (family: {
          name = family;
          value = builtins.filter (
            workflowId: workflowFamilyFromId workflowId == family
          ) projectionWorkflowIds;
        }) projectionWorkflowFamilies
      );
      projectionWorkflowModesByFamily = builtins.listToAttrs (
        map (family: {
          name = family;
          value = uniqueSorted (map workflowModeFromId (projectionWorkflowIdsByFamily.${family} or [ ]));
        }) projectionWorkflowFamilies
      );
      projectionExecution = {
        inherit (executionBase) schema;
        enabledServices = uniqueSorted (
          map (
            serviceId:
            let
              service = projectionServiceCatalog.${serviceId};
            in
            service.name or serviceId
          ) (builtins.attrNames projectionServiceCatalog)
        );
        taskIds = projectionTaskIds;
        workflowIds = projectionWorkflowIds;
        workflowFamilies = projectionWorkflowFamilies;
        workflowIdsByFamily = projectionWorkflowIdsByFamily;
        workflowModesByFamily = projectionWorkflowModesByFamily;
        tasks = {
          byId = lib.getAttrs projectionTaskIds executionBase.tasks.byId;
        };
        workflows = {
          byId = lib.getAttrs projectionWorkflowIds executionBase.workflows.byId;
        };
      };
      executionDescriptors = buildExecutionDescriptors {
        apps = appSet;
        compiledExecution = projectionExecution;
        tasks = projectionTasks;
        workflows = projectionWorkflows;
      };
    in
    canonical.canonicalize (
      projectionExecution
      // {
        tasks = {
          byId = builtins.mapAttrs (
            taskId: taskExecution: taskExecution // (executionDescriptors.tasks.${taskId} or { })
          ) projectionExecution.tasks.byId;
        };
        workflows = {
          byId = builtins.mapAttrs (
            workflowId: workflowExecution:
            workflowExecution // (executionDescriptors.workflows.${workflowId} or { })
          ) projectionExecution.workflows.byId;
        };
      }
    );

  compiledExecution = mkExecutionProjection {
    projectionTaskIds = taskIds;
    projectionWorkflowIds = workflowIds;
    projectionTasks = taskSet;
    projectionWorkflows = workflowSet;
    projectionServiceCatalog = catalog;
  };

  serviceSetExecutionById = builtins.mapAttrs (name: serviceSet: {
    id = serviceSet.id or name;
    name = serviceSet.name or name;
    summary = serviceSet.summary or "";
    services = serviceSet.services or { };
    requiredServices = serviceSet.services.required or [ ];
    optionalServices = serviceSet.services.optional or [ ];
    allServices = serviceSet.services.all or [ ];
    defaultOperation = serviceSet.defaultOperation or "health";
  }) serviceSetCatalog;
in
canonical.canonicalize (
  compiledExecution
  // {
    serviceSets = {
      ids = serviceSetIds;
      byId = serviceSetExecutionById;
    };
  }
)
