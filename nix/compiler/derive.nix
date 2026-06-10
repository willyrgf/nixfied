{
  lib,
  pkgs,
  system,
  constants,
  config,
}:

let
  inherit (lib) mapAttrs mapAttrsToList;
  targetLib = import ../lib/target.nix { inherit lib; };
  identifiers = import ../lib/identifiers.nix;

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
    candidatePorts = slotWindow slot;
  };
  slotPlacements = builtins.listToAttrs (
    map (slot: {
      name = builtins.toString slot;
      value = slotPlacement slot;
    }) slots
  );

  # Closures: realise package store paths and absolute executable paths.
  closurePackages = mapAttrsToList (_id: closure: closure.package) config.nixfied.closures;
  closureExecutable =
    id: "${config.nixfied.closures.${id}.package}/${config.nixfied.closures.${id}.executable}";
  closureSpec = id: closure: {
    kind = closure.kind;
    storePath = "${closure.package}";
    executable = closureExecutable id;
    targetSystem = target.closureSystem;
    operationBindings = closure.operationBindings;
    requiresExecutable = closure.requiresExecutable;
    effects = closure.effects;
  };
  closures = mapAttrs closureSpec config.nixfied.closures;

  # Execs: default the executable to the bound closure executable.
  execSpec = _id: exec: {
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
        containment = service.containment;
        execs = lib.filterAttrs (id: _: builtins.elem id lifecycleExecIds) execs;
      };
    in
    {
      inherit lifecycle endpoint;
      stateRefs = service.stateRefs;
      logRefs = service.logRefs;
      containment = service.containment;
      identity = {
        serviceAddressHash = identifiers.hashJson addressInputs;
        endpointIdentityHash = identifiers.hashJson endpoint;
        stateIdentityHash = identifiers.hashJson stateIdentityInputs;
        runtimeCompatibilityHash = identifiers.hashJson runtimeIdentityInputs;
        targetIdentityHash = identifiers.hashJson target;
      };
    };
  services = mapAttrs serviceSpec config.nixfied.services;

  taskSpec = _name: task: {
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

  environments = mapAttrs (_name: env: {
    inherit (env) services tasks;
  }) config.nixfied.environments;

  workflowSpec = _name: workflow: {
    servicesRequired = workflow.servicesRequired;
    nodes = builtins.listToAttrs (
      map (node: {
        name = node.nodeId;
        value = {
          taskId = node.taskId;
          dependsOn = node.dependsOn;
        };
      }) workflow.nodes
    );
  };
  workflows = mapAttrs workflowSpec config.nixfied.workflows;
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
    placement = {
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
