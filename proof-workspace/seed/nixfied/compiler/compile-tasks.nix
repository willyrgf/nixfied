{
  lib,
  canonical,
  idLib,
}:
{
  resolved,
  ...
}:
let
  listUtils = import ../framework/core/list-utils.nix;
  rawTasks = resolved.tasks or { };
  names = builtins.sort builtins.lessThan (builtins.attrNames rawTasks);
  globalRuntimeInputs = map builtins.toString (resolved.tooling.runtimePackages or [ ]);
  excludedServices = resolved.graph.excludedServices or [ ];

  normalizeHooks =
    hooks:
    builtins.mapAttrs (_: hook: {
      inherit (hook) command;
      runtimeInputs = map builtins.toString hook.runtimeInputs;
      inherit (hook) passThroughEnv;
      inherit (hook) env;
      inherit (hook) workdir;
      inherit (hook) customWorkdir;
    }) hooks;

  normalizeId =
    name: rawId:
    let
      effective = if rawId == "" || rawId == name then "task.${idLib.sanitize name}" else rawId;
    in
    idLib.ensurePrefix "task" effective;

  normalizeTaskBase =
    name:
    let
      raw = rawTasks.${name};
      id = normalizeId name raw.id;
      requiredServices = listUtils.uniquePreserveOrder (raw.requirements.services or [ ]);
    in
    canonical.canonicalize {
      inherit id;

      inherit (raw) kind;
      requirements = raw.requirements // {
        services = requiredServices;
      };
      inherit (raw) summary;
      inherit (raw) description;
      inherit (raw) tags;

      runner = {
        inherit (raw.runner) type;
        inherit (raw.runner) command;
        package = if raw.runner.package == null then null else builtins.toString raw.runner.package;
        inherit (raw.runner) workflowId;
      };

      inherit (raw) commandApi;
      inherit (raw) launcher;

      runtime = {
        inherit (raw.runtime) slotEnv;
        inherit (raw.runtime) workdir;
        inherit (raw.runtime) customWorkdir;
        inherit (raw.runtime) hermetic;
        runtimeInputs = listUtils.uniquePreserveOrder (
          map builtins.toString raw.runtime.runtimeInputs ++ globalRuntimeInputs
        );
        inherit (raw.runtime) passThroughEnv;
        references = {
          taskIds = raw.runtime.references.taskIds or [ ];
          workflowIds = raw.runtime.references.workflowIds or [ ];
        };
        inherit (raw.runtime) allowSensitivePassThrough;
        inherit (raw.runtime) logging;
        inherit (raw.runtime) env;
        inherit (raw.runtime) umask;
        inherit (raw.runtime) locale;
        inherit (raw.runtime) timezone;
        preHooks = normalizeHooks raw.runtime.preHooks;
        postHooks = normalizeHooks raw.runtime.postHooks;
      };

      inherit (raw) scheduling;
      inherit (raw) deps;
      inherit (raw) produces;
    };

  addTask =
    acc: name:
    let
      task = normalizeTaskBase name;
      inherit (task) id;
    in
    if builtins.hasAttr id acc then
      if canonical.toCanonicalNix acc.${id} == canonical.toCanonicalNix task then
        acc
      else
        throw "task id collision for '${id}'"
    else
      acc
      // {
        ${id} = task;
      };

  tasksByIdRaw = builtins.foldl' addTask { } names;

  normalizeTaskDepId =
    depTaskId:
    if depTaskId == "" then
      depTaskId
    else if builtins.hasAttr depTaskId tasksByIdRaw then
      depTaskId
    else
      idLib.ensurePrefix "task" depTaskId;

  normalizeWorkflowId =
    workflowId: if workflowId == "" then workflowId else idLib.ensurePrefix "workflow" workflowId;

  tasksById = builtins.mapAttrs (
    _: task:
    let
      normalizedNeeds = listUtils.uniquePreserveOrder (map normalizeTaskDepId (task.deps.needs or [ ]));
      normalizedSoftNeeds = listUtils.uniquePreserveOrder (
        map normalizeTaskDepId (task.deps.softNeeds or [ ])
      );
      normalizedRuntimeTaskRefs = listUtils.uniquePreserveOrder (
        map normalizeTaskDepId (task.runtime.references.taskIds or [ ])
      );
      normalizedRuntimeWorkflowRefs = listUtils.uniquePreserveOrder (
        map normalizeWorkflowId (task.runtime.references.workflowIds or [ ])
      );
    in
    canonical.canonicalize (
      task
      // {
        deps = task.deps // {
          needs = normalizedNeeds;
          softNeeds = normalizedSoftNeeds;
        };
        runtime = task.runtime // {
          references = {
            taskIds = normalizedRuntimeTaskRefs;
            workflowIds = normalizedRuntimeWorkflowRefs;
          };
        };
      }
    )
  ) tasksByIdRaw;

  ids = builtins.sort builtins.lessThan (builtins.attrNames tasksById);

  initialPruneReasons = builtins.listToAttrs (
    builtins.concatLists (
      map (
        taskId:
        let
          task = tasksById.${taskId};
          excludedRequirements = builtins.filter (
            serviceName: builtins.elem serviceName excludedServices
          ) task.requirements.services;
        in
        if excludedRequirements != [ ] then
          [
            {
              name = taskId;
              value = {
                reason = "service-excluded";
                serviceName = builtins.head excludedRequirements;
                serviceNames = excludedRequirements;
              };
            }
          ]
        else
          [ ]
      ) ids
    )
  );

  pruneTasksUntilStable =
    pruneReasonsByTaskId:
    let
      nextPruneReasons = builtins.listToAttrs (
        builtins.concatLists (
          map (
            taskId:
            if builtins.hasAttr taskId pruneReasonsByTaskId then
              [ ]
            else
              let
                task = tasksById.${taskId};
                blockingNeeds = builtins.filter (depTaskId: builtins.hasAttr depTaskId pruneReasonsByTaskId) (
                  task.deps.needs or [ ]
                );
              in
              if blockingNeeds == [ ] then
                [ ]
              else
                [
                  {
                    name = taskId;
                    value = {
                      reason = "required-task-pruned";
                      dependency = builtins.head blockingNeeds;
                    };
                  }
                ]
          ) ids
        )
      );
    in
    if nextPruneReasons == { } then
      pruneReasonsByTaskId
    else
      pruneTasksUntilStable (pruneReasonsByTaskId // nextPruneReasons);

  pruneReasonsByTaskId = pruneTasksUntilStable initialPruneReasons;
  prunedTaskIds = builtins.sort builtins.lessThan (builtins.attrNames pruneReasonsByTaskId);
  survivingTasks = lib.removeAttrs tasksById prunedTaskIds;
in
{
  allTasks = tasksById;
  tasks = survivingTasks;
  declaredTaskIds = ids;
  inherit prunedTaskIds;
  inherit pruneReasonsByTaskId;
}
