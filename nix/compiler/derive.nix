{
  lib,
  pkgs,
  system,
  constants,
  config,
}:

let
  inherit (lib) mapAttrs mapAttrsToList;
  constructors = (import ../meta/manifest-structure.nix { inherit lib; }).constructors;
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
      requiresExecutable = closure.requiresExecutable;
      effects = closure.effects;
    };
  declaredClosures = mapAttrs declaredClosureSpec config.nixfied.closures;

  # A package-shaped tool synthesizes a closure: its executable anchors the
  # PATH root (the bin dir) and run[0] resolution; effects default to the
  # weakest attestation (per-tool effects granularity is deferred).
  # Labels are not identities: versions, overrides and selected outputs may
  # share a name. Compare full identities on insertion even after hashing.
  toolIdentity = package: {
    storePath = "${package}";
    executable = "${package}/bin/${toolMainProgram package}";
  };
  toolClosureId =
    package: "tool-${builtins.hashString "sha256" (builtins.toJSON (toolIdentity package))}";
  toolMainProgram = package: package.meta.mainProgram or (lib.getName package);
  # Effective operation ids: derived by default (docs/DERIVATION_SPEC.md §4.1),
  # declared only to override.
  leafOperationId =
    name: task: if task.operationId != null then task.operationId else deriveFacts.leafOperationId name;
  serviceOperationId =
    name: op: declared:
    if declared != null then declared else deriveFacts.serviceOperationId name op;

  invocationPositions = import ./invocation-positions.nix { inherit lib config; };

  toolEntryId = tool: if builtins.isString tool then tool else toolClosureId tool;
  packageTools = lib.concatMap (
    position: builtins.filter (tool: !(builtins.isString tool)) position.invocation.tools
  ) invocationPositions;
  synthesizedIdentities = lib.foldl' (
    identities: package:
    let
      id = toolClosureId package;
      identity = toolIdentity package;
    in
    assert lib.assertMsg (
      !(identities ? ${id}) || identities.${id} == identity
    ) "unequal synthesized tool identities collide at ${id}";
    identities // { ${id} = identity; }
  ) { } packageTools;
  synthesizedClosures = mapAttrs (
    id: identity:
    construct "ClosureSpec" {
      kind = "executable";
      inherit (identity) storePath executable;
      targetSystem = target.closureSystem;
      requiresExecutable = true;
      effects = [ "process" ];
    }
  ) synthesizedIdentities;
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

  probeOf = owner: op:
    if op.probe == null then null else resolveInvocation owner op.probe;
  policyOf = op: construct "ProbePolicy" {
    inherit (op.policy) timeoutMs retryIntervalMs maxAttempts;
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
        policy = policyOf lc.ready;
        probe = probeOf "service ${name} ready probe" lc.ready;
        terminal = terminalOf lc.ready;
      };
      health = construct "HealthSpec" {
        operationId = serviceOperationId name "health" lc.health.operationId;
        policy = policyOf lc.health;
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

  # Services carry no identity hashes: a session owns every service it starts,
  # and the runtime identifies a service instance by its run and declared name.
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
              readyProbe = resolveInvocation "service ${name} endpoint ready" service.endpoint.readyProbe;
              healthProbe = resolveInvocation "service ${name} endpoint health" service.endpoint.healthProbe;
            };
          }
        else
          builtins.mapAttrs (
            id: ep:
            construct "Endpoint" {
              endpointId = id;
              inherit (ep) host;
              readyProbe = resolveInvocation "service ${name} endpoint ${id} ready" ep.readyProbe;
              healthProbe = resolveInvocation "service ${name} endpoint ${id} health" ep.healthProbe;
            }
          ) service.endpoints;
      primaryEndpoint = if singular then service.endpoint.endpointId else service.primaryEndpoint;
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
      !multi || (service.primaryEndpoint != null && endpoints ? ${service.primaryEndpoint})
    ) "service ${name}: `endpoints` requires a declared `primaryEndpoint`";
    assert lib.assertMsg (
      !singular || service.primaryEndpoint == null
    ) "service ${name}: endpoint shorthand supplies its own primary endpoint";
    assert lib.assertMsg (
      lib.all (id: !(builtins.elem id service.connectsTo)) (builtins.attrNames endpoints)
    ) "service ${name}: endpoint ids must be distinct from connectsTo service ids";
    assert lib.assertMsg (lib.all (phase:
      (service.lifecycle.${phase}.probe != null) == endpointLess
    ) [ "ready" "health" ])
      "service ${name}: scalar probes are required exactly for endpoint-less services";
    assert lib.assertMsg (
      !endpointLess || service.primaryEndpoint == null
    ) "service ${name}: endpoint-less services cannot have a primary endpoint";
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
  # connectsTo (docs/DERIVATION_SPEC.md §3). Force the capacity check before
  # emission; runtime independently derives its graph at admission (DERIVE-1).
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
    builtins.seq (taskServicesRequired name) (
      if task.kind == "composite" then
        construct "TaskSpec" {
          kind = "composite";
          defaultOutput = task.defaultOutput;
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
          operationId = leafOperationId name task;
          invocation = resolveInvocation "task ${name}" task.invocation;
          requires = task.requires;
          exitPolicy = construct "ExitPolicy" {
            successCodes = task.exitPolicy.successCodes;
          };
          artifactRefs = task.artifactRefs;
          logRefs = task.logRefs;
          summaryRefs = task.summaryRefs;
        }
    );
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
