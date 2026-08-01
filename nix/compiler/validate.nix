{ lib, system }:

config:
let
  fail = message: throw "nixfied model validation failed: ${message}";
  expect = condition: message: if condition then null else fail message;
  slotPolicy = config.nixfied.slotPolicy;
  portPolicy = config.nixfied.placement.ports;
  source = config.nixfied.codebases.main;
  sourceIsLive = source.sourceMode == "live-workspace";
  sourceIsImmutable = source.sourceMode == "snapshot" || source.sourceMode == "flake-input";
  reservedProjectApps = [
    "help"
    "run"
    "ps"
    "down"
    "clean"
    "model-check"
  ];
  sourceIdentity = toString source.sourceIdentity;
  immutableSourceIsStoreRoot =
    !sourceIsImmutable || lib.hasPrefix "${builtins.storeDir}/" sourceIdentity;
  liveRejectAllowed = !(sourceIsLive && source.dirtyPolicy == "reject");
  slots = lib.range slotPolicy.min slotPolicy.max;
  slotWindow = slot: {
    start = portPolicy.base + (slot * portPolicy.slotStride);
    end = portPolicy.base + (slot * portPolicy.slotStride) + portPolicy.windowSize - 1;
  };
  windows = map slotWindow slots;
  windowsInRange = lib.all (window: window.start >= 1 && window.end <= 65535) windows;
  windowsDoNotOverlap = portPolicy.slotStride >= portPolicy.windowSize;
  services = config.nixfied.services;
  connectsToDeclared = lib.all (
    name: lib.all (target: builtins.hasAttr target services) services.${name}.connectsTo
  ) (builtins.attrNames services);
  # Cycle check: a service may not reach itself through connectsTo. Walking
  # with a `seen` set terminates even on cyclic graphs; reaching the start
  # service again is the cycle proof.
  reaches =
    start: current: seen:
    lib.any (
      target:
      target == start || (!(builtins.elem target seen) && reaches start target (seen ++ [ target ]))
    ) (services.${current}.connectsTo or [ ]);
  connectsToAcyclic = lib.all (name: !(reaches name name [ ])) (builtins.attrNames services);
  # Mirror the runtime's `LoopbackHost` wire type so a model that the runtime
  # cannot even parse fails at evaluation instead of admission. The runtime
  # accepts any loopback IP literal; this check admits the canonical forms
  # (127.x.x.x and ::1), which is strictly narrower — fail-closed.
  isLoopbackHost =
    host:
    host == "::1"
    || (
      let
        octets = builtins.match "127\\.([0-9]+)\\.([0-9]+)\\.([0-9]+)" host;
      in
      octets != null && lib.all (octet: lib.toInt octet <= 255) octets
    );
  # A service declares either the single-endpoint `endpoint` sugar or the
  # multi-endpoint `endpoints` map; collect the bind hosts from whichever form.
  serviceHosts =
    service:
    if service.endpoint != null then
      [ service.endpoint.host ]
    else
      map (ep: ep.host) (builtins.attrValues service.endpoints); # [] when endpoint-less
  endpointHostsLoopback = lib.all (name: lib.all isLoopbackHost (serviceHosts services.${name})) (
    builtins.attrNames services
  );
  # Task kind/field coherence, composite graph validity, and step-path id
  # discipline: invalid composition must fail at evaluation, not compile into
  # a model the runtime only rejects at admission.
  tasks = config.nixfied.tasks;
  surfaceVerbs = config.nixfied.surface.verbs;
  surfaceVerbIds = builtins.attrNames surfaceVerbs;
  # `attrsOf` values are lazy. Force every description here so malformed or
  # throwing descriptions fail model evaluation even when no app projection is
  # requested later.
  surfaceDescriptionsForced = builtins.deepSeq (
    map (verb: surfaceVerbs.${verb}) surfaceVerbIds
  ) true;
  deriveFacts = import ../lib/derive-facts.nix { inherit lib; };
  taskNames = builtins.attrNames tasks;
  stepSafe = id: builtins.match "[A-Za-z0-9][A-Za-z0-9_-]*" id != null;
  endpointLess = service: service.endpoint == null && service.endpoints == { };
  endpointIds =
    service:
    if service.endpoint != null then
      [ service.endpoint.endpointId ]
    else
      builtins.attrNames service.endpoints;
  invocationValues = invocation: invocation.run ++ builtins.attrValues invocation.env;
  refsAfterPrefix =
    prefix: value:
    lib.concatMap (
      part:
      let
        split = lib.splitString "}" part;
      in
      if builtins.length split > 1 then [ (builtins.head split) ] else [ ]
    ) (builtins.tail (lib.splitString prefix value));
  secretRefs = refsAfterPrefix "\${secret:";
  hasSecretRefSyntax = value: lib.hasInfix "\${secret:" value;
  secretRefMalformed =
    value:
    lib.any (part: builtins.length (lib.splitString "}" part) == 1) (
      builtins.tail (lib.splitString "\${secret:" value)
    );
  namedEndpointRefs = value: refsAfterPrefix "\${port:" value ++ refsAfterPrefix "\${host:" value;
  invocationNamedRefs =
    invocation: lib.unique (lib.concatMap namedEndpointRefs (invocationValues invocation));
  hasBareEndpointRef = value: lib.hasInfix "\${port}" value || lib.hasInfix "\${host}" value;
  invocationHasBareRef = invocation: lib.any hasBareEndpointRef (invocationValues invocation);
  taskIdsStepSafe = lib.all stepSafe taskNames && lib.all stepSafe (builtins.attrNames services);
  secrets = config.nixfied.secrets;
  secretNames = builtins.attrNames secrets;
  secretIdsStepSafe = lib.all stepSafe secretNames;
  secretFilePathConfined =
    path: path != null && !(lib.hasPrefix "/" path) && !(builtins.elem ".." (lib.splitString "/" path));
  secretResolversCoherent = lib.all (
    name:
    let
      source = secrets.${name}.source;
    in
    if source.kind == "env-var" then
      source.envVar != null && source.path == null
    else
      source.path != null && source.envVar == null && secretFilePathConfined source.path
  ) secretNames;
  leafTasks = lib.filterAttrs (_n: task: task.kind == "leaf") tasks;
  compositeTasks = lib.filterAttrs (_n: task: task.kind == "composite") tasks;
  leavesCoherent = lib.all (
    name:
    let
      task = leafTasks.${name};
    in
    task.invocation != null && task.steps == { }
  ) (builtins.attrNames leafTasks);
  compositesCoherent = lib.all (
    name:
    let
      task = compositeTasks.${name};
    in
    task.steps != { }
    && task.operationId == null
    && task.invocation == null
    && task.requires == [ ]
    && lib.all stepSafe (builtins.attrNames task.steps)
  ) (builtins.attrNames compositeTasks);
  stepTasksDeclared = lib.all (
    name:
    lib.all (step: builtins.hasAttr step.task tasks) (builtins.attrValues compositeTasks.${name}.steps)
  ) (builtins.attrNames compositeTasks);
  stepDependsOnSiblings = lib.all (
    name:
    let
      stepNames = builtins.attrNames compositeTasks.${name}.steps;
    in
    lib.all (step: lib.all (dep: builtins.elem dep stepNames) step.dependsOn) (
      builtins.attrValues compositeTasks.${name}.steps
    )
  ) (builtins.attrNames compositeTasks);
  # Sibling dependsOn acyclicity per composite, and task-reference acyclicity
  # through nesting (a composite may not reach itself through step task refs).
  stepReaches =
    steps: start: current: seen:
    lib.any (
      dep: dep == start || (!(builtins.elem dep seen) && stepReaches steps start dep (seen ++ [ dep ]))
    ) steps.${current}.dependsOn;
  stepGraphsAcyclic = lib.all (
    name:
    let
      steps = compositeTasks.${name}.steps;
    in
    lib.all (step: !(stepReaches steps step step [ ])) (builtins.attrNames steps)
  ) (builtins.attrNames compositeTasks);
  taskRefsOf =
    name:
    if tasks.${name}.kind == "composite" then
      map (step: step.task) (builtins.attrValues tasks.${name}.steps)
    else
      [ ];
  taskReaches =
    start: current: seen:
    lib.any (
      target:
      builtins.hasAttr target tasks
      && (
        target == start || (!(builtins.elem target seen) && taskReaches start target (seen ++ [ target ]))
      )
    ) (taskRefsOf current);
  taskGraphAcyclic = lib.all (name: !(taskReaches name name [ ])) taskNames;
  leafRequiresDeclared = lib.all (
    name: lib.all (target: builtins.hasAttr target services) leafTasks.${name}.requires
  ) (builtins.attrNames leafTasks);
  leafNamedRefsInScope = lib.all (
    name:
    let
      task = leafTasks.${name};
      allowed = builtins.filter (
        target: builtins.hasAttr target services && !(endpointLess services.${target})
      ) task.requires;
    in
    lib.all (reference: builtins.elem reference allowed) (invocationNamedRefs task.invocation)
  ) (builtins.attrNames leafTasks);
  leafBareRefsResolvable = lib.all (
    name:
    let
      task = leafTasks.${name};
      primary = if task.requires == [ ] then null else builtins.head task.requires;
      primaryHasEndpoint =
        primary != null && builtins.hasAttr primary services && !(endpointLess services.${primary});
    in
    !(invocationHasBareRef task.invocation) || primaryHasEndpoint
  ) (builtins.attrNames leafTasks);
  serviceLifecycleInvocations =
    service:
    let
      lc = service.lifecycle;
    in
    [ lc.start.invocation ]
    ++ lib.optional (
      lc.ready.probe.kind == "exec" && lc.ready.probe.invocation != null
    ) lc.ready.probe.invocation
    ++ lib.optional (
      lc.health.probe.kind == "exec" && lc.health.probe.invocation != null
    ) lc.health.probe.invocation;
  allInvocations =
    (map (task: task.invocation) (builtins.attrValues leafTasks))
    ++ lib.concatMap serviceLifecycleInvocations (builtins.attrValues services);
  secretRefsOnlyInEnv = lib.all (
    invocation: lib.all (value: !(hasSecretRefSyntax value)) invocation.run
  ) allInvocations;
  secretRefsWellFormed = lib.all (
    invocation: lib.all (value: !(secretRefMalformed value)) (builtins.attrValues invocation.env)
  ) allInvocations;
  secretRefsDeclared = lib.all (
    invocation:
    lib.all (reference: reference != "" && builtins.hasAttr reference secrets) (
      lib.concatMap secretRefs (builtins.attrValues invocation.env)
    )
  ) allInvocations;
  serviceNamedRefsInScope = lib.all (
    name:
    let
      service = services.${name};
      allowed =
        endpointIds service
        ++ builtins.filter (
          target: builtins.hasAttr target services && !(endpointLess services.${target})
        ) service.connectsTo;
      refs = lib.unique (lib.concatMap invocationNamedRefs (serviceLifecycleInvocations service));
    in
    lib.all (reference: builtins.elem reference allowed) refs
  ) (builtins.attrNames services);
  endpointLessServicesHaveNoBareRefs = lib.all (
    name:
    let
      service = services.${name};
    in
    !(endpointLess service)
    || lib.all (invocation: !(invocationHasBareRef invocation)) (serviceLifecycleInvocations service)
  ) (builtins.attrNames services);
  leafOperationId =
    name: task: if task.operationId != null then task.operationId else deriveFacts.leafOperationId name;
  serviceOperationId =
    name: op: declared:
    if declared != null then declared else deriveFacts.serviceOperationId name op;
  effectiveOperationIds =
    map (name: leafOperationId name leafTasks.${name}) (builtins.attrNames leafTasks)
    ++ lib.concatMap (
      name:
      let
        lc = services.${name}.lifecycle;
      in
      [
        (serviceOperationId name "start" lc.start.operationId)
        (serviceOperationId name "ready" lc.ready.operationId)
        (serviceOperationId name "health" lc.health.operationId)
        (serviceOperationId name "stop" lc.stop.operationId)
        (serviceOperationId name "clean" lc.clean.operationId)
      ]
    ) (builtins.attrNames services);
  duplicateOperationIds = lib.unique (
    builtins.filter (
      id: builtins.length (builtins.filter (other: other == id) effectiveOperationIds) > 1
    ) effectiveOperationIds
  );
  # prepare-as-task: the reference must resolve, and the combined connectsTo +
  # prepare-requires service graph must stay acyclic (a service cannot —
  # directly or transitively — wait on itself to prepare).
  prepareTaskOf = name: services.${name}.lifecycle.prepare.task;
  prepareTasksDeclared = lib.all (
    name: prepareTaskOf name == null || builtins.hasAttr (prepareTaskOf name) tasks
  ) (builtins.attrNames services);
  prepareRequiresOf =
    name:
    if prepareTaskOf name == null || !(builtins.hasAttr (prepareTaskOf name) tasks) then
      [ ]
    else
      lib.unique (
        lib.concatMap (leaf: tasks.${leaf}.requires) (deriveFacts.leavesOf tasks (prepareTaskOf name))
      );
  combinedEdges = name: services.${name}.connectsTo ++ prepareRequiresOf name;
  combinedReaches =
    start: current: seen:
    lib.any (
      target:
      builtins.hasAttr target services
      && (
        target == start
        || (!(builtins.elem target seen) && combinedReaches start target (seen ++ [ target ]))
      )
    ) (combinedEdges current);
  combinedGraphAcyclic = lib.all (name: !(combinedReaches name name [ ])) (
    builtins.attrNames services
  );
  checks = [
    (expect (config.nixfied.target.system == system) "target.system must match the compile system")
    (expect (slotPolicy.min >= 0) "slotPolicy.min must be non-negative")
    (expect (slotPolicy.max >= slotPolicy.min) "slotPolicy.max must be >= min")
    (expect (
      slotPolicy.default >= slotPolicy.min && slotPolicy.default <= slotPolicy.max
    ) "slotPolicy.default must be within the slot range")
    (expect windowsInRange "per-slot candidate port windows must be in 1..65535")
    (expect windowsDoNotOverlap "per-slot candidate port windows must not overlap")
    (expect immutableSourceIsStoreRoot "immutable codebase sourceIdentity must be a Nix store path")
    (expect liveRejectAllowed "dirtyPolicy=reject is only valid for immutable source modes")
    # A model with no services is valid as long as it declares something to
    # run: the runtime supports service-less task selections.
    (expect (
      config.nixfied.services != { } || config.nixfied.tasks != { }
    ) "at least one service or task must be declared")
    (expect endpointHostsLoopback "service endpoint.host must be a loopback IP literal (127.x.x.x or ::1)")
    (expect connectsToDeclared "service connectsTo targets must be declared services")
    (expect connectsToAcyclic "service connectsTo graph must be acyclic")
    (expect taskIdsStepSafe "task and service ids must match [A-Za-z0-9][A-Za-z0-9_-]* (step-path segments)")
    (expect secretIdsStepSafe "secret ids must match [A-Za-z0-9][A-Za-z0-9_-]*")
    (expect secretResolversCoherent "secret resolvers must be env-var with envVar only, or file with a confined relative path only")
    (expect secretRefsOnlyInEnv "secret placeholders are only valid in invocation.env values")
    (expect secretRefsWellFormed "secret placeholders must use the \${secret:<id>} grammar")
    (expect secretRefsDeclared "secret placeholders must reference declared nixfied.secrets ids")
    (expect leavesCoherent "a leaf task must declare an invocation and no steps")
    (expect compositesCoherent "a composite task carries only steps (no invocation, operationId, or requires) with step-safe names")
    (expect stepTasksDeclared "composite steps must reference declared tasks")
    (expect stepDependsOnSiblings "composite step dependsOn must name a sibling step")
    (expect stepGraphsAcyclic "composite step dependency graph must be acyclic")
    (expect taskGraphAcyclic "the task reference graph must be acyclic")
    (expect leafRequiresDeclared "leaf task requires targets must be declared services")
    (expect leafNamedRefsInScope "leaf task named endpoint placeholders must reference addressable required services")
    (expect leafBareRefsResolvable "leaf task bare endpoint placeholders require a primary service with endpoints")
    (expect serviceNamedRefsInScope "service named endpoint placeholders must reference own endpoints or addressable connectsTo services")
    (expect endpointLessServicesHaveNoBareRefs "endpoint-less service lifecycle invocations must not use bare endpoint placeholders")
    (expect (duplicateOperationIds == [ ])
      "effective operation ids must be globally unique: ${builtins.concatStringsSep ", " duplicateOperationIds}"
    )
    (expect prepareTasksDeclared "service prepare must reference a declared task")
    (expect surfaceDescriptionsForced "surface.verbs descriptions must evaluate")
    (expect (lib.all (
      verb: builtins.hasAttr verb tasks
    ) surfaceVerbIds) "surface.verbs must name declared tasks")
    (expect
      (lib.all (
        verb: !(builtins.elem verb reservedProjectApps)
      ) surfaceVerbIds)
      "surface.verbs must not collide with the reserved project-app namespace (${builtins.concatStringsSep ", " reservedProjectApps})"
    )
    (expect combinedGraphAcyclic "the combined connectsTo + prepare-requires service graph must be acyclic")
  ];
in
lib.foldl' (acc: check: lib.seq check acc) config checks
