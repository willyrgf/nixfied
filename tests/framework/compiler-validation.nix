{
  pkgs,
  model,
}:
let
  taskIds = builtins.attrNames model.tasks;
  workflowIds = builtins.attrNames model.workflows;

  tasksHaveStableIds = builtins.all (
    taskId:
    let
      task = model.tasks.${taskId};
    in
    task.id == taskId && builtins.substring 0 5 task.id == "task."
  ) taskIds;

  workflowsReferenceKnownTasks = builtins.all (
    workflowId:
    let
      workflow = model.workflows.${workflowId};
    in
    builtins.all (unit: builtins.hasAttr unit.taskId model.tasks) workflow.plan
  ) workflowIds;

  workflowsHaveLifecycle = builtins.all (
    workflowId:
    let
      workflow = model.workflows.${workflowId};
    in
    workflow ? preRun && workflow ? postRun && workflow.postRun ? alwaysRun
  ) workflowIds;

  frameworkTask = model.tasks."task.framework.test" or null;
  formatTask = model.tasks."task.format" or null;
  commandSurfaces = model.views.help.commandSurfaces or [ ];
  nginxService = model.services."service.nginx" or null;
  heliosService = model.services."service.helios" or null;
  hasCommandSurface =
    name: ownerFile:
    builtins.any (entry: entry.name == name && entry.owner_file == ownerFile) commandSurfaces;
in
assert frameworkTask != null;
assert frameworkTask.runner.type == "shell";
assert frameworkTask.ui.app.name == "framework::test";
assert formatTask != null;
assert model.identity.projectName == "Nixfied Project";
assert model.identity.description == "Reusable Nix development framework";
assert model.runtime ? ephemeral;
assert model.runtime.ephemeral.copyMode == "nix-source";
assert builtins.isBool model.runtime.ephemeral.includeUntracked;
assert builtins.isList model.runtime.ephemeral.excludePatterns;
assert builtins.isList model.runtime.ephemeral.extraDirs;
assert builtins.isBool model.runtime.ephemeral.keepFailures;
assert builtins.isInt model.runtime.ephemeral.maxFailedRoots;
assert builtins.isInt model.runtime.ephemeral.maxFailedRootAgeHours;
assert builtins.isInt model.runtime.ephemeral.maxCopyBytes;
assert builtins.isInt model.runtime.ephemeral.minFreeBytesAfterCopy;
assert model.runtime.ephemeral.envFileMode == "disabled";
assert model.runtime.ephemeral.envFilePath == ".env";
assert model.runtime.runtimePackages != [ ];
assert nginxService != null;
assert heliosService != null;
assert nginxService.config.resolved.operationProbes.health.count == 2;
assert heliosService.config.resolved.operationProbes.ready.count == 2;
assert builtins.all (
  pkg: builtins.elem pkg formatTask.runtime.runtimeInputs
) model.runtime.runtimePackages;
assert formatTask.runtime ? preHooks;
assert formatTask.runtime ? postHooks;
assert formatTask.runtime.postHooks ? "framework.nixfmt";
assert pkgs.lib.hasInfix "nixfmt --" formatTask.runtime.postHooks."framework.nixfmt".command;
assert commandSurfaces != [ ];
assert hasCommandSurface "dev" "nixfied/project/module.nix";
assert hasCommandSurface "validate-env" "nixfied/modules/operations.nix";
assert hasCommandSurface "framework::test" "nixfied/project/module.nix";
assert tasksHaveStableIds;
assert workflowsReferenceKnownTasks;
assert workflowsHaveLifecycle;
pkgs.runCommand "compiler-validation" { } ''
  echo "OK: compiler task and workflow contracts are stable" > "$out"
''
