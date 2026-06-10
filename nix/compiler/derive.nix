{
  lib,
  pkgs,
  system,
  constants,
  config,
}:

let
  inherit (lib) mapAttrs mapAttrsToList attrNames;
  targetLib = import ../lib/target.nix { inherit lib; };
  identifiers = import ../lib/identifiers.nix;

  surfaceNames = [
    "model"
    "schema"
    "docs"
    "capabilities"
    "check"
    "run"
    "ps"
    "down"
    "clean"
  ];
  surfaceSpec = name: {
    inherit name;
    aliases = [ ];
    inputSchema = { };
    outputSchema = { };
    exitClasses = [
      "ok"
      "error"
    ];
    evaluationPermission = "never";
    maturity = "stable";
  };

  target = targetLib.fromSystem config.nixfied.target.system;

  slotPolicy = config.nixfied.slotPolicy;
  slots = lib.range slotPolicy.min slotPolicy.max;
  portPolicy = config.nixfied.placement.ports;
  slotWindow =
    slot:
    let
      start = portPolicy.base + (slot * portPolicy.slotStride);
    in
    {
      inherit start;
      end = start + portPolicy.windowSize - 1;
    };
  slotPlacement = slot: {
    inherit slot;
    stateRootTemplate = "\${projectId}/\${environment}/\${slot}";
    registryDir = "registry";
    runDirTemplate = "runs/\${runId}";
    logsDirTemplate = "runs/\${runId}/logs";
    artifactsDirTemplate = "runs/\${runId}/artifacts";
    candidatePorts = slotWindow slot;
  };
  slotPlacements = builtins.listToAttrs (
    map (slot: {
      name = builtins.toString slot;
      value = slotPlacement slot;
    }) slots
  );
  defaultPortWindow = slotWindow slotPolicy.default;

  # Closures: realise package store paths and absolute executable paths.
  closurePackages = mapAttrsToList (_id: closure: closure.package) config.nixfied.closures;
  closureExecutable = id: "${config.nixfied.closures.${id}.package}/${config.nixfied.closures.${id}.executable}";
  closureSpec = id: closure: {
    closureId = id;
    kind = closure.kind;
    storePath = "${closure.package}";
    executable = closureExecutable id;
    targetSystem = target.closureSystem;
    operationBindings = closure.operationBindings;
    requiresExecutable = closure.requiresExecutable;
    effects = closure.effects;
  };
  closures = mapAttrsToList closureSpec config.nixfied.closures;

  # Execs: default the executable to the bound closure executable.
  execSpec = id: exec: {
    execId = id;
    closureId = exec.closureId;
    executable = if exec.executable != null then exec.executable else closureExecutable exec.closureId;
    args = exec.args;
    env = exec.env;
    codebaseId = exec.codebaseId;
    cwd = exec.cwd;
    stdin = exec.stdin;
    timeoutMs = exec.timeoutMs;
    outputCapture = exec.outputCapture;
    cancellationMode = exec.cancellationMode;
  };
  execs = mapAttrs execSpec config.nixfied.execs;

  endpointSpec = endpoint: {
    inherit (endpoint) endpointId host;
  };
  probeOf = op: { inherit (op.probe) timeoutMs retryIntervalMs maxAttempts; };
  terminalOf = op: { inherit (op.terminal) success failure; };
  lifecycleSpec = lc: {
    prepare = {
      inherit (lc.prepare) operationId execId execArgs;
      terminal = terminalOf lc.prepare;
    };
    start = {
      inherit (lc.start) operationId execId execArgs;
      terminal = terminalOf lc.start;
    };
    ready = {
      inherit (lc.ready) operationId;
      probe = probeOf lc.ready;
      terminal = terminalOf lc.ready;
    };
    health = {
      inherit (lc.health) operationId;
      probe = probeOf lc.health;
      terminal = terminalOf lc.health;
    };
    stop = {
      inherit (lc.stop) operationId signal timeoutMs;
      terminal = terminalOf lc.stop;
    };
    clean = {
      inherit (lc.clean) operationId;
      terminal = terminalOf lc.clean;
    };
  };

  serviceSpec =
    name: service:
    let
      endpoint = endpointSpec service.endpoint;
      lifecycle = lifecycleSpec service.lifecycle;
      lifecycleExecIds = lib.filter (id: id != null) [
        service.lifecycle.prepare.execId
        service.lifecycle.start.execId
      ];
      addressInputs = {
        projectId = config.nixfied.project.projectId;
        environment = "dev";
        slot = slotPolicy.default;
        service = name;
      };
      stateIdentityInputs = {
        inherit (config.nixfied.state) stateEpoch cleanupPolicy persistence;
      };
      runtimeIdentityInputs = {
        inherit lifecycle endpoint;
        foreground = service.foreground;
        healthPolicy = service.healthPolicy;
        containment = service.containment;
        lifetime = service.lifetime;
        execs = lib.filterAttrs (id: _: builtins.elem id lifecycleExecIds) execs;
      };
    in
    {
      serviceId = name;
      foreground = service.foreground;
      inherit lifecycle endpoint;
      healthPolicy = service.healthPolicy;
      stateRefs = service.stateRefs;
      logRefs = service.logRefs;
      containment = service.containment;
      lifetime = service.lifetime;
      identity = {
        serviceAddressHash = identifiers.hashJson addressInputs;
        endpointIdentityHash = identifiers.hashJson endpoint;
        stateIdentityHash = identifiers.hashJson stateIdentityInputs;
        runtimeCompatibilityHash = identifiers.hashJson runtimeIdentityInputs;
        targetIdentityHash = identifiers.hashJson target;
      };
    };
  services = mapAttrs serviceSpec config.nixfied.services;

  taskSpec = name: task: {
    taskId = name;
    operationId = task.operationId;
    execId = task.execId;
    args = task.args;
    dependsOnServicesReady = task.dependsOnServicesReady;
    exitPolicy = {
      successCodes = task.exitPolicy.successCodes;
    };
    outputCapture = task.outputCapture;
    artifactRefs = task.artifactRefs;
    logRefs = task.logRefs;
    summaryRefs = task.summaryRefs;
  };
  tasks = mapAttrs taskSpec config.nixfied.tasks;

  environments = mapAttrs (name: env: {
    environmentId = name;
    inherit (env) services tasks;
  }) config.nixfied.environments;

  workflowSpec = name: workflow: {
    workflowId = name;
    servicesRequired = workflow.servicesRequired;
    nodes = map (node: {
      nodeId = node.nodeId;
      taskId = node.taskId;
      dependsOn = node.dependsOn;
    }) workflow.nodes;
  };
  workflows = mapAttrs workflowSpec config.nixfied.workflows;

  serviceNames = attrNames config.nixfied.services;
  taskNames = attrNames config.nixfied.tasks;
  workflowNames = attrNames config.nixfied.workflows;
in
{
  packages = closurePackages;

  model = {
    modelVersion = constants.modelVersion;
    toolchainId = constants.toolchainId;
    runtimeAbi = constants.runtimeAbi;
    generator = {
      name = "nixfied";
      version = constants.toolchainId;
      emitter = "nix/compiler/emit-model.nix";
    };
    project = {
      inherit (config.nixfied.project) projectId name;
    };
    inherit target;
    codebases = [
      {
        codebaseId = "main";
        inherit (config.nixfied.codebases.main) logicalRoot sourceIdentity;
        sourceMode = "live-workspace";
        sourcePolicy = {
          inherit (config.nixfied.codebases.main) dirtyPolicy admissionFingerprintPolicy;
        };
      }
    ];
    inherit environments;
    inherit slotPolicy;
    capabilities = {
      environments = attrNames environments;
      inherit slots;
      services = serviceNames;
      tasks = taskNames;
      workflows = workflowNames;
      surfaces = surfaceNames;
    };
    runtimeConstraints = {
      allowedEnvironments = attrNames environments;
      slotMin = slotPolicy.min;
      slotDefault = slotPolicy.default;
      slotMax = slotPolicy.max;
      allowPortOverride = false;
      collisionPolicy = "fail";
    };
    surfaces = map surfaceSpec surfaceNames;
    placement = {
      stateRootTemplate = "\${projectId}/\${environment}/\${slot}";
      registryDir = "registry";
      runDirTemplate = "runs/\${runId}";
      logsDirTemplate = "runs/\${runId}/logs";
      artifactsDirTemplate = "runs/\${runId}/artifacts";
      candidatePorts = {
        inherit (defaultPortWindow) start end;
      };
      inherit slotPlacements;
    };
    state = {
      inherit (config.nixfied.state)
        markerIdentity
        stateEpoch
        cleanupPolicy
        persistence
        ;
    };
    secrets = [ ];
    inherit
      closures
      execs
      services
      tasks
      workflows
      ;
    docs = {
      title = config.nixfied.project.name;
      summary = "Compiled model for ${config.nixfied.project.projectId}.";
    };
  };
}
