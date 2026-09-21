# Model wire declarations. Algorithms and configured values stay in the native
# compiler; vocabulary members remain authored only in capability.txt.
{ lib }:
let
  text = {
    kind = "Text";
  };
  boolean = {
    kind = "Boolean";
  };
  integer = signed: bits: nonzero: {
    kind = "Integer";
    inherit signed bits nonzero;
  };
  u16 = integer false 16 false;
  u32 = integer false 32 false;
  i32 = integer true 32 false;
  nz32 = integer false 32 true;
  nz64 = integer false 64 true;
  enum = name: {
    kind = "Enum";
    coordinate = "enum ${name}";
  };
  list = element: {
    kind = "List";
    inherit element;
    unique = false;
  };
  unique = element: {
    kind = "List";
    inherit element;
    unique = true;
  };
  mapOf = value: {
    kind = "Map";
    inherit value;
  };
  ref = name: {
    kind = "RecordRef";
    identity = {
      kind = "Inventory";
      coordinate = "primitive ${name}";
    };
  };
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
  required = {
    kind = "Required";
  };
  optional = {
    kind = "Optional";
  };
  default = literal: {
    kind = "Default";
    inherit literal;
  };
  # Policy arguments are mandatory: neither inventory punctuation nor a missing
  # producer input can silently select an encoding or decoder default.
  field = name: value: decode: rustEncode: nixEncode: description: {
    inherit
      name
      value
      decode
      rustEncode
      nixEncode
      description
      ;
    rust = {
      visibility = "pub";
      storage = "Direct";
    };
  };
  record = name: description: fields: {
    identity = {
      kind = "Inventory";
      coordinate = "primitive ${name}";
    };
    inherit description fields;
    decoder = "RejectUnknown";
    rust = {
      file = "crates/nixfied-model/src/generated/types.rs";
      module = [ "types" ];
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
      module = [ "types" ];
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
      (field "modelVersion" u32 required "Present" "RequiredPresent"
        "Exact numeric model contract version."
      )
      (field "toolchainId" text required "Present" "RequiredPresent"
        "Pinned toolchain contract identity."
      )
      (field "runtimeAbi" text required "Present" "RequiredPresent"
        "Exact runtime ABI derived from the authored capability bytes."
      )
      (field "generator" (ref "Generator") required "Present" "RequiredPresent" "Nix producer identity.")
      (field "project" (ref "Project") required "Present" "RequiredPresent"
        "Project identity and display name."
      )
      (field "target" (ref "Target") required "Present" "RequiredPresent"
        "Declared platform and closure target."
      )
      (field "codebases" (list (
        ref "Codebase"
      )) required "Present" "RequiredPresent" "Declared source codebases.")
      (field "secrets" (mapOf (ref "SecretDescriptor")) required "Present" "RequiredPresent"
        "Secret descriptors keyed by secret id; never secret material."
      )
      (field "environments" (unique text) required "Present" "RequiredPresent"
        "The sole isolation namespace is dev; membership does not exist."
      )
      (field "slotPolicy" (ref "SlotPolicy") required "Present" "RequiredPresent"
        "Accepted slot range and default selection."
      )
      (field "placement" (ref "Placement") required "Present" "RequiredPresent"
        "Derived candidate port windows."
      )
      (field "state" (ref "StatePolicy") required "Present" "RequiredPresent"
        "State compatibility and cleanup choices."
      )
      (field "closures" (mapOf (
        ref "ClosureSpec"
      )) required "Present" "RequiredPresent" "Realised executable closures keyed by closure id.")
      (field "services" (mapOf (
        ref "ServiceSpec"
      )) required "Present" "RequiredPresent" "Service declarations keyed by service id.")
      (field "tasks" (mapOf (
        ref "TaskSpec"
      )) required "Present" "RequiredPresent" "Task declarations keyed by task id.")
    ])
    (record "Generator" "Native Nix emitter provenance." [
      (field "name" text required "Present" "RequiredPresent" "Generator name.")
      (field "version" text required "Present" "RequiredPresent" "Generator toolchain version identity.")
      (field "emitter" text required "Present" "RequiredPresent" "Native model emission owner.")
    ])
    (record "Project" "Stable project identity and human-readable label." [
      (field "projectId" text required "Present" "RequiredPresent"
        "Stable project identifier; native validation owns lexical checks."
      )
      (field "name" text required "Present" "RequiredPresent" "Human-readable project name.")
    ])
    (record "Target" "Platform facts derived natively from the selected system." [
      (field "system" text required "Present" "RequiredPresent" "Nix system identifier.")
      (field "os" text required "Present" "RequiredPresent" "Operating-system component.")
      (field "arch" text required "Present" "RequiredPresent" "Architecture component.")
      (field "closureSystem" text required "Present" "RequiredPresent"
        "Required system for every executable closure."
      )
    ])
    (record "Codebase" "A source observed by invocations; native admission resolves its actual root." [
      (field "codebaseId" (id "CodebaseId") required "Present" "RequiredPresent"
        "Source namespace identifier."
      )
      (field "logicalRoot" text required "Present" "RequiredPresent"
        "Confined logical root within the declared source."
      )
      (field "sourceMode" (enum "SourceMode") required "Present" "RequiredPresent"
        "Live or immutable source interpretation."
      )
      (field "sourceIdentity" text required "Present" "RequiredPresent"
        "Live marker or immutable store source; native authoring apply preserves string context."
      )
      (field "sourcePolicy" (ref "SourcePolicy") required "Present" "RequiredPresent"
        "Source admission policy."
      )
    ])
    (record "SourcePolicy" "Native source-admission choices." [
      (field "dirtyPolicy" (enum "DirtyPolicy") required "Present" "RequiredPresent"
        "Uncommitted-change policy."
      )
      (field "admissionFingerprintPolicy" text required "Present" "RequiredPresent"
        "Declared fingerprint policy; native validation enforces the supported choice."
      )
    ])
    (record "StatePolicy" "Native state-marker compatibility and cleanup policy." [
      (field "markerIdentity" text required "Present" "RequiredPresent"
        "Required marker identity for adopting or cleaning state."
      )
      (field "stateEpoch" text required "Present" "RequiredPresent"
        "Project-selected compatibility epoch."
      )
      (field "cleanupPolicy" (enum "CleanupPolicy") required "Present" "RequiredPresent"
        "Ordinary cleanup permission."
      )
      (field "persistence" (enum "PersistencePolicy") required "Present" "RequiredPresent"
        "Persistence permission; independent from task service lifetime."
      )
    ])
    (record "SecretDescriptor" "A reference to runtime-resolved secret material." [
      (field "secretId" text required "Present" "RequiredPresent" "Declared secret id.")
      (field "source" (ref "SecretSource") required "Present" "RequiredPresent"
        "Resolver description, not its resolved value."
      )
    ])
    (record "SecretSource"
      "Environment or file resolver; native validation enforces cross-field coherence."
      [
        (field "kind" (enum "SecretSourceKind") required "Present" "RequiredPresent" "Resolver kind.")
        (field "envVar" text optional "OmitAbsent" "RequiredOmitAbsent"
          "Environment variable for an env-var resolver."
        )
        (field "path" text optional "OmitAbsent" "RequiredOmitAbsent"
          "Confined relative path for a file resolver."
        )
      ]
    )
    (record "SlotPolicy" "Slot bounds and default; native validation checks their ordering." [
      (field "min" u32 required "Present" "RequiredPresent" "Lowest admitted slot.")
      (field "default" u32 required "Present" "RequiredPresent" "Default selected slot.")
      (field "max" u32 required "Present" "RequiredPresent" "Highest admitted slot.")
    ])
    (record "Placement" "Natively derived port-placement facts." [
      (field "slotPlacements" (mapOf (
        ref "SlotPlacement"
      )) required "Present" "RequiredPresent" "Candidate window per string-encoded slot number.")
    ])
    (record "SlotPlacement" "A slot's candidate port window; host directory layout stays runtime-owned."
      [
        (field "slot" u32 required "Present" "RequiredPresent"
          "Slot number, required to match its map key."
        )
        (field "candidatePorts" (ref "CandidatePortWindow") required "Present" "RequiredPresent"
          "Inclusive candidate port bounds."
        )
      ]
    )
    (record "CandidatePortWindow"
      "Inclusive TCP candidate window; native validation checks range and demand."
      [
        (field "start" u16 required "Present" "RequiredPresent" "First candidate port.")
        (field "end" u16 required "Present" "RequiredPresent" "Last candidate port.")
      ]
    )
    (record "ClosureSpec"
      "Realised executable contract; native lowering derives bindings and executable selection."
      [
        (field "kind" (enum "ClosureKind") required "Present" "RequiredPresent" "Declared closure role.")
        (field "storePath" text required "Present" "RequiredPresent"
          "Realised package store path, retaining Nix dependency context."
        )
        (field "executable" text required "Present" "RequiredPresent" "Absolute declared executable path.")
        (field "targetSystem" text required "Present" "RequiredPresent" "Declared closure platform.")
        (field "operationBindings" (unique (
          id "OperationId"
        )) required "Present" "RequiredPresent" "Independently re-derived operation authorization set.")
        (field "requiresExecutable" boolean required "Present" "RequiredPresent"
          "Whether admission verifies executable permission."
        )
        (field "effects" (list (
          enum "ClosureEffect"
        )) required "Present" "RequiredPresent" "Attested effect classes, checked natively for coherence.")
      ]
    )
    (record "Invocation"
      "Inline anonymous argv/environment and tool contract; no invocation registry or runtime executable lookup."
      [
        (field "tools" (unique (
          id "ClosureId"
        )) required "Present" "RequiredPresent" "Declared tool closure ids in PATH precedence order.")
        (field "run" (list text) required "Present" "RequiredPresent"
          "Argv; native lowering resolves run[0] against the declared tools."
        )
        (field "executable" text required "Present" "RequiredPresent"
          "Nix-resolved executable, independently re-derived at admission."
        )
        (field "env" (mapOf text) required "Present" "RequiredPresent"
          "Hermetic declared environment; PATH remains runtime-owned."
        )
        (field "codebaseId" (id "CodebaseId") required "Present" "RequiredPresent"
          "Observed codebase reference."
        )
        (field "cwd" text required "Present" "RequiredPresent" "Confined relative working directory.")
        (field "stdin" (enum "StdinPolicy") required "Present" "RequiredPresent" "Child stdin policy.")
        (field "timeoutMs" nz64 required "Present" "RequiredPresent"
          "Positive invocation timeout in milliseconds."
        )
      ]
    )
    (record "ServiceSpec"
      "A generic foreground service; endpoint-less services retain ownership but make no addressability claim."
      [
        (field "lifecycle" (ref "Lifecycle") required "Present" "RequiredPresent"
          "Per-class lifecycle contract."
        )
        (field "endpoints" (mapOf (ref "Endpoint")) (default { }) "OmitEmpty" "RequiredOmitEmpty"
          "Owned endpoints keyed by id; empty is omitted by both producers."
        )
        (field "primaryEndpoint" text optional "OmitAbsent" "RequiredOmitAbsent"
          "Primary endpoint id, coherent with the endpoint map."
        )
        (field "connectsTo" (unique (
          id "ServiceId"
        )) required "Present" "RequiredPresent" "Direct service dependencies and named-addressing scope.")
        (field "stateRefs" (list text) required "Present" "RequiredPresent"
          "Descriptive labels; execution lowering discards them, including for service reuse identity."
        )
        (field "logRefs" (list text) required "Present" "RequiredPresent"
          "Descriptive log labels, not evidence path selectors."
        )
        (field "containment" (enum "ContainmentRequirement") required "Present" "RequiredPresent"
          "Required native process containment strength."
        )
      ]
    )
    (record "Lifecycle"
      "Each lifecycle position binds exactly its native mechanism; preparation is a task reference."
      [
        (field "prepare" (ref "PrepareSpec") optional "OmitAbsent" "RequiredOmitAbsent"
          "Optional leaf or composite preparation task binding."
        )
        (field "start" (ref "StartSpec") required "Present" "RequiredPresent" "Spawn-and-own contract.")
        (field "ready" (ref "ReadySpec") required "Present" "RequiredPresent" "Readiness contract.")
        (field "health" (ref "HealthSpec") required "Present" "RequiredPresent" "Reuse-health contract.")
        (field "stop" (ref "StopSpec") required "Present" "RequiredPresent" "Signal shutdown contract.")
        (field "clean" (ref "CleanSpec") required "Present" "RequiredPresent"
          "Marker-gated cleanup contract."
        )
      ]
    )
    (record "PrepareSpec"
      "Optional task reference with full task semantics and native prepare-requires graph checks."
      [
        (field "task" (id "TaskId") required "Present" "RequiredPresent" "Declared preparation task id.")
      ]
    )
    (record "StartSpec" "Spawn and own the service's foreground process." [
      (field "operationId" (id "OperationId") required "Present" "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "invocation" (ref "Invocation") required "Present" "RequiredPresent"
        "Foreground service invocation."
      )
      (field "terminal" (ref "TerminalSemantics") required "Present" "RequiredPresent"
        "Terminal evidence tokens."
      )
    ])
    (record "ReadySpec" "Probe until ready under the native lifecycle rules." [
      (field "operationId" (id "OperationId") required "Present" "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "probe" (ref "ProbeSpec") required "Present" "RequiredPresent" "Readiness probe.")
      (field "terminal" (ref "TerminalSemantics") required "Present" "RequiredPresent"
        "Terminal evidence tokens."
      )
    ])
    (record "HealthSpec" "Verify whether an existing service remains reusable." [
      (field "operationId" (id "OperationId") required "Present" "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "probe" (ref "ProbeSpec") required "Present" "RequiredPresent" "Health probe.")
      (field "terminal" (ref "TerminalSemantics") required "Present" "RequiredPresent"
        "Terminal evidence tokens."
      )
    ])
    (record "StopSpec" "Signal-based shutdown, with native timeout escalation." [
      (field "operationId" (id "OperationId") required "Present" "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "signal" {
        kind = "Enum";
        coordinate = "signal";
      } required "Present" "RequiredPresent" "Graceful shutdown signal.")
      (field "timeoutMs" nz64 required "Present" "RequiredPresent"
        "Grace period in milliseconds before escalation."
      )
      (field "terminal" (ref "TerminalSemantics") required "Present" "RequiredPresent"
        "Terminal evidence tokens."
      )
    ])
    (record "CleanSpec" "Marker-gated native cleanup; no executable or probe binding." [
      (field "operationId" (id "OperationId") required "Present" "RequiredPresent"
        "Globally unique operation identity."
      )
      (field "terminal" (ref "TerminalSemantics") required "Present" "RequiredPresent"
        "Terminal evidence tokens."
      )
    ])
    (record "Endpoint" "Loopback endpoint identity; the runtime assigns its port from the slot window."
      [
        (field "endpointId" text required "Present" "RequiredPresent"
          "Endpoint identifier, coherent with its map key."
        )
        (field "host" host required "Present" "RequiredPresent"
          "Loopback bind host with native parse/serde validation."
        )
      ]
    )
    (record "ProbeSpec"
      "Discriminator-plus-record probe shape; native validation rejects incoherent combinations."
      [
        (field "kind" (enum "ProbeKind") required "Present" "RequiredPresent" "TCP or exec mechanism.")
        (field "invocation" (ref "Invocation") optional "OmitAbsent" "RequiredOmitAbsent"
          "Bound short-lived invocation for exec probes only."
        )
        (field "timeoutMs" nz64 required "Present" "RequiredPresent"
          "Per-attempt timeout in milliseconds, independent of invocation.timeoutMs."
        )
        (field "retryIntervalMs" nz64 required "Present" "RequiredPresent"
          "Delay between attempts in milliseconds."
        )
        (field "maxAttempts" nz32 required "Present" "RequiredPresent" "Positive attempt limit.")
      ]
    )
    (record "TerminalSemantics" "Native lifecycle evidence result tokens." [
      (field "success" text required "Present" "RequiredPresent" "Result token recorded on success.")
      (field "failure" text required "Present" "RequiredPresent" "Result token recorded on failure.")
    ])
    (record "TaskSpec"
      "Leaf invocation or static composite DAG; native validation enforces kind/field coherence."
      [
        (field "kind" (enum "TaskKind") required "Present" "RequiredPresent"
          "Leaf or composite discriminator."
        )
        (field "defaultOutput" (enum "TaskDefaultOutput") (default "summary") "Present" "RequiredPresent"
          "Default for direct selection; its serde literal is independent of native Default."
        )
        (field "serviceLifetime" (enum "ServiceLifetime") required "Present" "RequiredPresent"
          "Lifetime for the task's full required-service closure."
        )
        (field "operationId" (id "OperationId") optional "OmitAbsent" "PreserveSupplied"
          "Leaf operation identity; composites omit it."
        )
        (field "invocation" (ref "Invocation") optional "OmitAbsent" "PreserveSupplied"
          "Leaf invocation; composites omit it."
        )
        (field "requires" (unique (id "ServiceId")) (default [ ]) "OmitEmpty" "PreserveSupplied"
          "Direct leaf dependencies in authored order; the first supplies bare endpoint placeholders."
        )
        (field "servicesRequired" (unique (id "ServiceId")) (default [ ]) "Present" "RequiredPresent"
          "Independently re-derived, sorted transitive service closure; Nix must always supply it."
        )
        (field "exitPolicy" (ref "ExitPolicy") optional "OmitAbsent" "PreserveSupplied"
          "Leaf success-code policy; composites omit it."
        )
        (field "steps" (mapOf (ref "StepSpec")) (default
          { }
        ) "OmitEmpty" "PreserveSupplied" "Composite steps; leaf producers omit this map.")
        (field "artifactRefs" (list text) (default [ ]) "OmitEmpty" "PreserveSupplied"
          "Descriptive artifact labels; leaf Nix producers retain supplied empty lists."
        )
        (field "logRefs" (list text) (default
          [ ]
        ) "OmitEmpty" "PreserveSupplied" "Descriptive log labels; not runtime evidence paths.")
        (field "summaryRefs" (list text) (default
          [ ]
        ) "OmitEmpty" "PreserveSupplied" "Descriptive summary labels; not runtime summary paths.")
      ]
    )
    (record "StepSpec" "A named composite step referencing a declared task." [
      (field "task" (id "TaskId") required "Present" "RequiredPresent"
        "Declared leaf or composite task reference."
      )
      (field "dependsOn" (unique text) (default
        [ ]
      ) "OmitEmpty" "RequiredPresent" "Sibling dependency names; Nix retains supplied empty lists.")
    ])
    (record "ExitPolicy"
      "Native leaf success classification; accepted child codes are not passed through as command status."
      [
        (field "successCodes" (unique i32) required "Present" "RequiredPresent"
          "Nonempty distinct success exit codes, further constrained by native validation."
        )
      ]
    )
  ];
}
