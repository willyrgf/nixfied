# Wire descriptions for native runtime outputs. Output records generate no Rust;
# each vocabulary names only the enum and owner file that nix/meta/rust.nix emits.
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
  owner = file: { file = "crates/nixfied-runtime/src/generated/${file}.rs"; };
  main = owner "main";
  process = owner "process";
  error = owner "error";
  status = owner "status";
  record = identity: decoder: description: fields: {
    inherit
      identity
      description
      decoder
      fields
      ;
    producer = "None";
  };
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
  vocabulary = coordinate: name: owner: description: {
    inherit coordinate description;
    decoder = "NoDecoder";
    rust = owner // {
      inherit name;
    };
  };
  annotation = description: contextTopic: { inherit description contextTopic; };
in
{
  records = [
    (record (local "CheckOutput") "NoDecoder"
      "Successful native manifest admission, without executing the declared task."
      [
        (required "manifestPath" path "Admitted manifest file path.")
        (required "computedManifestHash" text "Digest computed from the actual admitted manifest bytes.")
        (required "rawLen" usize "Number of bytes in the raw manifest document.")
        (required "projectId" text "Admitted project identity.")
        (required "runtimeAbi" text "Admitted exact runtime ABI.")
        (required "toolchainId" text "Admitted toolchain identity.")
        (required "targetSystem" text "Admitted target system.")
        (required "environment" text "The selected dev isolation namespace.")
        (required "slot" u32 "Validated selected slot.")
      ]
    )
    (record (output "run-service") "NoDecoder"
      "Service evidence selected for the run, including an endpoint only when addressable."
      [
        (required "serviceId" text "Declared service identity.")
        (required "serviceInstanceId" text "Run-scoped service reference derived from the run ID and declared name.")
        (required "processKey" text "Registry evidence key for the selected service process.")
        (omitted "selectedEndpoint" (ref (
          output "selected-endpoint"
        )) "Primary selected endpoint; absent for endpoint-less services.")
      ]
    )
    (record (output "run-node") "NoDecoder" "One flattened task node's outcome and evidence links." (
      [
        (required "nodeId" text "Flattened node identity.")
        (required "taskId" text "Declared leaf task identity.")
        (required "success" boolean "Native success classification.")
        (optional "exitCode" i32 "Observed child code, or explicit null when unavailable.")
        (required "durationMs" u64 "Observed node duration in milliseconds.")
      ]
      ++ evidencePaths
    ))
    (record (output "run-json") "NoDecoder"
      "Native run JSON, converted to Value, redacted and formatted at the existing output boundary."
      [
        (required "runId" text "Native run evidence identity.")
        (required "manifestPath" path "Admitted manifest file path.")
        (required "computedManifestHash" text "Digest of admitted bytes.")
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
    (record (output "run-summary-json") "NoDecoder"
      "Aggregate run summary combining overall success with service, node and task evidence."
      [
        (required "runId" text "Native run evidence identity.")
        (required "success" boolean "Native run success combined with all node outcomes.")
        (required "durationMs" u64 "Observed duration in milliseconds.")
        (required "services" (list (ref (output "run-service"))) "Selected service evidence.")
        (required "nodes" (list (ref (output "run-node"))) "Node evidence, including an empty list.")
        (required "tasks" (list (ref (output "run-task"))) "Completed task evidence.")
      ]
    )
    (record (output "run-task") "IgnoreUnknown"
      "Evidence for one task execution: its outcome, timeout or cancellation, duration and paths to captured output."
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
    (record (output "selected-endpoint") "NoDecoder"
      "Native slot-plan endpoint selected for service execution."
      [
        (required "endpointId" text "Declared endpoint identity.")
        (required "host" host "Validated loopback bind host.")
        (required "port" u16 "Native planned TCP port.")
      ]
    )
    (record (output "ps-json") "NoDecoder"
      "Read-only process observations from one registry snapshot; ps writes nothing."
      [
        (required "processes" (list (ref (output "ps-process"))) "Observed rows in native query order.")
      ]
    )
    (record (output "ps-process") "NoDecoder"
      "Recorded facts and host observations from a read-only snapshot."
      [
        (required "processKey" text "Registry process key.")
        (required "runId" text "Owning run id.")
        (optional "serviceInstanceId" text "Service instance identity or explicit null for task processes.")
        (required "pid" u32 "Observed process id.")
        (required "pgid" i32 "Observed signed process-group id.")
        (required "registryStatus" text "Native registry status string.")
        (required "observedStatus" text "Recorded status as currently observed on the host; observation writes nothing.")
        (required "ownership" text "`unresolved` until process, group, and tracked descendants are proven gone; otherwise `settled`.")
        (required "live" boolean "Native host liveness observation.")
      ]
    )
    (record (output "run-daemon-json") "NoDecoder"
      "Background establishment acknowledgement: identity and evidence location, never readiness or task success."
      [
        (required "runId" text "Immutable identity of the established session.")
        (required "runDir" path "Retained evidence directory of the session.")
        (required "logsDir" path "Retained redacted log directory of the session.")
      ]
    )
    (record (local "DownReport") "NoDecoder"
      "Native down result: a canceled live session, or dead-owner recovery."
      [
        (omitted "canceledRunId" text "The live session that received the request and settled.")
        (required "stopped" (list text) "Process keys stopped by native control.")
        (required "stale" (list text) "Process keys that recovery found already gone.")
      ]
    )
    (record (local "CleanupOutcome") "NoDecoder"
      "Result of native cleanup: one deleted data generation, or an absent tree with nothing pending."
      [
        (required "result" text "`deleted` or `absent`.")
        (omitted "cleanupId" text "Deletion operation identity; present only for `deleted`.")
        (omitted "deletedPath" path "Deleted application root; present only for `deleted`.")
        (omitted "targetPath" path "Absent application root; present only for `absent`.")
      ]
    )
    (record (output "runtime-error-cause") "NoDecoder"
      "Non-recursive safe cause; native selection and allowlisting remove unsafe infrastructure text."
      errorFields
    )
    (record (output "runtime-error") "NoDecoder"
      "The one owned runtime error representation; native constructors retain phase, class, message and detail policy."
      (
        errorFields
        ++ [
          (omitEmpty "causes" (list (
            ref (output "runtime-error-cause")
          )) "Native ordered non-recursive causes; empty list omitted.")
          (optional "manifestPath" path "Associated manifest path or explicit null.")
          (optional "computedManifestHash" text "Associated computed hash or explicit null.")
        ]
      )
    )
    (record (output "runtime-error-projection") "NoDecoder"
      "A failure while reading or writing an output projection: the affected stream, operation, redaction-safe error kind, display path and committed byte count."
      [
        (required "stream" (native "OutputStream" text
          "Native stdout/stderr enum serialization."
        ) "Affected native output stream.")
        (required "operation" (native "ProjectionOperation" text
          "Native open/read/write/join enum serialization."
        ) "Failed native projection operation.")
        (required "kind" text "Redaction-safe native error-kind classification.")
        (required "path" text "Already formatted path; delivery deliberately uses to_string_lossy.")
        (required "bytesWritten" u64 "Committed byte count before the failure.")
      ]
    )
    (record (output "runtime-error-port-conflict") "NoDecoder"
      "The portConflict member of error details identifies the contended endpoint and any verified Nixfied owner."
      [
        (required "portConflict" (ref (
          output "port-conflict"
        )) "Structured conflict evidence inserted through the existing serialization-failure fallback.")
      ]
    )
    (record (output "port-conflict-endpoint") "NoDecoder"
      "Endpoint evidence for a proven host conflict."
      [
        (required "transport" text "Native transport spelling, currently tcp.")
        (required "family" text "Native address-family classification.")
        (required "address" host "Validated loopback address serialized by its native scalar type.")
        (required "port" u16 "Contended planned port.")
        (required "endpointId" text "Declared endpoint id.")
      ]
    )
    (record (output "port-conflict") "NoDecoder"
      "Native lock/listener conflict evidence; no ownership or liveness decision is generated."
      [
        (required "reason" (enum "enum PortConflictReason") "Native choice of the proven conflict fact.")
        (required "projectId" text "Requesting project identity.")
        (required "endpoint" (ref (output "port-conflict-endpoint")) "The contended endpoint.")
        (omitted "nixfiedOwner" (ref (
          output "nixfied-owner"
        )) "Present only when native ownership proof identifies a Nixfied owner.")
      ]
    )
    (record (output "nixfied-owner") "NoDecoder"
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
    (record (local "RegistryIdentityDiagnostic") "NoDecoder"
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
    (vocabulary "run-output-mode" "RunOutputMode" main
      "Native run output selection; contextual default and parser repetition rules stay native."
    )
    (
      (vocabulary "error-code" "ErrorCode" error
        "Native failure classes; no per-code exit policy is generated."
      )
      // {
        # The background launcher decodes an owner's pre-establishment rejection.
        decoder = "Closed";
        annotations = {
          MANIFEST_NOT_STORE_OUTPUT = annotation "Manifest input is not an allowed realised store output." "manifest";
          MANIFEST_INVALID = annotation "Raw manifest bytes or structural values are invalid." "manifest";
          MANIFEST_ADMISSION = annotation "Native pre-execution manifest admission rejected a contract." "manifest";
          RUNTIME_ABI_MISMATCH = annotation "The manifest and runtime have different exact capability identities." "recovery";
          SOURCE_MISMATCH = annotation "Native source identity or fingerprint policy rejected the observed workspace." "context";
          PLATFORM_UNSUPPORTED = annotation "Required platform or containment capability is unavailable." "recovery";
          CLOSURE_MISSING = annotation "A required realised executable closure is unavailable." "manifest";
          REGISTRY_CORRUPT = annotation "Registry schema, identity, status or transactional evidence cannot be trusted." "state";
          STATE_UNWRITABLE = annotation "Native state or evidence I/O could not complete." "state";
          STATE_UNOWNED = annotation "Required state ownership proof is absent or inconsistent." "state";
          CLEANUP_REFUSED = annotation "Native persistence, ownership, or process checks refused deletion, or a pending deletion remains unresolved." "state";
          PORT_CONFLICT = annotation "A startup lock or listener proves a conflict at a planned endpoint." "placeholders";
          PORT_UNVERIFIABLE = annotation "Native endpoint ownership cannot be proved safely." "placeholders";
          PROC_ESCAPE = annotation "Required process containment or foreground-process identity could not be maintained or verified." "recovery";
          READINESS_TIMEOUT = annotation "The native service readiness budget expired." "services";
          CANCELED = annotation "Native cancellation interrupted the operation." "recovery";
          TASK_FAILED = annotation "An admitted task failed its native execution or success policy." "tasks";
          LIFECYCLE_FAILED = annotation "An admitted lifecycle operation or finalization stage failed." "services";
          DEPENDENCY_UNAVAILABLE = annotation "An admitted task or service dependency became unavailable." "services";
          SECRET_UNAVAILABLE = annotation "Native secret resolution could not obtain the declared material." "secrets";
          SECRET_LEAK_BLOCKED = annotation "Native secret handling blocked unsafe disclosure." "secrets";
          OUTPUT_MODE_INVALID = annotation "The requested output mode is outside the supported domain." "outputs";
          OUTPUT_MODE_CONFLICT = annotation "Output selections conflict under the native parser's rules." "outputs";
          TASK_SELECTION_INVALID = annotation "Native task/output selection validation rejected the request." "tasks";
          OUTPUT_PROJECTION_FAILED = annotation "Command-owned output delivery failed, was interrupted, or could not be confirmed; the session is already settled." "outputs";
        };
      }
    )
    (vocabulary "exit-class" "ExitClass" error
      "Native constructor-selected class; numeric process status is a separate native mapping."
    )
    (vocabulary "enum PortConflictReason" "PortConflictReason" process
      "The two inventoried proven host conflict facts."
    )
    (vocabulary "status ExecutionOutcome" "ExecutionOutcome" status
      "Immutable session execution outcome; absence means execution has not settled."
    )
    (vocabulary "status FinalizationStatus" "FinalizationStatus" status
      "Session resource finalization, independent of execution outcome and output sealing."
    )
    (vocabulary "status ProcessRole" "ProcessRole" status
      "Durable workload role; preparation is a task occurrence and probes own separate process evidence."
    )
    (vocabulary "status ProcessStatus" "ProcessStatus" status
      "Native processes.status vocabulary; ownership settlement is the separate ownership column."
    )
    (vocabulary "status PortStatus" "PortStatus" status
      "Native ports.status vocabulary: endpoint evidence (reserved until verified, then active), never a socket reservation."
    )
    (vocabulary "status CleanupStatus" "CleanupStatus" status
      "Native cleanups.status vocabulary; marker-gated deletion stays native."
    )
  ];
}
