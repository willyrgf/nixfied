# Model wire declarations. Algorithms and configured values stay in the native
# compiler; vocabulary members remain authored only in capability.txt.
{ lib }:
let
  d = import ./declarations.nix;
  inherit (d)
    text
    boolean
    u16
    u32
    i32
    nz32
    nz64
    list
    unique
    mapOf
    required
    omitted
    empty
    emptyOmitted
    enumDefault
    ;
  enum = name: d.enum "enum ${name}";
  ref = name: d.ref (d.inventory "primitive ${name}");
  id = name: {
    kind = "NativeDomain";
    id = name;
    wire = text;
    description = "${name} namespace wrapper; lexical and reference checks remain in native validation.";
    producerCheck = builtins.isString;
    rustPath = [ name ];
  };
  host = {
    kind = "NativeDomain";
    id = "LoopbackHost";
    wire = text;
    description = "Native IP loopback literal; Nix accepts canonical 127.x.x.x and ::1, Rust parses and validates independently.";
    producerCheck = import ../lib/loopback-host.nix { inherit lib; };
    rustPath = [ "LoopbackHost" ];
  };
  field =
    name: value: presence: nixEncode: description:
    (d.field name value presence description) // { inherit nixEncode; };
  record = name: description: fields: {
    identity = {
      kind = "Inventory";
      coordinate = "primitive ${name}";
    };
    inherit description fields;
    decoder = "RejectUnknown";
    producer = "Nix";
    rust = {
      file = "crates/nixfied-model/src/generated/types.rs";
      name = if name == "Invocation" then "InvocationSpec" else name;
      emission = "Owned";
      visibility = "pub";
      derives = [
        "Debug"
        "Clone"
        "PartialEq"
        "Eq"
      ];
    };
  };
  vocabulary = name: copy: description: {
    coordinate = if name == "StopSignal" then "signal" else "enum ${name}";
    decoder = "Closed";
    inherit description;
    rust = {
      inherit name;
      file = "crates/nixfied-model/src/generated/types.rs";
      emission = "Owned";
      visibility = "pub";
      derives = [
        "Debug"
        "Clone"
      ]
      ++ lib.optional copy "Copy"
      ++ [
        "PartialEq"
        "Eq"
      ];
    };
  };
  probeRecord =
    name: description: probeDescription:
    record name description [
      (field "operationId" (id "OperationId") required "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "probe" (ref "ProbeSpec") required "RequiredPresent" probeDescription)
      (field "terminal" (ref "TerminalSemantics") required "RequiredPresent" "Terminal evidence tokens.")
    ];
in
{
  vocabularies = [
    (vocabulary "ClosureKind" false "Declared role of a realised executable closure.")
    (vocabulary "ClosureEffect" false
      "Attested effect classes; native admission validates their coherence."
    )
    (vocabulary "StdinPolicy" true "Closed stdin or inherited runtime-command stdin.")
    (vocabulary "ProbeKind" true
      "TCP endpoint probe or bounded exec probe; native lowering checks the discriminator."
    )
    (vocabulary "TaskKind" true
      "Leaf invocation or static composite DAG; native lowering checks the discriminator."
    )
    (vocabulary "TaskDefaultOutput" true
      "Default output for a directly selected task; native Default remains Summary."
    )
    (vocabulary "ServiceLifetime" true
      "Service lifetime applied to the selected task's derived dependency closure."
    )
    (vocabulary "SecretSourceKind" true
      "Runtime secret resolver kind; no secret values are serialized."
    )
    (vocabulary "ContainmentRequirement" false
      "Required process-group or descendant-tree containment strength."
    )
    (vocabulary "SourceMode" false
      "Source identity interpretation: immutable snapshot/input or live workspace."
    )
    (vocabulary "DirtyPolicy" false "Policy for uncommitted live-workspace source changes.")
    (vocabulary "CleanupPolicy" false "Ordinary cleanup permission for owned state.")
    (vocabulary "PersistencePolicy" false "State persistence policy, distinct from service lifetime.")
    (vocabulary "StopSignal" true "The closed set of graceful shutdown signals the runtime can send.")
  ];
  records = [
    (record "Model" "The sole required semantic artifact admitted independently by the runtime." [
      (field "modelVersion" u32 required "RequiredPresent" "Exact numeric model contract version.")
      (field "toolchainId" text required "RequiredPresent" "Pinned toolchain contract identity.")
      (field "runtimeAbi" text required "RequiredPresent"
        "Exact runtime ABI derived from the authored capability bytes."
      )
      (field "generator" (ref "Generator") required "RequiredPresent" "Nix producer identity.")
      (field "project" (ref "Project") required "RequiredPresent" "Project identity and display name.")
      (field "target" (ref "Target") required "RequiredPresent" "Declared platform and closure target.")
      (field "codebases" (list (ref "Codebase")) required "RequiredPresent" "Declared source codebases.")
      (field "secrets" (mapOf (
        ref "SecretDescriptor"
      )) required "RequiredPresent" "Secret descriptors keyed by secret id; never secret material.")
      (field "environments" (unique text) required "RequiredPresent"
        "The sole isolation namespace is dev; membership does not exist."
      )
      (field "slotPolicy" (ref "SlotPolicy") required "RequiredPresent"
        "Accepted slot range and default selection."
      )
      (field "placement" (ref "Placement") required "RequiredPresent" "Derived candidate port windows.")
      (field "state" (ref "StatePolicy") required "RequiredPresent"
        "State compatibility and cleanup choices."
      )
      (field "closures" (mapOf (
        ref "ClosureSpec"
      )) required "RequiredPresent" "Realised executable closures keyed by closure id.")
      (field "services" (mapOf (
        ref "ServiceSpec"
      )) required "RequiredPresent" "Service declarations keyed by service id.")
      (field "tasks" (mapOf (
        ref "TaskSpec"
      )) required "RequiredPresent" "Task declarations keyed by task id.")
    ])
    (record "Generator" "Native Nix emitter provenance." [
      (field "name" text required "RequiredPresent" "Generator name.")
      (field "version" text required "RequiredPresent" "Generator toolchain version identity.")
      (field "emitter" text required "RequiredPresent" "Native model emission owner.")
    ])
    (record "Project" "Stable project identity and human-readable label." [
      (field "projectId" text required "RequiredPresent"
        "Stable project identifier; native validation owns lexical checks."
      )
      (field "name" text required "RequiredPresent" "Human-readable project name.")
    ])
    (record "Target" "Platform facts derived natively from the selected system." [
      (field "system" text required "RequiredPresent" "Nix system identifier.")
      (field "os" text required "RequiredPresent" "Operating-system component.")
      (field "arch" text required "RequiredPresent" "Architecture component.")
      (field "closureSystem" text required "RequiredPresent"
        "Required system for every executable closure."
      )
    ])
    (record "Codebase" "A source observed by invocations; native admission resolves its actual root." [
      (field "codebaseId" (id "CodebaseId") required "RequiredPresent" "Source namespace identifier.")
      (field "logicalRoot" text required "RequiredPresent"
        "Confined logical root within the declared source."
      )
      (field "sourceMode" (enum "SourceMode") required "RequiredPresent"
        "Live or immutable source interpretation."
      )
      (field "sourceIdentity" text required "RequiredPresent"
        "Live marker or immutable store source; native authoring apply preserves string context."
      )
      (field "sourcePolicy" (ref "SourcePolicy") required "RequiredPresent" "Source admission policy.")
    ])
    (record "SourcePolicy" "Native source-admission choices." [
      (field "dirtyPolicy" (enum "DirtyPolicy") required "RequiredPresent" "Uncommitted-change policy.")
      (field "admissionFingerprintPolicy" text required "RequiredPresent"
        "Declared fingerprint policy; native validation enforces the supported choice."
      )
    ])
    (record "StatePolicy" "Native state-marker compatibility and cleanup policy." [
      (field "markerIdentity" text required "RequiredPresent"
        "Required marker identity for adopting or cleaning state."
      )
      (field "stateEpoch" text required "RequiredPresent" "Project-selected compatibility epoch.")
      (field "cleanupPolicy" (enum "CleanupPolicy") required "RequiredPresent"
        "Ordinary cleanup permission."
      )
      (field "persistence" (enum "PersistencePolicy") required "RequiredPresent"
        "Persistence permission; independent from task service lifetime."
      )
    ])
    (record "SecretDescriptor" "A reference to runtime-resolved secret material." [
      (field "secretId" text required "RequiredPresent" "Declared secret id.")
      (field "source" (ref "SecretSource") required "RequiredPresent"
        "Resolver description, not its resolved value."
      )
    ])
    (record "SecretSource"
      "Environment or file resolver; native validation enforces cross-field coherence."
      [
        (field "kind" (enum "SecretSourceKind") required "RequiredPresent" "Resolver kind.")
        (field "envVar" text omitted "RequiredOmitAbsent" "Environment variable for an env-var resolver.")
        (field "path" text omitted "RequiredOmitAbsent" "Confined relative path for a file resolver.")
      ]
    )
    (record "SlotPolicy" "Slot bounds and default; native validation checks their ordering." [
      (field "min" u32 required "RequiredPresent" "Lowest admitted slot.")
      (field "default" u32 required "RequiredPresent" "Default selected slot.")
      (field "max" u32 required "RequiredPresent" "Highest admitted slot.")
    ])
    (record "Placement" "Natively derived port-placement facts." [
      (field "slotPlacements" (mapOf (
        ref "SlotPlacement"
      )) required "RequiredPresent" "Candidate window per string-encoded slot number.")
    ])
    (record "SlotPlacement" "A slot's candidate port window; host directory layout stays runtime-owned."
      [
        (field "slot" u32 required "RequiredPresent" "Slot number, required to match its map key.")
        (field "candidatePorts" (ref "CandidatePortWindow") required "RequiredPresent"
          "Inclusive candidate port bounds."
        )
      ]
    )
    (record "CandidatePortWindow"
      "Inclusive TCP candidate window; native validation checks range and demand."
      [
        (field "start" u16 required "RequiredPresent" "First candidate port.")
        (field "end" u16 required "RequiredPresent" "Last candidate port.")
      ]
    )
    (record "ClosureSpec"
      "Realised executable contract; native lowering derives bindings and executable selection."
      [
        (field "kind" (enum "ClosureKind") required "RequiredPresent" "Declared closure role.")
        (field "storePath" text required "RequiredPresent"
          "Realised package store path, retaining Nix dependency context."
        )
        (field "executable" text required "RequiredPresent" "Absolute declared executable path.")
        (field "targetSystem" text required "RequiredPresent" "Declared closure platform.")
        (field "operationBindings" (unique (
          id "OperationId"
        )) required "RequiredPresent" "Independently re-derived operation authorization set.")
        (field "requiresExecutable" boolean required "RequiredPresent"
          "Whether admission verifies executable permission."
        )
        (field "effects" (list (
          enum "ClosureEffect"
        )) required "RequiredPresent" "Attested effect classes, checked natively for coherence.")
      ]
    )
    (record "Invocation"
      "Inline anonymous argv/environment and tool contract; no invocation registry or runtime executable lookup."
      [
        (field "tools" (unique (
          id "ClosureId"
        )) required "RequiredPresent" "Declared tool closure ids in PATH precedence order.")
        (field "run" (list text) required "RequiredPresent"
          "Argv; native lowering resolves run[0] against the declared tools."
        )
        (field "executable" text required "RequiredPresent"
          "Nix-resolved executable, independently re-derived at admission."
        )
        (field "env" (mapOf text) required "RequiredPresent"
          "Hermetic declared environment; PATH remains runtime-owned."
        )
        (field "codebaseId" (id "CodebaseId") required "RequiredPresent" "Observed codebase reference.")
        (field "cwd" text required "RequiredPresent" "Confined relative working directory.")
        (field "stdin" (enum "StdinPolicy") required "RequiredPresent" "Child stdin policy.")
        (field "timeoutMs" nz64 required "RequiredPresent" "Positive invocation timeout in milliseconds.")
      ]
    )
    (record "ServiceSpec"
      "A generic foreground service; endpoint-less services retain ownership but make no addressability claim."
      [
        (field "lifecycle" (ref "Lifecycle") required "RequiredPresent" "Per-class lifecycle contract.")
        (field "endpoints" (mapOf (ref "Endpoint")) emptyOmitted "RequiredOmitEmpty"
          "Owned endpoints keyed by id; empty is omitted by both producers."
        )
        (field "primaryEndpoint" text omitted "RequiredOmitAbsent"
          "Primary endpoint id, coherent with the endpoint map."
        )
        (field "connectsTo" (unique (
          id "ServiceId"
        )) required "RequiredPresent" "Direct service dependencies and named-addressing scope.")
        (field "stateRefs" (list text) required "RequiredPresent"
          "Descriptive labels; execution lowering discards them, including for service reuse identity."
        )
        (field "logRefs" (list text) required "RequiredPresent"
          "Descriptive log labels, not evidence path selectors."
        )
        (field "containment" (enum "ContainmentRequirement") required "RequiredPresent"
          "Required native process containment strength."
        )
      ]
    )
    (record "Lifecycle"
      "Each lifecycle position binds exactly its native mechanism; preparation is a task reference."
      [
        (field "prepare" (ref "PrepareSpec") omitted "RequiredOmitAbsent"
          "Optional leaf or composite preparation task binding."
        )
        (field "start" (ref "StartSpec") required "RequiredPresent" "Spawn-and-own contract.")
        (field "ready" (ref "ReadySpec") required "RequiredPresent" "Readiness contract.")
        (field "health" (ref "HealthSpec") required "RequiredPresent" "Reuse-health contract.")
        (field "stop" (ref "StopSpec") required "RequiredPresent" "Signal shutdown contract.")
        (field "clean" (ref "CleanSpec") required "RequiredPresent" "Marker-gated cleanup contract.")
      ]
    )
    (record "PrepareSpec"
      "Optional task reference with full task semantics and native prepare-requires graph checks."
      [
        (field "task" (id "TaskId") required "RequiredPresent" "Declared preparation task id.")
      ]
    )
    (record "StartSpec" "Spawn and own the service's foreground process." [
      (field "operationId" (id "OperationId") required "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "invocation" (ref "Invocation") required "RequiredPresent" "Foreground service invocation.")
      (field "terminal" (ref "TerminalSemantics") required "RequiredPresent" "Terminal evidence tokens.")
    ])
    (probeRecord "ReadySpec" "Probe until ready under the native lifecycle rules." "Readiness probe.")
    (probeRecord "HealthSpec" "Verify whether an existing service remains reusable." "Health probe.")
    (record "StopSpec" "Signal-based shutdown, with native timeout escalation." [
      (field "operationId" (id "OperationId") required "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "signal" {
        kind = "Enum";
        coordinate = "signal";
      } required "RequiredPresent" "Graceful shutdown signal.")
      (field "timeoutMs" nz64 required "RequiredPresent"
        "Grace period in milliseconds before escalation."
      )
      (field "terminal" (ref "TerminalSemantics") required "RequiredPresent" "Terminal evidence tokens.")
    ])
    (record "CleanSpec" "Marker-gated native cleanup; no executable or probe binding." [
      (field "operationId" (id "OperationId") required "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "terminal" (ref "TerminalSemantics") required "RequiredPresent" "Terminal evidence tokens.")
    ])
    (record "Endpoint" "Loopback endpoint identity; the runtime assigns its port from the slot window."
      [
        (field "endpointId" text required "RequiredPresent"
          "Endpoint identifier, coherent with its map key."
        )
        (field "host" host required "RequiredPresent"
          "Loopback bind host with native parse/serde validation."
        )
      ]
    )
    (record "ProbeSpec"
      "Discriminator-plus-record probe shape; native validation rejects incoherent combinations."
      [
        (field "kind" (enum "ProbeKind") required "RequiredPresent" "TCP or exec mechanism.")
        (field "invocation" (ref "Invocation") omitted "RequiredOmitAbsent"
          "Bound short-lived invocation for exec probes only."
        )
        (field "timeoutMs" nz64 required "RequiredPresent"
          "Per-attempt timeout in milliseconds, independent of invocation.timeoutMs."
        )
        (field "retryIntervalMs" nz64 required "RequiredPresent" "Delay between attempts in milliseconds.")
        (field "maxAttempts" nz32 required "RequiredPresent" "Positive attempt limit.")
      ]
    )
    (record "TerminalSemantics" "Native lifecycle evidence result tokens." [
      (field "success" text required "RequiredPresent" "Result token recorded on success.")
      (field "failure" text required "RequiredPresent" "Result token recorded on failure.")
    ])
    (record "TaskSpec"
      "Leaf invocation or static composite DAG; native validation enforces kind/field coherence."
      [
        (field "kind" (enum "TaskKind") required "RequiredPresent" "Leaf or composite discriminator.")
        (field "defaultOutput" (enum "TaskDefaultOutput") (enumDefault "summary") "RequiredPresent"
          "Default for direct selection; its serde literal is independent of native Default."
        )
        (field "serviceLifetime" (enum "ServiceLifetime") required "RequiredPresent"
          "Lifetime for the task's full required-service closure."
        )
        (field "operationId" (id "OperationId") omitted "PreserveSupplied"
          "Leaf operation identity; composites omit it."
        )
        (field "invocation" (ref "Invocation") omitted "PreserveSupplied"
          "Leaf invocation; composites omit it."
        )
        (field "requires" (unique (id "ServiceId")) emptyOmitted "PreserveSupplied"
          "Direct leaf dependencies in authored order; the first supplies bare endpoint placeholders."
        )
        (field "servicesRequired" (unique (id "ServiceId")) empty "RequiredPresent"
          "Independently re-derived, sorted transitive service closure; Nix must always supply it."
        )
        (field "exitPolicy" (ref "ExitPolicy") omitted "PreserveSupplied"
          "Leaf success-code policy; composites omit it."
        )
        (field "steps" (mapOf (
          ref "StepSpec"
        )) emptyOmitted "PreserveSupplied" "Composite steps; leaf producers omit this map.")
        (field "artifactRefs" (list text) emptyOmitted "PreserveSupplied"
          "Descriptive artifact labels; leaf Nix producers retain supplied empty lists."
        )
        (field "logRefs" (list text) emptyOmitted "PreserveSupplied"
          "Descriptive log labels; not runtime evidence paths."
        )
        (field "summaryRefs" (list text) emptyOmitted "PreserveSupplied"
          "Descriptive summary labels; not runtime summary paths."
        )
      ]
    )
    (record "StepSpec" "A named composite step referencing a declared task." [
      (field "task" (id "TaskId") required "RequiredPresent" "Declared leaf or composite task reference.")
      (field "dependsOn" (unique text) emptyOmitted "RequiredPresent"
        "Sibling dependency names; Nix retains supplied empty lists."
      )
    ])
    (record "ExitPolicy"
      "Native leaf success classification; accepted child codes are not passed through as command status."
      [
        (field "successCodes" (unique i32) required "RequiredPresent"
          "Nonempty distinct success exit codes, further constrained by native validation."
        )
      ]
    )
  ];
}
