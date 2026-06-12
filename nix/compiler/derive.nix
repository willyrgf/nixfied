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

  # Declared closures: realise package store paths and absolute executable
  # paths. Tool entries given as plain packages synthesize additional closures
  # below.
  declaredClosureSpec = _id: closure: {
    kind = closure.kind;
    storePath = "${closure.package}";
    executable = "${closure.package}/${closure.executable}";
    targetSystem = target.closureSystem;
    operationBindings = closure.operationBindings;
    requiresExecutable = closure.requiresExecutable;
    effects = closure.effects;
  };
  declaredClosures = mapAttrs declaredClosureSpec config.nixfied.closures;

  # A package-shaped tool synthesizes a closure: its executable anchors the
  # PATH root (the bin dir) and run[0] resolution; effects default to the
  # weakest attestation (per-tool effects granularity is deferred).
  toolClosureId = package: "tool-${lib.getName package}";
  toolMainProgram = package: package.meta.mainProgram or (lib.getName package);
  synthesizedClosureOf = package: {
    kind = "executable";
    storePath = "${package}";
    executable = "${package}/bin/${toolMainProgram package}";
    targetSystem = target.closureSystem;
    operationBindings = derivedToolBindings.${toolClosureId package} or [ ];
    requiresExecutable = true;
    effects = [ "process" ];
  };

  # Every invocation position in the model, with the operation id it executes
  # under: task leaves plus each service's prepare/start and exec probes.
  invocationPositions =
    (mapAttrsToList (_id: task: {
      operationId = task.operationId;
      invocation = task.invocation;
    }) config.nixfied.tasks)
    ++ lib.concatLists (
      mapAttrsToList (
        _name: service:
        let
          lc = service.lifecycle;
        in
        lib.optional (lc.prepare.invocation != null) {
          operationId = lc.prepare.operationId;
          invocation = lc.prepare.invocation;
        }
        ++ [
          {
            operationId = lc.start.operationId;
            invocation = lc.start.invocation;
          }
        ]
        ++ lib.optional (lc.ready.probe.kind == "exec" && lc.ready.probe.invocation != null) {
          operationId = lc.ready.operationId;
          invocation = lc.ready.probe.invocation;
        }
        ++ lib.optional (lc.health.probe.kind == "exec" && lc.health.probe.invocation != null) {
          operationId = lc.health.operationId;
          invocation = lc.health.probe.invocation;
        }
      ) config.nixfied.services
    );

  toolEntryId = tool: if builtins.isString tool then tool else toolClosureId tool;
  packageTools = lib.concatMap (
    position: builtins.filter (tool: !(builtins.isString tool)) position.invocation.tools
  ) invocationPositions;
  synthesizedClosures = builtins.listToAttrs (
    map (package: {
      name = toolClosureId package;
      value = synthesizedClosureOf package;
    }) packageTools
  );
  collidingToolIds = builtins.filter (id: declaredClosures ? ${id}) (
    builtins.attrNames synthesizedClosures
  );
  closures =
    assert lib.assertMsg (collidingToolIds == [ ]) "synthesized tool closure ids collide with declared closures: ${builtins.concatStringsSep ", " collidingToolIds}";
    declaredClosures // synthesizedClosures;
  closurePackages = mapAttrsToList (_id: closure: closure.package) config.nixfied.closures ++ packageTools;

  # The declarative run[0] resolution rule (docs/DERIVATION_SPEC.md §1.1): the
  # first tool closure whose executable basename equals run[0] provides the
  # executable. The runtime re-derives the same rule at admission.
  resolveInvocation =
    owner: invocation:
    let
      toolIds = map toolEntryId invocation.tools;
      undeclared = builtins.filter (id: !(closures ? ${id})) toolIds;
      program = builtins.head invocation.run;
      resolvedId = lib.findFirst (id: baseNameOf closures.${id}.executable == program) null toolIds;
    in
    assert lib.assertMsg (undeclared == [ ])
      "${owner}: tools reference undeclared closures: ${builtins.concatStringsSep ", " undeclared}";
    assert lib.assertMsg (resolvedId != null)
      "${owner}: run[0] \"${program}\" is not the executable of any declared tool closure";
    assert lib.assertMsg (!(invocation.env ? PATH))
      "${owner}: env.PATH is runtime-owned (assembled from the tool roots) and must not be declared";
    {
      tools = toolIds;
      run = invocation.run;
      executable = closures.${resolvedId}.executable;
      env = invocation.env;
      codebaseId = invocation.codebaseId;
      cwd = invocation.cwd;
      stdin = invocation.stdin;
      timeoutMs = invocation.timeoutMs;
    };

  # Synthesized tool closures derive their bindings from the positions they
  # resolve run[0] for (declared closures keep hand-declared bindings).
  derivedToolBindings =
    let
      bindingsFor =
        toolId:
        lib.naturalSort (
          lib.unique (
            map (position: position.operationId) (
              builtins.filter (
                position:
                let
                  toolIds = map toolEntryId position.invocation.tools;
                  program = builtins.head position.invocation.run;
                  matches = builtins.filter (
                    id:
                    (
                      if builtins.isString id then
                        (closures ? ${id}) && baseNameOf closures.${id}.executable == program
                      else
                        false
                    )
                  ) toolIds;
                in
                matches != [ ] && builtins.head matches == toolId
              ) invocationPositions
            )
          )
        );
    in
    builtins.listToAttrs (
      map (package: rec {
        name = toolClosureId package;
        value = bindingsFor name;
      }) packageTools
    );

  # Mirror the runtime's serde skip rules (invocation only on exec probes and
  # bound prepares) so nix-emitted and runtime-rederived views stay
  # byte-comparable.
  probeOf =
    owner: op:
    {
      inherit (op.probe)
        kind
        timeoutMs
        retryIntervalMs
        maxAttempts
        ;
    }
    // lib.optionalAttrs (op.probe.kind == "exec" && op.probe.invocation != null) {
      invocation = resolveInvocation owner op.probe.invocation;
    };
  terminalOf = op: { inherit (op.terminal) success failure; };
  lifecycleSpec = name: lc: {
    prepare = {
      inherit (lc.prepare) operationId;
      terminal = terminalOf lc.prepare;
    }
    // lib.optionalAttrs (lc.prepare.invocation != null) {
      invocation = resolveInvocation "service ${name} prepare" lc.prepare.invocation;
    };
    start = {
      inherit (lc.start) operationId;
      invocation = resolveInvocation "service ${name} start" lc.start.invocation;
      terminal = terminalOf lc.start;
    };
    ready = {
      inherit (lc.ready) operationId;
      probe = probeOf "service ${name} ready probe" lc.ready;
      terminal = terminalOf lc.ready;
    };
    health = {
      inherit (lc.health) operationId;
      probe = probeOf "service ${name} health probe" lc.health;
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
  #
  # `endpoint` (single) is sugar for the common one-endpoint service; `endpoints`
  # (keyed by id) + `primaryEndpoint` is the multi-endpoint form. Exactly one must
  # be set; both compile to the same wire shape — an `endpoints` map plus a
  # `primaryEndpoint`.
  serviceSpec =
    name: service:
    let
      lifecycle = lifecycleSpec name service.lifecycle;
      singular = service.endpoint != null;
      multi = service.endpoints != { };
      endpoints =
        if singular then
          {
            ${service.endpoint.endpointId} = {
              inherit (service.endpoint) endpointId host;
            };
          }
        else
          builtins.mapAttrs (id: ep: { endpointId = id; inherit (ep) host; }) service.endpoints;
      primaryEndpoint = if singular then service.endpoint.endpointId else service.primaryEndpoint;
    in
    assert lib.assertMsg (
      singular != multi
    ) "service ${name}: set exactly one of `endpoint` or `endpoints`";
    assert lib.assertMsg (
      !multi || service.primaryEndpoint != null
    ) "service ${name}: `endpoints` requires `primaryEndpoint`";
    {
      inherit lifecycle endpoints primaryEndpoint;
      connectsTo = service.connectsTo;
      stateRefs = service.stateRefs;
      logRefs = service.logRefs;
      containment = service.containment;
    };
  services = mapAttrs serviceSpec config.nixfied.services;

  taskSpec = name: task: {
    operationId = task.operationId;
    invocation = resolveInvocation "task ${name}" task.invocation;
    requires = task.requires;
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
