{
  lib,
}:
{
  tasks,
  workflows,
  serviceCatalog,
  apps ? { },
  compiledExecution,
}:
let
  listUtils = import ../framework/core/list-utils.nix;
  uniquePreserveOrder = listUtils.uniquePreserveOrder;
  uniqueSorted = listUtils.uniqueSorted;

  taskIds = uniqueSorted (builtins.attrNames tasks);
  workflowIds = uniqueSorted (builtins.attrNames workflows);

  workflowModesByFamily = compiledExecution.workflowModesByFamily or { };
  workflowFamilies =
    if compiledExecution ? workflowFamilies then
      compiledExecution.workflowFamilies
    else
      uniqueSorted (builtins.attrNames workflowModesByFamily);

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
      slotEnv = taskRuntime.slotEnv;
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
      hermetic = taskRuntime.hermetic;
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
        requiredServices = uniquePreserveOrder ((task.requirements.services or [ ]));
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
          phase: hooks:
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
                  command = hook.command;
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
          requiredServices = requiredServices;
          closureSelectedServices =
            ((compiledExecution.tasks.byId or { }).${taskId}.closureSelectedServices or [ ]);
          baseClosureSelectedServices =
            ((compiledExecution.tasks.byId or { }).${taskId}.baseClosureSelectedServices or [ ]);
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
            specs = specs;
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
            displayName = displayName;
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
            taskId = taskId;
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
          mode = mode;
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
          closureSelectedServices =
            ((compiledExecution.workflows.byId or { }).${workflowId}.closureSelectedServices or [ ]);
          unitClosureSelectedServices =
            ((compiledExecution.workflows.byId or { }).${workflowId}.unitClosureSelectedServices or [ ]);
          referenceClosureSelectedServices =
            ((compiledExecution.workflows.byId or { }).${workflowId}.referenceClosureSelectedServices or [ ]);
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
  schema = {
    kind = "nixfied-runtime-metadata";
    version = 1;
  };
  tasks = taskDescriptorById;
  workflows = workflowDescriptorById;
  workflowFamilies = builtins.listToAttrs (
    map (family: {
      name = family;
      value = {
        modes = workflowModesByFamily.${family} or [ ];
      };
    }) workflowFamilies
  );
  availableServices = uniqueSorted (
    map (
      serviceId:
      let
        service = serviceCatalog.${serviceId};
      in
      service.name or serviceId
    ) (builtins.attrNames serviceCatalog)
  );
}
