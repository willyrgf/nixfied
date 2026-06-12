{ lib, system }:

config:
let
  fail = message: throw "nixfied model validation failed: ${message}";
  expect = condition: message: if condition then null else fail message;
  slotPolicy = config.nixfied.slotPolicy;
  portPolicy = config.nixfied.placement.ports;
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
      map (ep: ep.host) (builtins.attrValues service.endpoints);
  endpointHostsLoopback = lib.all (
    name: lib.all isLoopbackHost (serviceHosts services.${name})
  ) (builtins.attrNames services);
  # Task kind/field coherence, composite graph validity, and step-path id
  # discipline: invalid composition must fail at evaluation, not compile into
  # a model the runtime only rejects at admission.
  tasks = config.nixfied.tasks;
  taskNames = builtins.attrNames tasks;
  stepSafe = id: builtins.match "[A-Za-z0-9][A-Za-z0-9_-]*" id != null;
  taskIdsStepSafe =
    lib.all stepSafe taskNames && lib.all stepSafe (builtins.attrNames services);
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
    lib.all (step: builtins.hasAttr step.task tasks) (
      builtins.attrValues compositeTasks.${name}.steps
    )
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
      dep:
      dep == start || (!(builtins.elem dep seen) && stepReaches steps start dep (seen ++ [ dep ]))
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
  checks = [
    (expect (config.nixfied.target.system == system) "target.system must match the compile system")
    (expect (slotPolicy.min >= 0) "slotPolicy.min must be non-negative")
    (expect (slotPolicy.max >= slotPolicy.min) "slotPolicy.max must be >= min")
    (expect (
      slotPolicy.default >= slotPolicy.min && slotPolicy.default <= slotPolicy.max
    ) "slotPolicy.default must be within the slot range")
    (expect windowsInRange "per-slot candidate port windows must be in 1..65535")
    (expect windowsDoNotOverlap "per-slot candidate port windows must not overlap")
    # A model with no services is valid as long as it declares something to
    # run: the runtime supports service-less task selections.
    (expect (
      config.nixfied.services != { } || config.nixfied.tasks != { }
    ) "at least one service or task must be declared")
    (expect endpointHostsLoopback
      "service endpoint.host must be a loopback IP literal (127.x.x.x or ::1)"
    )
    (expect connectsToDeclared "service connectsTo targets must be declared services")
    (expect connectsToAcyclic "service connectsTo graph must be acyclic")
    (expect taskIdsStepSafe "task and service ids must match [A-Za-z0-9][A-Za-z0-9_-]* (step-path segments)")
    (expect leavesCoherent "a leaf task must declare an invocation and no steps")
    (expect compositesCoherent
      "a composite task carries only steps (no invocation, operationId, or requires) with step-safe names"
    )
    (expect stepTasksDeclared "composite steps must reference declared tasks")
    (expect stepDependsOnSiblings "composite step dependsOn must name a sibling step")
    (expect stepGraphsAcyclic "composite step dependency graph must be acyclic")
    (expect taskGraphAcyclic "the task reference graph must be acyclic")
  ];
in
lib.foldl' (acc: check: lib.seq check acc) config checks
