{
  pkgs,
  model,
  serviceDefinitions,
  serviceCatalog,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    system = pkgs.system;
  };
  serviceConfigLib = import ../../nixfied/framework/core/service-config.nix {
    inherit pkgs;
    lib = pkgs.lib;
  };
  testCatalog = import ../../nixfied/framework/testing/catalog.nix;
  taskIds = builtins.attrNames model.tasks;
  workflowIds = builtins.attrNames model.workflows;
  serviceIds = builtins.attrNames serviceCatalog;
  featureIds = builtins.attrNames (model.features or { });
  requiredCompileFeatureIds = builtins.sort builtins.lessThan (
    builtins.filter (
      featureId:
      let
        feature = model.features.${featureId};
      in
      (feature.coverageRequired or false) && (feature.coverageLayer or "") == "compile"
    ) featureIds
  );
  expectedCompileFeatureIds = builtins.sort builtins.lessThan (
    builtins.filter (featureId: featureId != null && featureId != "") (
      builtins.concatLists [
        (builtins.map (appName: model.views.apps.${appName}.taskId) (
          builtins.attrNames (model.views.apps or { })
        ))
        workflowIds
        serviceIds
      ]
    )
  );
  safeRepoRoot = builtins.unsafeDiscardStringContext (builtins.toString ../..);
  expectedRuntimeFeatureIds = [
    "runtime.ephemeral.source-materialization"
    "runtime.ephemeral.include-untracked"
    "runtime.ephemeral.env-file-loading"
    "runtime.registry.isolation"
    "runtime.service-operations"
    "runtime.workflow-service-phases"
    "runtime.output.prefix-contract"
  ];

  featureKinds = [
    "runtime"
    "service"
    "task"
    "workflow"
  ];

  featureCoverageLayers = [
    "compile"
    "manifest"
    "kernel"
    "adapter"
    "e2e"
  ];

  nonEmptyShardNames =
    profileName:
    builtins.filter (
      shardName: (testCatalog.profileShardChecks.${profileName}.${shardName} or [ ]) != [ ]
    ) testCatalog.order;

  expectedFrameworkTaskIds = builtins.concatLists (
    map (
      profileName:
      map (shardName: "task.test.framework.${profileName}.${shardName}") (nonEmptyShardNames profileName)
    ) testCatalog.profileNames
  );

  expectedFrameworkWorkflowIds = map (
    profileName: "workflow.test.${profileName}"
  ) testCatalog.profileNames;

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
    && (
      let
        coverageLayer = feature.coverageLayer or null;
      in
      coverageLayer == null
      || (builtins.isString coverageLayer && builtins.elem coverageLayer featureCoverageLayers)
    )
    && (
      if feature.coverageRequired or false then
        builtins.isString (feature.coverageLayer or null)
      else
        true
    )
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
  featuresWithMissingOwnerFiles = builtins.filter (
    featureId:
    let
      feature = model.features.${featureId};
      ownerFiles = feature.ownerFiles or [ ];
    in
    builtins.any (path: !builtins.pathExists "${safeRepoRoot}/${path}") ownerFiles
  ) featureIds;

  formatTask = model.tasks."task.format" or null;
  checkTask = model.tasks."task.check" or null;
  testTask = model.tasks."task.test" or null;
  ciTask = model.tasks."task.ci" or null;
  qualityTask = model.tasks."task.ci.quality" or null;
  validateEnvTask = model.tasks."task.ops.validate-env" or null;
  testIsolationTask = model.tasks."task.ops.test-isolation" or null;
  selfhostTask = model.tasks."task.test.framework.selfhost" or null;
  selfhostWorkflow = model.workflows."workflow.test.framework.selfhost" or null;
  isolationProbeTask = model.tasks."task.test.isolation.probe" or null;
  isolationProbeUnit = model.tasks."task.test.isolation.unit" or null;
  isolationProbeWorkflow = model.workflows."workflow.test.isolation.probe" or null;
  commandSurfaces = model.views.help.commandSurfaces or [ ];
  featureView = model.views.features or null;
  normalizeServiceDefinition =
    serviceName:
    let
      serviceDefinition = serviceDefinitions.${serviceName} or null;
      serviceEnabled = if serviceDefinition == null then false else serviceDefinition.enable or false;
      rawConfig =
        if serviceDefinition == null then null else pkgs.lib.removeAttrs serviceDefinition [ "enable" ];
      configInput =
        if rawConfig == null then
          null
        else if serviceEnabled then
          rawConfig
        else
          rawConfig // { defaultSource = ""; };
    in
    if configInput == null then
      null
    else
      {
        enable = serviceEnabled;
        config = serviceConfigLib.normalizeServiceConfig {
          name = serviceName;
          config = configInput;
        };
      };
  nginxService = normalizeServiceDefinition "nginx";
  heliosService = normalizeServiceDefinition "helios";
  invalidMachineOutputContractRef = builtins.tryEval (
    builtins.deepSeq ((frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        {
          nixfied.machineOutputs."test-invalid-machine-output-contract" = {
            id = "test-invalid-machine-output-contract";
            targetAppId = "check";
            validation.contractRef = "missing.machine-output.contract";
            summary = "invalid machine-output contract";
            description = "Compiler validation coverage for unknown machine-output contracts.";
            usage = [ "nix run .#test-invalid-machine-output-contract" ];
            ownerFile = "tests/framework/compiler-validation.nix";
          };
        }
      ];
      localOverrides = [ ];
    }).model.apps
    ) true
  );
  hasCommandSurface = name: builtins.any (entry: entry.name == name) commandSurfaces;
  findCommandArg =
    task: name:
    let
      matches = builtins.filter (arg: (arg.name or "") == name) (task.commandApi.args or [ ]);
    in
    if matches == [ ] then null else builtins.head matches;
  testModeArg = if testTask == null then null else findCommandArg testTask "mode";
in
assert model ? compiled;
assert model.compiled ? apiCatalog;
assert model.compiled ? execution;
assert !(model.compiled ? runtimeMetadata);
assert !(model.compiled ? runtimeManifests);
assert model.compiled.execution.tasks.byId."task.check".runner.type == "derivation";
assert model.compiled.execution.workflows.byId."workflow.ci.full".family == "ci";
assert model.compiled ? serviceSurfaceCatalog;
assert builtins.isList (model.compiled.serviceSurfaceCatalog.appNames or [ ]);
assert formatTask != null;
assert checkTask != null;
assert testTask != null;
assert ciTask != null;
assert qualityTask != null;
assert validateEnvTask != null;
assert testIsolationTask != null;
assert selfhostTask != null;
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
assert testTask.runner.workflowId == "workflow.test.full";
assert testModeArg != null;
assert
  testModeArg.values == [
    "feature-proof"
    "ci"
    "full"
  ];
assert !(builtins.any (arg: (arg.long or "") == "--basic") (testTask.commandApi.args or [ ]));
assert ciTask.runner.type == "workflowRef";
assert ciTask.runner.workflowId == "workflow.ci.full";
assert qualityTask.runner.type == "derivation";
assert qualityTask.runner.command == "nix-checks --mode full";
assert pkgs.lib.hasInfix "nix-checks" (qualityTask.runner.package or "");
assert
  testIsolationTask.runtime.references.taskIds == [
    "task.ops.validate-env"
    "task.test.isolation.probe"
  ];
assert testIsolationTask.runtime.references.workflowIds == [ ];
assert selfhostTask.runtime.references.taskIds == [ "task.dev" ];
assert selfhostTask.runtime.references.workflowIds == [ "workflow.ci.basic" ];
assert selfhostWorkflow != null;
assert selfhostWorkflow.units.main.taskId == "task.test.framework.selfhost";
assert isolationProbeTask.runner.type == "workflowRef";
assert isolationProbeTask.runner.workflowId == "workflow.test.isolation.probe";
assert isolationProbeUnit.runner.type == "shell";
assert isolationProbeWorkflow.units.probe.taskId == "task.test.isolation.unit";
assert commandSurfaces != [ ];
assert featureView != null;
assert featureView ? lines;
assert hasCommandSurface "dev";
assert hasCommandSurface "validate-env";
assert hasCommandSurface "test";
assert !hasCommandSurface "framework::test";
assert !(builtins.hasAttr "framework::test" model.apps);
assert builtins.all (taskId: builtins.hasAttr taskId model.tasks) expectedFrameworkTaskIds;
assert builtins.all (
  workflowId: builtins.hasAttr workflowId model.workflows
) expectedFrameworkWorkflowIds;
assert hasCommandSurface "features";
assert hasCommandSurface "introspect";
assert tasksHaveStableIds;
assert tasksHaveServiceRequirements;
assert workflowsReferenceKnownTasks;
assert workflowsHaveLifecycle;
assert workflowsHaveServiceRequirements;
assert servicesHaveStableIds;
assert featureIds != [ ];
assert featuresWithMissingOwnerFiles == [ ];
assert featuresHaveStableIds;
assert serviceFeaturesPresent;
assert workflowFeaturesPresent;
assert exposedTaskFeaturesPresent;
assert runtimeFeaturesPresent;
assert requiredCompileFeatureIds == expectedCompileFeatureIds;
assert invalidMachineOutputContractRef.success == false;
pkgs.runCommand "compiler-validation" { } ''
  echo "OK: compiler task, workflow, and feature contracts are stable" > "$out"
''
