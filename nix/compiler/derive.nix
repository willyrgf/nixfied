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

  # Execs: the executable is always the bound closure's executable — admission
  # rejects any other pairing, so the option does not exist.
  execSpec = _id: exec: {
    closureId = exec.closureId;
    executable = closureExecutable exec.closureId;
    args = exec.args;
    env = exec.env;
    codebaseId = exec.codebaseId;
    cwd = exec.cwd;
    stdin = exec.stdin;
    timeoutMs = exec.timeoutMs;
  };
  execs = mapAttrs execSpec config.nixfied.execs;

  endpointSpec = endpoint: {
    inherit (endpoint) endpointId host;
  };
  # Mirror the runtime's serde skip rules (exec fields only on exec probes) so
  # nix-emitted and runtime-rederived views stay byte-comparable.
  probeOf =
    op:
    {
      inherit (op.probe)
        kind
        timeoutMs
        retryIntervalMs
        maxAttempts
        ;
    }
    // lib.optionalAttrs (op.probe.kind == "exec") {
      inherit (op.probe) execId execArgs;
    };
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

  # Service identity is no longer emitted: the runtime derives a service's reuse
  # identity from its own lowered contract, so the model carries no identity
  # hashes for it to trust.
  serviceSpec =
    _name: service:
    let
      endpoint = endpointSpec service.endpoint;
      lifecycle = lifecycleSpec service.lifecycle;
    in
    {
      inherit lifecycle endpoint;
      connectsTo = service.connectsTo;
      stateRefs = service.stateRefs;
      logRefs = service.logRefs;
      containment = service.containment;
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
    # `nodes` is already an attrset keyed by node id (a duplicate id cannot
    # survive evaluation), so the model object is a direct projection.
    nodes = mapAttrs (_nodeId: node: {
      taskId = node.taskId;
      dependsOn = node.dependsOn;
    }) workflow.nodes;
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
