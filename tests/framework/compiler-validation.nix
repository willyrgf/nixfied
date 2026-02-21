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
    workflow ? preRun
    && workflow ? postRun
    && workflow.postRun ? alwaysRun
  ) workflowIds;

  frameworkTask = model.tasks."task.framework.test" or null;
  formatTask = model.tasks."task.format" or null;
in
assert frameworkTask != null;
assert frameworkTask.runner.type == "shell";
assert frameworkTask.ui.app.name == "framework::test";
assert formatTask != null;
assert model.identity.projectName == "Nixfied Project";
assert model.identity.description == "Reusable Nix development framework";
assert model.runtime ? ephemeral;
assert model.runtime.ephemeral.copyMode == "git-files";
assert builtins.isList model.runtime.ephemeral.excludePatterns;
assert builtins.isList model.runtime.ephemeral.extraDirs;
assert builtins.isBool model.runtime.ephemeral.keepFailures;
assert builtins.isInt model.runtime.ephemeral.maxFailedRoots;
assert builtins.isInt model.runtime.ephemeral.maxFailedRootAgeHours;
assert model.runtime.runtimePackages != [ ];
assert builtins.all (pkg: builtins.elem pkg formatTask.runtime.runtimeInputs) model.runtime.runtimePackages;
assert formatTask.runtime ? preHooks;
assert formatTask.runtime ? postHooks;
assert formatTask.runtime.postHooks ? "framework.nixfmt";
assert pkgs.lib.hasInfix "nixfmt --" formatTask.runtime.postHooks."framework.nixfmt".command;
assert tasksHaveStableIds;
assert workflowsReferenceKnownTasks;
assert workflowsHaveLifecycle;
pkgs.runCommand "compiler-validation" { } ''
  echo "OK: compiler task and workflow contracts are stable" > "$out"
''
