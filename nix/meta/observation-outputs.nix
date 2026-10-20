# Associated-data alternatives stay native. Every wire alternative is described
# separately; only closed unit vocabularies generate native enums.
{
  d,
  output,
  record,
  required,
  omitted,
  vocabulary,
  owner,
}:
let
  inherit (d)
    text
    u16
    u32
    u64
    i32
    i64
    ref
    list
    enum
    ;
  link = name: ref (output name);
  object = description: {
    kind = "OpenJson";
    inherit description;
  };
  row =
    name: description: fields:
    record (output name) "NoDecoder" description fields;
  tag = name: value: required name text "Exactly `${value}`.";
  run = required "runId" text "Bounded immutable run identity.";
  cursor =
    required "selection"
      (object "Native closed selection alternative described by observation-selection-* records.")
      "One fixed selection, including absence.";
  step = omitted "stepPath" text "Known checked root step path; omitted when unknown.";
  establishment =
    required "establishment" (object "Native closed observation-establishment-* alternative.")
      "Known bootstrap certainty.";
  checkpoint = kind: [
    (tag "kind" kind)
    (required "stage" (enum "enum ObservationStage") "Current observation phase.")
    cursor
    establishment
    step
  ];
in
{
  records = [
    (row "observation-context"
      "Checked native slot coordinates; construction performs no filesystem effects."
      [
        (required "projectId" text "Admitted project identity.")
        (required "environment" text "Admitted slot environment.")
        (required "slot" u32 "Declared slot number.")
        (required "stateBase" text "Effective absolute UTF-8 state base.")
        (required "stateRoot" text "Absolute UTF-8 application state root.")
        (required "registryDir" text "Absolute UTF-8 slot registry directory.")
        (required "registryPath" text "Absolute UTF-8 registry database path.")
      ]
    )
    (row "observation-source"
      "Historical source metadata without host admission; fingerprint data remains uninterpreted."
      [
        (required "codebaseId" text "Exactly main.")
        (required "logicalRoot" text "Confined logical source root.")
        (required "observedRoot" text "Recorded absolute UTF-8 source root.")
        (required "sourceMode" (enum "enum SourceMode") "Existing source-mode vocabulary.")
        (required "sourceIdentity" text "Recorded source identity.")
        (required "dirtyPolicy" (enum "enum DirtyPolicy") "Existing dirty-policy vocabulary.")
        (required "admissionFingerprintPolicy" text "Uninterpreted policy, including empty text.")
      ]
    )
    (row "observation-request"
      "Exact declared-request comparison metadata derived from actual full run admission."
      [
        (required "computedManifestHash" text "Lowercase SHA256 digest.")
        (required "rootTask" text "Declared root task identity.")
        (required "source" (link "observation-source") "All seven source fields.")
        (required "operationTimeoutMs" u64
          "Full-range operation timeout; independent of observation budget."
        )
      ]
    )
    (row "observation-leader" "Fresh exact leader sample retaining the expected recorded start token." [
      (required "pid" u32 "Positive PID no greater than i32::MAX.")
      (required "startIdentity" text "Nonempty recorded platform start identity.")
      (required "observation" (enum "enum LeaderObservation") "Fresh native sample; zombies are absent.")
    ])
    (row "observation-endpoint"
      "Recorded coordinates, without endpoint health or socket ownership claims."
      [
        (required "endpointId" text "Declared endpoint identity.")
        (required "host" text "Literal declared loopback host spelling.")
        (required "port" u16 "Nonzero planned port.")
      ]
    )
    (row "observation-service" "Recorded service identity and exact leader sample." [
      (required "serviceId" text "Declared service identity.")
      (required "processKey" text "Canonical service process key.")
      (required "leader" (link "observation-leader") "Exact start identity and PGID sample.")
      (required "endpoints" (list (
        link "observation-endpoint"
      )) "All endpoints sorted by endpoint ID; empty remains present.")
    ])
    (row "observation-progress-pending" "Execution has no recorded outcome." [
      (tag "state" "outcome-pending")
    ])
    (row "observation-progress-finalizing"
      "Known execution outcome with unfinished resource finalization."
      [
        (tag "state" "finalizing")
        (required "outcome" (enum "status ExecutionOutcome") "Recorded immutable outcome.")
      ]
    )
    (row "observation-progress-finalized" "Execution and resource finalization are recorded complete." [
      (tag "state" "finalized")
      (required "outcome" (enum "status ExecutionOutcome") "Recorded immutable outcome.")
    ])
    (row "observation-session"
      "One selected publication with checked historical metadata and current leader samples."
      [
        run
        (required "manifestPath" text "Recorded absolute UTF-8 manifest path.")
        (required "request" (link "observation-request") "Recorded declared request.")
        (required "progress" (object "Exactly one observation-progress-* alternative.")
          "Closed native progress sum."
        )
        (required "runDir" text "Derived absolute UTF-8 evidence directory.")
        (required "logsDir" text "Derived absolute UTF-8 log directory.")
        (required "owner" (link "observation-leader") "Fresh owner identity sample.")
        (required "services" (list (
          link "observation-service"
        )) "Services sorted by (serviceId, processKey); duplicate identities reject.")
      ]
    )
    (row "observation-completed-task"
      "Canonical successful root task occurrence with settled ownership and complete retained captures."
      [
        (required "stepPath" text "Exact checked root occurrence path.")
        (required "taskId" text "Leaf task identity derived from the checked plan.")
        (required "processKey" text "Canonical root process key.")
        (required "exitCode" i32 "Present policy-accepted exit, including accepted nonzero.")
        (required "stdoutPath" text "Absolute UTF-8 retained capture path.")
        (required "stderrPath" text "Absolute UTF-8 retained capture path.")
      ]
    )
    (row "task-observation"
      "Private native success proof constructed only after canonical reread and fresh leader checks."
      [
        (required "context" (link "observation-context") "Checked slot coordinates.")
        (required "session" (link "observation-session") "Selected executing session.")
        (required "task" (link "observation-completed-task") "Complete canonical root evidence.")
      ]
    )
    (row "observe-absent" "Read-only inspection selected absence once; exit zero." [
      (tag "result" "absent")
      (required "context" (link "observation-context") "Checked slot coordinates.")
    ])
    (row "observe-session" "Read-only inspection of the one selected session." [
      (tag "result" "session")
      (required "context" (link "observation-context") "Checked slot coordinates.")
      (required "session" (link "observation-session") "Selected session facts and samples.")
    ])
    (row "observe-satisfied" "Awaiting succeeded without retargeting or effects." [
      (tag "result" "satisfied")
      (required "observation" (link "task-observation") "Fresh checked success proof.")
    ])
    (row "ensure-json"
      "Full admission followed by shared establishment or observation of exactly one matching request."
      [
        (required "disposition" (enum "enum EnsureDisposition")
          "Whether this helper committed the selected run."
        )
        (required "observation" (link "task-observation") "Fresh checked success proof.")
      ]
    )
    (row "observation-selector-current" "Current selection selector." [ (tag "kind" "current") ])
    (row "observation-selector-run-id" "Exact bounded selector." [
      (tag "kind" "run-id")
      run
    ])
    (row "observation-selection-unenrolled" "No coherent enrollment yet." [
      (tag "state" "unenrolled")
    ])
    (row "observation-selection-absent" "Absence selected once." [
      (tag "state" "absent")
      (required "selector" (object "Closed observation-selector-* alternative.") "The original selector.")
    ])
    (row "observation-selection-seeking" "Frozen pre-publication enrollment." [
      (tag "state" "seeking")
      (required "watermark" i64 "Nonnegative original native event high-water mark.")
    ])
    (row "observation-selection-pinned" "One immutable selected run." [
      (tag "state" "pinned")
      run
    ])
    (row "observation-establishment-not-requested" "No helper was requested." [
      (tag "state" "not-requested")
    ])
    (row "observation-establishment-not-established" "Candidate helper did not establish a run." [
      (tag "state" "not-established")
      (required "candidateRunId" text "Bounded candidate identity.")
    ])
    (row "observation-establishment-uncertain"
      "Complete dispatch without a conclusive establishment reply."
      [
        (tag "state" "uncertain")
        (required "candidateRunId" text "Bounded candidate identity.")
      ]
    )
    (row "observation-establishment-established" "A conclusive run commit reply was received." [
      (tag "state" "established")
      run
    ])
    (row "observation-error-slot-busy" "Typed details.sessionObservation for strict slot contention." [
      (tag "kind" "slot-busy")
      (required "context" (link "observation-context") "Checked slot coordinates.")
    ])
    (row "observation-error-request-mismatch"
      "Exact request comparison rejection in manifest/root/source/timeout order."
      [
        (tag "kind" "request-mismatch")
        (required "field" (enum "enum RequestMismatchField") "First differing declaration field.")
        cursor
      ]
    )
    (row "observation-error-timeout" "Observer-local timeout; never cancels a committed owner." (
      checkpoint "timeout"
    ))
    (row "observation-error-canceled" "Observer signal wins over expiry or reply." (
      checkpoint "canceled"
    ))
    (row "observation-error-bootstrap-uncertain"
      "EOF or malformed complete-dispatch reply; no owner cancellation."
      [
        (tag "kind" "bootstrap-uncertain")
        cursor
        establishment
      ]
    )
    (row "observation-evidence" "Safe retained task locators, without captured data." [
      (required "stdoutPath" text "Checked absolute capture path.")
      (required "stderrPath" text "Checked absolute capture path.")
    ])
    (row "observation-error-unavailable"
      "Selected session cannot satisfy the wait; no retarget or restart."
      [
        (tag "kind" "unavailable")
        (required "reason" (enum "enum SessionUnavailableReason")
          "Checked failure reason in native precedence."
        )
        cursor
        step
        (omitted "evidence" (link "observation-evidence") "Known canonical task locators.")
      ]
    )
  ];
  vocabularies = [
    (vocabulary "enum LeaderObservation" "LeaderObservation" (owner "observation")
      "Exact expected native leader identity sample."
    )
    (vocabulary "enum EnsureDisposition" "EnsureDisposition" (owner "observation")
      "Established or observed selected run."
    )
    (vocabulary "enum ObservationStage" "ObservationStage" (owner "error") "Current observer phase.")
    (vocabulary "enum RequestMismatchField" "RequestMismatchField" (owner "error")
      "First exact-request mismatch."
    )
    (vocabulary "enum SessionUnavailableReason" "SessionUnavailableReason" (owner "error")
      "Closed checked observation failure reasons."
    )
  ];
}
