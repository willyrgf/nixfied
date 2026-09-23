{
  lib,
  pkgs,
  system,
  constants,
  config,
}:

let
  inherit (lib) mapAttrs mapAttrsToList;
  constructors = (import ../meta/default.nix { inherit lib; }).constructors;
  construct = name: constructors.${"primitive/" + name};
  targetLib = import ../lib/target.nix { inherit lib; };
  deriveFacts = import ../lib/derive-facts.nix { inherit lib; };

  target = construct "Target" (targetLib.fromSystem config.nixfied.target.system);

  slotPolicy = construct "SlotPolicy" config.nixfied.slotPolicy;
  slots = lib.range slotPolicy.min slotPolicy.max;
  portPolicy = config.nixfied.placement.ports;
  slotWindow =
    slot:
    let
      start = portPolicy.base + (slot * portPolicy.slotStride);
    in
    construct "CandidatePortWindow" {
      inherit start;
      end = start + portPolicy.windowSize - 1;
    };
  slotPlacement =
    slot:
    construct "SlotPlacement" {
      inherit slot;
      candidatePorts = slotWindow slot;
    };
  slotPlacements = builtins.listToAttrs (
    map (slot: {
      name = builtins.toString slot;
      value = slotPlacement slot;
    }) slots
  );
  secretDescriptors = mapAttrs (
    secretId: secret:
    construct "SecretDescriptor" {
      inherit secretId;
      source = construct "SecretSource" secret.source;
    }
  ) config.nixfied.secrets;

  # Declared closures: realise package store paths and absolute executable
  # paths. Tool entries given as plain packages synthesize additional closures
  # below.
  # Bindings are DERIVED (docs/DERIVATION_SPEC.md §4); a declared list is an
  # optional narrowing gate checked below.
  declaredClosureSpec =
    id: closure:
    construct "ClosureSpec" {
      kind = closure.kind;
      storePath = "${closure.package}";
      executable = "${closure.package}/${closure.executable}";
      targetSystem = target.closureSystem;
      operationBindings = gatedBindings id closure.operationBindings;
      requiresExecutable = closure.requiresExecutable;
      effects = closure.effects;
    };
  declaredClosures = mapAttrs declaredClosureSpec config.nixfied.closures;

  # A package-shaped tool synthesizes a closure: its executable anchors the
  # PATH root (the bin dir) and run[0] resolution; effects default to the
  # weakest attestation (per-tool effects granularity is deferred).
  toolClosureId = package: "tool-${lib.getName package}";
  toolMainProgram = package: package.meta.mainProgram or (lib.getName package);
  synthesizedClosureOf =
    package:
    construct "ClosureSpec" {
      kind = "executable";
      storePath = "${package}";
      executable = "${package}/bin/${toolMainProgram package}";
      targetSystem = target.closureSystem;
      operationBindings = derivedBindings (toolClosureId package);
      requiresExecutable = true;
      effects = [ "process" ];
    };

  # Effective operation ids: derived by default (docs/DERIVATION_SPEC.md §5.1),
  # declared only to override.
  leafOperationId =
    name: task: if task.operationId != null then task.operationId else deriveFacts.leafOperationId name;
  serviceOperationId =
    name: op: declared:
    if declared != null then declared else deriveFacts.serviceOperationId name op;

  # Every invocation position in the manifest, with the (effective) operation id
  # it executes under: task leaves plus each service's prepare/start and exec
  # probes.
  invocationPositions =
    (mapAttrsToList (name: task: {
      operationId = leafOperationId name task;
      invocation = task.invocation;
    }) (lib.filterAttrs (_id: task: task.kind == "leaf") config.nixfied.tasks))
    ++ lib.concatLists (
      mapAttrsToList (
        name: service:
        let
          lc = service.lifecycle;
        in
        [
          {
            operationId = serviceOperationId name "start" lc.start.operationId;
            invocation = lc.start.invocation;
          }
        ]
        ++ lib.optional (lc.ready.probe.kind == "exec" && lc.ready.probe.invocation != null) {
          operationId = serviceOperationId name "ready" lc.ready.operationId;
          invocation = lc.ready.probe.invocation;
        }
        ++ lib.optional (lc.health.probe.kind == "exec" && lc.health.probe.invocation != null) {
          operationId = serviceOperationId name "health" lc.health.operationId;
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
    assert lib.assertMsg (collidingToolIds == [ ])
      "synthesized tool closure ids collide with declared closures: ${builtins.concatStringsSep ", " collidingToolIds}";
    declaredClosures // synthesizedClosures;
  closurePackages =
    mapAttrsToList (_id: closure: closure.package) config.nixfied.closures ++ packageTools;

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
    assert lib.assertMsg (
      undeclared == [ ]
    ) "${owner}: tools reference undeclared closures: ${builtins.concatStringsSep ", " undeclared}";
    assert lib.assertMsg (
      resolvedId != null
    ) "${owner}: run[0] \"${program}\" is not the executable of any declared tool closure";
    assert lib.assertMsg (
      !(invocation.env ? PATH)
    ) "${owner}: env.PATH is runtime-owned (assembled from the tool roots) and must not be declared";
    construct "Invocation" {
      tools = toolIds;
      run = invocation.run;
      executable = closures.${resolvedId}.executable;
      env = invocation.env;
      codebaseId = invocation.codebaseId;
      cwd = invocation.cwd;
      stdin = invocation.stdin;
      timeoutMs = invocation.timeoutMs;
    };

  # Derived operation bindings (docs/DERIVATION_SPEC.md §4): the byte-sorted
  # operation ids of every position whose run[0] resolves to the closure. A
  # declared list narrows: the derived set must be a subset of it, and every
  # declared binding must name a declared operation.
  executableBasenames = mapAttrs (_id: closure: baseNameOf closure.executable) closures;
  bindingPositions = map (position: {
    operationId = position.operationId;
    toolIds = map toolEntryId position.invocation.tools;
    program = builtins.head position.invocation.run;
  }) invocationPositions;
  derivedBindings = deriveFacts.operationBindings {
    positions = bindingPositions;
    inherit executableBasenames;
  };
  declaredOperationIds = map (position: position.operationId) invocationPositions;
  gatedBindings =
    id: declared:
    let
      derived = derivedBindings id;
      outsideGate = builtins.filter (binding: !(builtins.elem binding declared)) derived;
      unknownDeclared = builtins.filter (binding: !(builtins.elem binding declaredOperationIds)) (
        if declared == null then [ ] else declared
      );
    in
    if declared == null then
      derived
    else
      assert lib.assertMsg (unknownDeclared == [ ])
        "closure ${id}: declared operationBindings name undeclared operations: ${builtins.concatStringsSep ", " unknownDeclared}";
      assert lib.assertMsg (outsideGate == [ ])
        "closure ${id}: dispatched against operations outside its declared bindings: ${builtins.concatStringsSep ", " outsideGate}";
      derived;

  # Native lowering selects invocations; shared construction owns omission.
  probeOf =
    owner: op:
    construct "ProbeSpec" {
      inherit (op.probe)
        kind
        timeoutMs
        retryIntervalMs
        maxAttempts
        ;
      invocation =
        if op.probe.kind == "exec" && op.probe.invocation != null then
          resolveInvocation owner op.probe.invocation
        else
          null;
    };
  terminalOf = op: construct "TerminalSemantics" { inherit (op.terminal) success failure; };
  lifecycleSpec =
    name: lc:
    construct "Lifecycle" {
      start = construct "StartSpec" {
        operationId = serviceOperationId name "start" lc.start.operationId;
        invocation = resolveInvocation "service ${name} start" lc.start.invocation;
        terminal = terminalOf lc.start;
      };
      ready = construct "ReadySpec" {
        operationId = serviceOperationId name "ready" lc.ready.operationId;
        probe = probeOf "service ${name} ready probe" lc.ready;
        terminal = terminalOf lc.ready;
      };
      health = construct "HealthSpec" {
        operationId = serviceOperationId name "health" lc.health.operationId;
        probe = probeOf "service ${name} health probe" lc.health;
        terminal = terminalOf lc.health;
      };
      stop = construct "StopSpec" {
        operationId = serviceOperationId name "stop" lc.stop.operationId;
        inherit (lc.stop) signal timeoutMs;
        terminal = terminalOf lc.stop;
      };
      clean = construct "CleanSpec" {
        operationId = serviceOperationId name "clean" lc.clean.operationId;
        terminal = terminalOf lc.clean;
      };
      prepare =
        if lc.prepare.task == null then
          null
        else
          construct "PrepareSpec" {
            task = lc.prepare.task;
          };
    };

  # Service identity is no longer emitted: the runtime derives a service's reuse
  # identity from its own lowered contract, so the manifest carries no identity
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
      endpointLess = !singular && !multi;
      endpoints =
        if singular then
          {
            ${service.endpoint.endpointId} = construct "Endpoint" {
              inherit (service.endpoint) endpointId host;
            };
          }
        else
          builtins.mapAttrs (
            id: ep:
            construct "Endpoint" {
              endpointId = id;
              inherit (ep) host;
            }
          ) service.endpoints;
      primaryEndpoint = if singular then service.endpoint.endpointId else service.primaryEndpoint;
      probeKind = op: service.lifecycle.${op}.probe.kind;
      startClosureEffects =
        let
          inv = service.lifecycle.start.invocation;
          toolIds = map toolEntryId inv.tools;
          program = builtins.head inv.run;
          resolvedId = lib.findFirst (
            id: (closures ? ${id}) && baseNameOf closures.${id}.executable == program
          ) null toolIds;
        in
        if resolvedId == null then [ ] else closures.${resolvedId}.effects;
      startListens = builtins.elem "network-listener" startClosureEffects;
    in
    assert lib.assertMsg (
      !(singular && multi)
    ) "service ${name}: set at most one of `endpoint` or `endpoints`";
    assert lib.assertMsg (
      !multi || service.primaryEndpoint != null
    ) "service ${name}: `endpoints` requires `primaryEndpoint`";
    # An endpoint-less service has no tcp probe target: readiness means "the
    # probe answers", so both probes must be invocation probes.
    assert lib.assertMsg
      (!endpointLess || (probeKind "ready" == "exec" && probeKind "health" == "exec"))
      "service ${name}: an endpoint-less service's ready/health probes must be invocation probes (tcp has no target)";
    # Effects coherence, both directions: declared endpoints require a
    # `network-listener` attestation on the start closure; an endpoint-less
    # start closure must not announce a listener the planner cannot reserve.
    assert lib.assertMsg (
      endpointLess || startListens
    ) "service ${name}: declared endpoints require `network-listener` on the start closure's effects";
    assert lib.assertMsg (
      !endpointLess || !startListens
    ) "service ${name}: an endpoint-less service's start closure must not declare `network-listener`";
    construct "ServiceSpec" {
      inherit lifecycle endpoints;
      primaryEndpoint = if endpointLess then null else primaryEndpoint;
      connectsTo = service.connectsTo;
      stateRefs = service.stateRefs;
      logRefs = service.logRefs;
      containment = service.containment;
    };
  services = mapAttrs serviceSpec config.nixfied.services;

  # Derived per task: the union of transitive leaf requires, closed over
  # connectsTo (docs/DERIVATION_SPEC.md §3). The runtime re-derives and
  # compares at admission (DERIVE-1).
  taskServicesRequired =
    let
      derived = deriveFacts.servicesRequired {
        tasks = config.nixfied.tasks;
        services = config.nixfied.services;
        prepareTaskOf = name: config.nixfied.services.${name}.lifecycle.prepare.task;
      };
      endpointDemand =
        required:
        lib.foldl' (
          total: serviceName:
          total + builtins.length (builtins.attrNames (services.${serviceName}.endpoints or { }))
        ) 0 required;
    in
    name:
    let
      required = derived name;
    in
    assert lib.assertMsg (
      endpointDemand required <= portPolicy.windowSize
    ) "task ${name}: derived service endpoint demand must fit placement.ports.windowSize";
    required;
  taskSpec =
    name: task:
    if task.kind == "composite" then
      construct "TaskSpec" {
        kind = "composite";
        defaultOutput = task.defaultOutput;
        serviceLifetime = task.serviceLifetime;
        servicesRequired = taskServicesRequired name;
        steps = mapAttrs (
          _stepName: step:
          construct "StepSpec" {
            task = step.task;
            dependsOn = step.dependsOn;
          }
        ) task.steps;
      }
    else
      construct "TaskSpec" {
        kind = "leaf";
        defaultOutput = task.defaultOutput;
        serviceLifetime = task.serviceLifetime;
        operationId = leafOperationId name task;
        invocation = resolveInvocation "task ${name}" task.invocation;
        requires = task.requires;
        servicesRequired = taskServicesRequired name;
        exitPolicy = construct "ExitPolicy" {
          successCodes = task.exitPolicy.successCodes;
        };
        artifactRefs = task.artifactRefs;
        logRefs = task.logRefs;
        summaryRefs = task.summaryRefs;
      };
  tasks = mapAttrs taskSpec config.nixfied.tasks;

in
{
  packages = closurePackages;

  manifest = construct "Manifest" {
    manifestVersion = constants.manifestVersion;
    toolchainId = constants.toolchainId;
    runtimeAbi = constants.runtimeAbi;
    generator = construct "Generator" {
      name = "nixfied";
      version = constants.toolchainId;
      emitter = "nix/compiler/emit-manifest.nix";
    };
    project = construct "Project" {
      inherit (config.nixfied.project) projectId name;
    };
    inherit target;
    codebases = [
      (construct "Codebase" {
        codebaseId = "main";
        inherit (config.nixfied.codebases.main) logicalRoot sourceMode sourceIdentity;
        sourcePolicy = construct "SourcePolicy" {
          inherit (config.nixfied.codebases.main) dirtyPolicy admissionFingerprintPolicy;
        };
      })
    ];
    secrets = secretDescriptors;
    # Membership does not exist; `dev` is the single isolation namespace
    # (state roots, slots, registry keys).
    environments = [ "dev" ];
    inherit slotPolicy;
    placement = construct "Placement" {
      inherit slotPlacements;
    };
    state = construct "StatePolicy" {
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
      ;
  };
}
