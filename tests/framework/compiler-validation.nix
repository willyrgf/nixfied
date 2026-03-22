{
  pkgs,
  model,
  services,
  serviceCatalog,
}:
let
  taskIds = builtins.attrNames model.tasks;
  workflowIds = builtins.attrNames model.workflows;
  serviceIds = builtins.attrNames serviceCatalog;
  featureIds = builtins.attrNames (model.features or { });
  expectedRuntimeFeatureIds = [
    "runtime.ephemeral.source-materialization"
    "runtime.ephemeral.include-untracked"
    "runtime.ephemeral.env-file-loading"
    "runtime.registry.isolation"
    "runtime.output.prefix-contract"
  ];

  featureKinds = [
    "runtime"
    "service"
    "task"
    "workflow"
  ];

  tasksHaveStableIds = builtins.all (
    taskId:
    let
      task = model.tasks.${taskId};
    in
    task.id == taskId && builtins.substring 0 5 task.id == "task."
  ) taskIds;

  tasksHaveServiceRequirements = builtins.all (
    taskId:
    let
      task = model.tasks.${taskId};
    in
    builtins.isList ((((task.requirements or { }).services))) && !(task ? serviceName)
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

  workflowsHaveServiceRequirements = builtins.all (
    workflowId:
    let
      workflow = model.workflows.${workflowId};
    in
    builtins.all (
      unitName:
      let
        unit = workflow.units.${unitName};
      in
      builtins.isList ((((unit.requirements or { }).services))) && !(unit ? serviceName)
    ) (builtins.attrNames workflow.units)
  ) workflowIds;

  servicesHaveStableIds = builtins.all (
    serviceId:
    let
      service = serviceCatalog.${serviceId};
    in
    service.id == serviceId && builtins.substring 0 8 service.id == "service."
  ) serviceIds;

  featuresHaveStableIds = builtins.all (
    featureId:
    let
      feature = model.features.${featureId};
      docs = feature.docs or [ ];
    in
    feature.id == featureId
    && builtins.elem (feature.kind or "") featureKinds
    && builtins.isString (feature.summary or "")
    && builtins.isList (feature.ownerFiles or [ ])
    && builtins.isList (feature.modelPaths or [ ])
    && builtins.isBool (feature.coverageRequired or false)
    && builtins.isList docs
  ) featureIds;

  serviceFeaturesPresent = builtins.all (
    serviceId: builtins.hasAttr serviceId model.features
  ) serviceIds;
  workflowFeaturesPresent = builtins.all (
    workflowId: builtins.hasAttr workflowId model.features
  ) workflowIds;
  exposedTaskFeaturesPresent = builtins.all (
    appName:
    let
      taskId = model.views.apps.${appName}.taskId;
    in
    taskId == null || builtins.hasAttr taskId model.features
  ) (builtins.attrNames (model.views.apps or { }));

  runtimeFeaturesPresent = builtins.all (
    featureId: builtins.hasAttr featureId model.features
  ) expectedRuntimeFeatureIds;

  frameworkTask = model.tasks."task.framework.test" or null;
  formatTask = model.tasks."task.format" or null;
  checkTask = model.tasks."task.check" or null;
  testTask = model.tasks."task.test" or null;
  ciTask = model.tasks."task.ci" or null;
  qualityTask = model.tasks."task.ci.quality" or null;
  isolationProbeTask = model.tasks."task.test.isolation.probe" or null;
  isolationProbeUnit = model.tasks."task.test.isolation.unit" or null;
  isolationProbeWorkflow = model.workflows."workflow.test.isolation.probe" or null;
  commandSurfaces = model.views.help.commandSurfaces or [ ];
  featureView = model.views.features or null;
  nginxService = services."service.nginx" or null;
  heliosService = services."service.helios" or null;
  hasCommandSurface =
    name: ownerFile:
    builtins.any (entry: entry.name == name && entry.owner_file == ownerFile) commandSurfaces;
in
assert frameworkTask != null;
assert frameworkTask.runner.type == "shell";
assert model.apps."framework::test".taskId == frameworkTask.id;
assert formatTask != null;
assert checkTask != null;
assert testTask != null;
assert ciTask != null;
assert qualityTask != null;
assert isolationProbeTask != null;
assert isolationProbeUnit != null;
assert isolationProbeWorkflow != null;
assert model ? features;
assert model.identity.projectName == "Nixfied Project";
assert model.identity.description == "Reusable Nix development framework";
assert model ? state;
assert model.state ? policy;
assert model.state.policy.id == "workspace-scoped";
assert model.state.policy.kind == "workspace-scoped";
assert builtins.isString model.state.policy.workspaceId;
assert model.state.policy.runtimeBase == model.runtime.directories.base;
assert model.runtime ? ephemeral;
assert model.runtime ? orchestrator;
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
assert model.runtime.orchestrator.stopTimeoutSec == 5;
assert model.runtime.runtimePackages != [ ];
assert nginxService != null;
assert heliosService != null;
assert nginxService.config.resolved.operationProbes.health.count == 2;
assert nginxService.config.resolved.probePlans.health.count == 2;
assert
  nginxService.config.resolved.probePlans.health.count
  == nginxService.config.resolved.operationProbes.health.count;
assert heliosService.config.resolved.operationProbes.ready.count == 2;
assert heliosService.config.resolved.probePlans.ready.count == 2;
assert heliosService.config.resolved.probePlans.ready.wait.enabled == true;
assert
  heliosService.config.resolved.probePlans.ready.wait.timeoutEnvVar == "HELIOS_READY_TIMEOUT_SECS";
assert
  heliosService.config.resolved.probePlans.ready.wait.intervalEnvVar == "HELIOS_READY_INTERVAL_SECS";
assert builtins.all (
  pkg: builtins.elem pkg formatTask.runtime.runtimeInputs
) model.runtime.runtimePackages;
assert formatTask.runtime ? preHooks;
assert formatTask.runtime ? postHooks;
assert formatTask.runtime.postHooks ? "framework.nixfmt";
assert pkgs.lib.hasInfix "nixfmt --" formatTask.runtime.postHooks."framework.nixfmt".command;
assert checkTask.runner.type == "derivation";
assert checkTask.runner.command == "nix-checks";
assert pkgs.lib.hasInfix "nix-checks" (checkTask.runner.package or "");
assert testTask.runner.type == "workflowRef";
assert testTask.runner.workflowId == "workflow.ci.full";
assert ciTask.runner.type == "workflowRef";
assert ciTask.runner.workflowId == "workflow.ci.full";
assert qualityTask.runner.type == "derivation";
assert qualityTask.runner.command == "nix-checks --mode full";
assert pkgs.lib.hasInfix "nix-checks" (qualityTask.runner.package or "");
assert isolationProbeTask.runner.type == "workflowRef";
assert isolationProbeTask.runner.workflowId == "workflow.test.isolation.probe";
assert isolationProbeUnit.runner.type == "shell";
assert isolationProbeWorkflow.units.probe.taskId == "task.test.isolation.unit";
assert commandSurfaces != [ ];
assert featureView != null;
assert featureView ? lines;
assert hasCommandSurface "dev" "nixfied/project/tasks.nix";
assert hasCommandSurface "validate-env" "nixfied/modules/operations.nix";
assert hasCommandSurface "framework::test" "nixfied/framework/presets/framework-test.nix";
assert hasCommandSurface "features" "nixfied/framework/runtime/dispatcher.nix";
assert hasCommandSurface "introspect" "nixfied/framework/core/mkCoreSurfaces.nix";
assert tasksHaveStableIds;
assert tasksHaveServiceRequirements;
assert workflowsReferenceKnownTasks;
assert workflowsHaveLifecycle;
assert workflowsHaveServiceRequirements;
assert servicesHaveStableIds;
assert featureIds != [ ];
assert featuresHaveStableIds;
assert serviceFeaturesPresent;
assert workflowFeaturesPresent;
assert exposedTaskFeaturesPresent;
assert runtimeFeaturesPresent;
pkgs.runCommand "compiler-validation" { } ''
  echo "OK: compiler task, workflow, and feature contracts are stable" > "$out"
''
