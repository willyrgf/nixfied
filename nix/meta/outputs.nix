# Public runtime data, included in its existing native owner scopes. Acquisition,
# redaction, failure selection, formatting and persistence remain native.
{ lib }:
let
  d = import ./declarations.nix;
  inherit (d)
    text
    boolean
    u16
    u32
    u64
    i32
    i64
    local
    ref
    list
    enum
    field
    ;
  output = name: d.inventory "output-schema ${name}";
  native = id: wire: description: {
    kind = "NativeDomain";
    inherit id wire description;
    rustPath = [ id ];
    producerCheck = null;
  };
  path =
    native "PathBuf" text
      "Native path serialization; non-UTF-8 paths retain serde's failure behavior.";
  host = native "LoopbackHost" text "The independently validated native IP loopback scalar.";
  usize = native "usize" u64 "Native address-sized byte length; Rust retains its platform domain.";
  required = name: value: field name value { kind = "Required"; };
  optional = name: value: field name value { kind = "Optional"; };
  omitted = name: value: field name value { kind = "OptionalOmitted"; };
  omitEmpty = name: value: field name value { kind = "OmitEmpty"; };
  boxed =
    field:
    field
    // {
      rust = field.rust // {
        storage = "Box";
      };
    };
  renamed =
    name: field:
    field
    // {
      rust = field.rust // {
        inherit name;
      };
    };
  owner = file: { file = "crates/nixfied-runtime/src/generated/${file}.rs"; };
  main = owner "main";
  control = owner "control";
  cleanup = owner "cleanup";
  task = owner "task";
  process = owner "process";
  error = owner "error";
  projection = owner "output";
  registry = owner "registry_identity";
  status = owner "status";
  record = identity: name: owner: visibility: derives: emission: decoder: description: fields: {
    inherit
      identity
      description
      decoder
      fields
      ;
    producer = "None";
    rust = owner // {
      inherit
        name
        visibility
        derives
        emission
        ;
    };
  };
  comparable = [
    "Debug"
    "Clone"
    "PartialEq"
    "Eq"
  ];
  copy = [
    "Debug"
    "Clone"
    "Copy"
    "PartialEq"
    "Eq"
  ];
  evidencePaths = [
    (required "stdoutPath" path "Redacted captured stdout evidence path.")
    (required "stderrPath" path "Redacted captured stderr evidence path.")
    (required "summaryPath" path "Native per-task summary evidence path.")
  ];
  errorFields = [
    (required "code" (enum "error-code")
      "Native failure classification, preserving admission/execution phases."
    )
    (required "exitClass" (enum "exit-class")
      "Native constructor-selected class; numeric command exit mapping stays native."
    )
    (required "message" text "Native diagnostic message, redacted at the output boundary.")
    (required "details" {
      kind = "OpenJson";
      description = "Open native diagnostics, including explicit null. Fixed nested structures have their own declared records.";
    } "Native detail composition and cause filtering remain authoritative.")
  ];
  vocabulary = coordinate: name: owner: visibility: derives: description: {
    inherit coordinate description;
    decoder = "NoDecoder";
    rust = owner // {
      inherit name visibility derives;
      emission = "Owned";
    };
  };
  annotation = description: recoveryTopic: { inherit description recoveryTopic; };
in
{
  records = [
    (record (local "CheckOutput") "CheckOutput" main "private" [ "Debug" ] "Owned" "NoDecoder"
      "Successful native model admission, without executing the declared task."
      [
        (required "modelPath" path "Admitted model file path.")
        (required "computedModelHash" text "Digest computed from the actual admitted model bytes.")
        (required "rawLen" usize "Number of bytes in the raw model document.")
        (required "projectId" text "Admitted project identity.")
        (required "runtimeAbi" text "Admitted exact runtime ABI.")
        (required "toolchainId" text "Admitted toolchain identity.")
        (required "targetSystem" text "Admitted target system.")
        (required "environment" text "The selected dev isolation namespace.")
        (required "slot" u32 "Validated selected slot.")
      ]
    )
    (record (output "run-service") "ServiceRunOutput" main "private" [ "Debug" ] "Owned" "NoDecoder"
      "Service evidence selected for the run, including an endpoint only when addressable."
      [
        (required "serviceId" text "Declared service identity.")
        (required "serviceInstanceId" text "Native service reuse identity.")
        (required "processKey" text "Registry evidence key for the selected service process.")
        (omitted "selectedEndpoint" (ref (
          output "selected-endpoint"
        )) "Primary selected endpoint; absent for endpoint-less services.")
      ]
    )
    (record (output "run-node") "NodeResult" main "private" [ "Debug" "Clone" ] "Owned" "NoDecoder"
      "One flattened task node's outcome and evidence links."
      (
        [
          (required "nodeId" text "Flattened node identity.")
          (required "taskId" text "Declared leaf task identity.")
          (required "success" boolean "Native success classification.")
          (optional "exitCode" i32 "Observed child code, or explicit null when unavailable.")
          (required "durationMs" u64 "Observed node duration in milliseconds.")
        ]
        ++ evidencePaths
      )
    )
    (record (output "run-json") "RunOutput" main "private" [ "Debug" ] "Owned" "NoDecoder"
      "Native run JSON, converted to Value, redacted and formatted at the existing output boundary."
      [
        (required "runId" text "Native run evidence identity.")
        (required "modelPath" path "Admitted model file path.")
        (required "computedModelHash" text "Digest of admitted bytes.")
        (required "durationMs" u64 "Observed run duration in milliseconds.")
        (required "services" (list (
          ref (output "run-service")
        )) "Selected service evidence in native order.")
        (required "tasks" (list (ref (output "run-task"))) "Completed task evidence in native order.")
        (omitted "task" (ref (output "run-task")) "Directly selected task evidence, when present.")
        (omitted "summaryPath" path "Direct task summary path, when present.")
        (omitEmpty "nodes" (list (
          ref (output "run-node")
        )) "Flattened node outcomes; empty list is omitted without a decoder default.")
        (omitted "runSummaryPath" path "Aggregate run summary path, when written.")
      ]
    )
    (record (output "run-summary-json") "RunSummaryOutput" main "private" [ ] "Borrowed" "NoDecoder"
      "Temporary aggregate-summary view; native success calculation, redaction, pretty serialization and writes remain unchanged."
      [
        (required "runId" text "Native run evidence identity.")
        (required "success" boolean "Native run success combined with all node outcomes.")
        (required "durationMs" u64 "Observed duration in milliseconds.")
        (required "services" (list (ref (output "run-service"))) "Selected service evidence.")
        (required "nodes" (list (ref (output "run-node"))) "Node evidence, including an empty list.")
        (required "tasks" (list (ref (output "run-task"))) "Completed task evidence.")
      ]
    )
    (record (output "run-task") "TaskRun" task "pub" comparable "Owned" "IgnoreUnknown"
      "The existing owned task evidence type, retained by CompletedEvidence; real readers accept unknown fields."
      (
        [
          (required "taskId" text "Declared leaf identity.")
          (required "stepPath" text
            "Flattened step path or direct leaf id; repeated leaf invocations retain distinct evidence."
          )
          (required "processKey" text "Registry process evidence key.")
          (optional "exitCode" i32 "Missing or null means absence; serialization always emits the member.")
          (required "timedOut" boolean "Native timeout observation.")
          (required "canceled" boolean "Native cancellation observation.")
          (required "success" boolean "Native exit-policy success classification.")
          (required "durationMs" u64 "Observed task duration in milliseconds.")
        ]
        ++ evidencePaths
      )
    )
    (record (output "selected-endpoint") "SelectedEndpoint" process "pub" comparable "Owned" "NoDecoder"
      "Native slot-plan endpoint selected for service execution."
      [
        (required "endpointId" text "Declared endpoint identity.")
        (required "host" host "Validated loopback bind host.")
        (required "port" u16 "Native planned TCP port.")
      ]
    )
    (record (output "ps-json") "PsReport" control "pub" comparable "Owned" "NoDecoder"
      "Native reconciled process observations; serialization performs no liveness checks."
      [
        (required "processes" (list (ref (output "ps-process"))) "Observed rows in native query order.")
      ]
    )
    (record (output "ps-process") "ProcessObservation" control "pub" comparable "Owned" "NoDecoder"
      "Registry and host facts assembled by native reconciliation."
      [
        (required "processKey" text "Registry process key.")
        (required "runId" text "Owning run id.")
        (optional "serviceInstanceId" text "Service instance identity or explicit null for task processes.")
        (required "pid" u32 "Observed process id.")
        (required "pgid" i32 "Observed signed process-group id.")
        (required "registryStatus" text "Native registry status string.")
        (required "reconciledStatus" text "Native reconciled status string.")
        (optional "serviceLifetime" text "Native service lifetime spelling or explicit null.")
        (required "borrowerCount" i64 "Signed count read through native SQLite handling.")
        (required "live" boolean "Native host liveness observation.")
      ]
    )
    (record (local "DownReport") "DownReport" control "pub" comparable "Owned" "NoDecoder"
      "Native down result after reconciliation and termination."
      [
        (required "stopped" (list text) "Process keys stopped by native control.")
        (required "stale" (list text) "Process keys classified stale by native reconciliation.")
      ]
    )
    (record (local "CleanupOutcome") "CleanupOutcome" cleanup "pub" comparable "Owned" "NoDecoder"
      "Result of successful marker-gated native cleanup."
      [
        (required "cleanupId" text "Recorded cleanup identity.")
        (required "deletedPath" path "Native deleted state path.")
      ]
    )
    (record (output "runtime-error-cause") "RuntimeCause" error "pub" [ "Debug" "Clone" ] "Owned"
      "NoDecoder"
      "Non-recursive safe cause; native selection and allowlisting remove unsafe infrastructure text."
      errorFields
    )
    (record (output "runtime-error") "RuntimeError" error "pub" [ "Debug" ] "Owned" "NoDecoder"
      "The one owned runtime error representation; native constructors retain phase, class, message and detail policy."
      (
        errorFields
        ++ [
          (boxed (
            omitEmpty "causes" (list (
              ref (output "runtime-error-cause")
            )) "Native ordered non-recursive causes; empty list omitted."
          ))
          (optional "modelPath" path "Associated model path or explicit null.")
          (optional "computedModelHash" text "Associated computed hash or explicit null.")
        ]
      )
    )
    (record (output "runtime-error-projection") "ProjectionDiagnostic" projection "pub" [ ] "Borrowed"
      "NoDecoder"
      "Temporary view of native projection failures; replay paths are deliberately converted lossily before borrowing."
      [
        (required "stream" (native "OutputStream" text
          "Native stdout/stderr enum serialization."
        ) "Affected native output stream.")
        (required "operation" (native "ProjectionOperation" text
          "Native open/read/write/flush/join enum serialization."
        ) "Failed native projection operation.")
        (required "kind" text "Redaction-safe native error-kind classification.")
        (required "path" text "Already formatted path; replay deliberately uses to_string_lossy.")
        (required "bytesWritten" u64 "Committed byte count before the failure.")
      ]
    )
    (record (output "runtime-error-port-conflict") "PORT_CONFLICT_KEY" process "private" [ ]
      "MemberNamesOnly"
      "NoDecoder"
      "One native with_detail insertion key; no envelope object or insertion helper is generated."
      [
        (required "portConflict" (ref (
          output "port-conflict"
        )) "Structured conflict evidence inserted through the existing serialization-failure fallback.")
      ]
    )
    (record (output "port-conflict-endpoint") "PortConflictEndpoint" process "private" [ ] "Borrowed"
      "NoDecoder"
      "Borrowed endpoint evidence for a proven host conflict."
      [
        (required "transport" text "Native transport spelling, currently tcp.")
        (required "family" text "Native address-family classification.")
        (renamed "host" (
          required "address" host "Validated loopback address serialized by its native scalar type."
        ))
        (required "port" u16 "Contended planned port.")
        (required "endpointId" text "Declared endpoint id.")
      ]
    )
    (record (output "port-conflict") "PortConflictDetails" process "private" [ ] "Borrowed" "NoDecoder"
      "Native lock/listener conflict evidence; no ownership or liveness decision is generated."
      [
        (required "reason" (enum "enum PortConflictReason") "Native choice of the proven conflict fact.")
        (required "projectId" text "Requesting project identity.")
        (required "endpoint" (ref (output "port-conflict-endpoint")) "Nested borrowed endpoint view.")
        (omitted "nixfiedOwner" (ref (
          output "nixfied-owner"
        )) "Present only when native ownership proof identifies a Nixfied owner.")
      ]
    )
    (record (output "nixfied-owner") "NixfiedOwner" process "private" [ "Debug" ] "Owned" "NoDecoder"
      "Owner identity assembled only from native verified registry/host evidence."
      [
        (required "projectId" text "Owning project id.")
        (required "environment" text "Owning dev namespace.")
        (required "slot" u32 "Owning slot.")
        (required "runId" text "Owning run.")
        (required "serviceId" text "Owning declared service.")
        (required "serviceInstanceId" text "Owning service instance.")
        (required "processKey" text "Owning registry process key.")
      ]
    )
    (record (local "RegistryIdentityDiagnostic") "RegistryIdentityDiagnostic" registry "private" [ ]
      "Borrowed"
      "NoDecoder"
      "Expected or found registry identity diagnostic; observed slot stays signed so corrupt negative values remain reportable."
      [
        (required "projectId" text "Expected or observed project id.")
        (required "environment" text "Expected or observed namespace.")
        (required "slot" i64 "Expected or observed signed registry slot.")
        (required "runtimeAbi" text "Expected or observed runtime ABI.")
        (required "toolchainId" text "Expected or observed toolchain identity.")
      ]
    )
  ];
  vocabularies = [
    (vocabulary "run-output-mode" "RunOutputMode" main "private" copy
      "Native run output selection; contextual default and parser repetition rules stay native."
    )
    (
      (vocabulary "error-code" "ErrorCode" error "pub" copy
        "Native failure classes; no per-code exit policy is generated."
      )
      // {
        annotations = {
          MODEL_NOT_STORE_OUTPUT = annotation "Model input is not an allowed realised store output." "model";
          MODEL_INVALID = annotation "Raw model bytes or structural values are invalid." "model";
          MODEL_ADMISSION = annotation "Native pre-execution model admission rejected a contract." "model";
          RUNTIME_ABI_MISMATCH = annotation "The model and runtime have different exact capability identities." "recovery";
          SOURCE_MISMATCH = annotation "Native source identity or fingerprint policy rejected the observed workspace." "context";
          PLATFORM_UNSUPPORTED = annotation "Required platform or containment capability is unavailable." "recovery";
          CLOSURE_MISSING = annotation "A required realised executable closure is unavailable." "model";
          REGISTRY_CORRUPT = annotation "Registry schema, identity, status or transactional evidence cannot be trusted." "state";
          STATE_UNWRITABLE = annotation "Native state or evidence I/O could not complete." "state";
          STATE_UNOWNED = annotation "Required state ownership proof is absent or inconsistent." "state";
          CLEANUP_REFUSED = annotation "Native cleanup policy or ownership checks refused deletion." "state";
          PORT_CONFLICT = annotation "A startup lock or listener proves a conflict at a planned endpoint." "placeholders";
          PORT_UNVERIFIABLE = annotation "Native endpoint ownership cannot be proved safely." "placeholders";
          PROC_ESCAPE = annotation "Native process containment observed an escape." "recovery";
          READINESS_TIMEOUT = annotation "The native service readiness budget expired." "services";
          CANCELED = annotation "Native cancellation interrupted the operation." "recovery";
          LEASE_STALE = annotation "Required run/service lease evidence is no longer current." "state";
          LEASE_CONFLICT = annotation "An existing lease conflicts with the requested ownership transition." "state";
          TASK_FAILED = annotation "An admitted task failed its native execution or success policy." "tasks";
          LIFECYCLE_FAILED = annotation "An admitted lifecycle operation or finalization stage failed." "services";
          DEPENDENCY_UNAVAILABLE = annotation "An admitted task or service dependency became unavailable." "services";
          SECRET_UNAVAILABLE = annotation "Native secret resolution could not obtain the declared material." "secrets";
          SECRET_LEAK_BLOCKED = annotation "Native secret handling blocked unsafe disclosure." "secrets";
          OUTPUT_MODE_INVALID = annotation "The requested output mode is outside the supported domain." "outputs";
          OUTPUT_MODE_CONFLICT = annotation "Output selections conflict under the native parser's rules." "outputs";
          TASK_SELECTION_INVALID = annotation "Native task/output selection validation rejected the request." "tasks";
          OUTPUT_PROJECTION_FAILED = annotation "Native output or evidence replay failed; cleanup still runs." "outputs";
        };
      }
    )
    (vocabulary "exit-class" "ExitClass" error "pub" copy
      "Native constructor-selected class; numeric process status is a separate native mapping."
    )
    (vocabulary "enum PortConflictReason" "PortConflictReason" process "private" copy
      "The two inventoried proven host conflict facts."
    )
    (vocabulary "status RunStatus" "RunStatus" status "pub" copy
      "Native runs.status vocabulary; transitions and terminal sets stay native."
    )
    (vocabulary "status ProcessStatus" "ProcessStatus" status "pub" copy
      "Native processes.status vocabulary; host reconciliation stays native."
    )
    (vocabulary "status RunLeaseStatus" "RunLeaseStatus" status "pub" copy
      "Native run_leases.status vocabulary; lease transitions stay native."
    )
    (vocabulary "status PortStatus" "PortStatus" status "pub" copy
      "Native ports.status vocabulary; reservation and ownership checks stay native."
    )
    (vocabulary "status CleanupStatus" "CleanupStatus" status "pub" copy
      "Native cleanups.status vocabulary; marker-gated deletion stays native."
    )
  ];
}
