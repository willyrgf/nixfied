# RFC: task output projection

- Status: accepted design; implementation pending
- Date: 2026-08-03
- Scope: runtime output, generated task applications, and adopter integration
- Implementation plan: `IMPL_PLAN_RFC_TASK_OUTPUT.md`

## Summary

Nixfied captures every task's stdout and stderr into runtime-owned, redacted
evidence files. The runtime currently exposes those files as metadata but does
not provide a supported way for an adopter to receive a directly selected
leaf task's captured bytes through the generated application's corresponding
output streams.

This RFC adds one opt-in runtime mode:

```text
--output task-output
```

The mode applies only to one explicitly selected direct leaf task. After the
task reaches a captured terminal outcome, the runtime replays its already
redacted stdout and stderr files byte-for-byte to the corresponding process
streams. Replay occurs for success, failure, timeout, and cancellation. It is
not gated by `exitPolicy.successCodes`.

The existing `summary`, `json`, and `both` modes remain unchanged. There are
no model fields, graph changes, compatibility aliases, legacy readers, or
adopter-side log-path protocols.

## Normative output contract

### Output modes

| Mode | stdout | stderr | status |
| --- | --- | --- | --- |
| `summary` | empty | existing human progress, result, and evidence | existing run status |
| `json` | existing structured `RunOutput` JSON | existing structured errors | existing run status |
| `both` | existing structured `RunOutput` JSON | existing human diagnostics and JSON errors | existing run status |
| `task-output` | exact redacted stdout of the selected leaf | human diagnostics plus exact redacted stderr replay | task/run/projection status |

`task-output` MUST NOT emit runtime JSON metadata to stdout. Runtime errors are
reported on stderr. The selected task's stderr is inserted into the runtime's
stderr stream at the replay point; runtime diagnostics before and after replay
remain runtime-owned bytes and are not part of the task's exact stderr byte
sequence.

The task's exit status remains authoritative. Callers MUST check the process
status before treating replayed stdout as a valid application result.

### Selection

`task-output` MUST have exactly one explicit `--task` argument. The argument
MUST name a directly declared leaf in `ExecutionModel.tasks`.

The following are invalid under `task-output`:

- no `--task`;
- an unknown task;
- a repeated `--task`;
- a composite task; and
- an admitted task plan that defensively flattens to no executable leaf.

Composite tasks remain valid under `summary`, `json`, and `both`. The runtime
MUST reject a composite `task-output` selection before slot selection,
placement, registry/state materialization, lease acquisition, service
startup, prepare execution, or child spawn. The rejection MUST NOT be
reported as `MODEL_ADMISSION`.

A service-backed direct leaf is valid. Output from services and service
prepare tasks is not replayed; only the direct execution of the explicitly
selected leaf is replayed. A prepare reference is not a new task kind: a
top-level leaf used as a prepare reference remains selectable, while an
execution of that task as a prepare node is not the direct selected execution.

### Replay lifecycle

The runtime MUST implement this lifecycle for a selected task:

1. Parse output syntax and flag combinations.
2. Load and admit the model.
3. Validate the selected task and `task-output` eligibility.
4. Select the slot and derive placement.
5. Materialize, open, reconcile, and upgrade runtime state.
6. Record the run.
7. Start services and execute prepare tasks.
8. Execute the selected task using the existing capture and redaction path.
9. Wait for process containment/reconciliation and join both redaction relays.
10. Construct typed `TaskRun` evidence and open the selected redacted log
    files before task-terminal registry updates or cleanup can remove their
    directory entries.
11. Write the task summary and mark the task terminal, collecting errors
    without discarding the replay ticket.
12. Replay selected stdout and stderr concurrently and wait for both replay
    workers.
13. Teardown services.
14. Finalize the run registry and stop/release the run lease.
15. Write the aggregate summary.
16. Write the run footer.
17. Project the final runtime error, if any, after the replay barrier.

Replay MUST occur after capture is complete and before teardown, lease release,
aggregate summary output, the run footer, or the final runtime error. A replay
failure MUST NOT skip teardown, registry finalization, lease release, or
summary finalization.

If task summary writing, registry finalization, service teardown, lease stop,
aggregate summary writing, or a later cancellation fails after the task has
finished, the runtime MUST retain the selected task's replay ticket, complete
the replay barrier, perform all remaining cleanup/finalization, and report all
relevant causes using the documented precedence.

If admission, service startup, prepare, dependency resolution, spawn,
capture, or redaction fails before a terminal selected-task outcome exists,
there is no replay ticket and stdout remains empty. A task that completes with
empty logs legitimately produces zero replay bytes.

A terminal task outcome is immutable. Cancellation of a child before its
terminal outcome produces the existing `CANCELED` result and still replays any
captured output. Cancellation observed after a task has become terminal cannot
rewrite that task as canceled. If a successful task has not yet committed the
run terminal state, a later cancellation may make the run canceled; it does
not cause a second replay.

### Typed ownership

Post-capture evidence MUST remain typed until it reaches a registry, summary,
or public error serialization boundary. The implementation should use a
move-only replay ticket and typed success/error variants equivalent to:

```text
ReplayTicket = { stdout: ReplaySource, stderr: ReplaySource }
ReplaySource = Ready(File) | OpenFailed(ProjectionIssue)
CompletedEvidence = Captured(TaskRun) | Replayable(TaskRun, ReplayTicket)
TaskExecutionError = BeforeTerminal(RuntimeError)
                    | AfterTerminal(RuntimeError, CompletedEvidence)
ReplayPlan = None | Selected(ReplayTicket)
```

The concrete internal API should be shaped as follows; names may be adjusted
during implementation, but the ownership and result alternatives are part of
the design:

```rust
pub(crate) enum EvidenceMode {
    CaptureOnly,
    ReplaySelected,
}

pub(crate) struct ReplayTicket {
    stdout: ReplaySource,
    stderr: ReplaySource,
}

impl ReplayTicket {
    pub(crate) fn open(stdout: &Path, stderr: &Path) -> Self;

    pub(crate) fn replay(self, sinks: ReplaySinks) -> ReplayReport;
}

pub(crate) enum ReplaySource {
    Ready(File),
    OpenFailed(ProjectionIssue),
}

pub(crate) enum CompletedEvidence {
    Captured(TaskRun),
    Replayable { task: TaskRun, ticket: ReplayTicket },
}

pub(crate) enum TaskExecution {
    Succeeded(CompletedEvidence),
    Failed { error: RuntimeError, evidence: CompletedEvidence },
}

pub(crate) enum TaskExecutionError {
    BeforeTerminal(RuntimeError),
    AfterTerminal { error: RuntimeError, evidence: CompletedEvidence },
}

pub(crate) fn run_dependent_task_cancellable(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    dependencies: &[&StartedService],
    node_id: &str,
    task: &ExecTask,
    cancellation: &CancellationToken,
    evidence: EvidenceMode,
) -> Result<TaskExecution, TaskExecutionError>;
```

`ReplayTicket::open` opens each stream independently so one open failure does
not prevent the other stream from being replayed. `ReplayTicket::replay(self)`
consumes the ticket; there is no clone or second-replay operation.

For a selected direct leaf, `run_dependent_task_cancellable` must perform the
following ownership transfer:

```text
child exits
→ redaction relays join
→ TaskRun is constructed
→ ReplayTicket::open, when EvidenceMode::ReplaySelected
→ task summary is written
→ registry task-terminal update is attempted
→ TaskExecution is returned with typed evidence
```

An expected task outcome such as `TASK_FAILED`, timeout, or `CANCELED` is an
`Ok(TaskExecution::Failed { ... })` result containing evidence. A summary or
registry failure after capture is an
`Err(TaskExecutionError::AfterTerminal { ... })` result containing the same
evidence. Spawn, capture, redaction, and other pre-terminal failures use
`BeforeTerminal` and have no replay ticket.

The finalizer should have one equivalent entry point:

```rust
pub(crate) fn finalize_run(
    session: RunSession,
    outcome: RunOutcome,
) -> Result<RunOutput, RuntimeError> {
    let mut failures = FailureAccumulator::from(outcome);

    let replay = session.replay.replay(session.output_sinks());
    failures.extend(replay.into_errors());

    failures.extend(session.teardown_services());
    failures.extend(session.finalize_registry());
    failures.extend(session.stop_lease());
    failures.extend(session.write_aggregate_summary());
    failures.extend(session.write_footer());

    failures.finish()
}
```

This is lifecycle pseudocode, not a demand for these exact Rust names. The
important properties are that replay happens before teardown, every later
stage runs after replay, and no `?` returns before the ticket is consumed.

The runtime MUST NOT reconstruct task evidence from
`RuntimeError.details["taskRun"]`. The existing `TaskRun` wire structure is
unchanged; it may be serialized for public projections, but that serialization
is never parsed back internally.

For a direct leaf, the plan contains one root node. The runtime must populate
`RunOutput.task` from that selected node rather than relying on an aggregate
`last()` rule. Existing composite `RunOutput` behavior remains unchanged.

### Byte semantics and concurrency

Replay MUST use one bounded-memory worker for stdout and one for stderr. Each
worker MUST read fixed-size chunks no larger than 64 KiB, preserve binary bytes
and missing final newlines, handle partial/interrupted writes, count committed
bytes, and convert broken pipes and other I/O failures into typed projection
errors.

One blocked or failed destination MUST NOT stop the other stream. Per-stream
byte order is guaranteed. Cross-stream interleaving is unspecified and MUST
NOT be relied upon.

The runtime MUST handle `SIGPIPE` so a closed downstream pipe becomes a
`BrokenPipe` result rather than process termination or a panic. Task-output
diagnostics and footer writes must also be fallible; a closed stderr sink MUST
not trigger an `eprintln!` panic.

The implementation is required for the runtime's supported Linux and macOS
targets and should use portable file/read/write/thread primitives rather than
platform-specific zero-copy behavior.

## Error contract

The existing exit-code mapping remains unchanged through exit 34. Add these
codes without renumbering existing values:

| ErrorCode | JSON code | exit | meaning |
| --- | --- | ---: | --- |
| `OutputModeInvalid` | `OUTPUT_MODE_INVALID` | 35 | unknown mode, missing value, or prohibited spelling |
| `OutputModeConflict` | `OUTPUT_MODE_CONFLICT` | 36 | `task-output` combined with a metadata projection |
| `TaskSelectionInvalid` | `TASK_SELECTION_INVALID` | 37 | missing, repeated, unknown, ambiguous, or composite selection |
| `OutputProjectionFailed` | `OUTPUT_PROJECTION_FAILED` | 38 | selected-log read, stdout/stderr write, flush, or broken-pipe failure |

### Compound errors

The public `RuntimeError` JSON projection gains an optional top-level `causes`
array, omitted when empty. Each cause contains `code`, `exitClass`, `message`,
and redaction-safe `details`. Projection failures use a
`details.projections[]` array with `stream`, `operation`, `kind`, `path`, and
`bytesWritten`. Captured bytes and secrets MUST never be included.

Task failure plus replay failure has `OUTPUT_PROJECTION_FAILED` as the primary
error and `TASK_FAILED` as a cause. Timeout plus replay failure has the same
primary with a `TASK_FAILED` cause whose `taskRun.timedOut` is true.
Cancellation plus replay failure has the same primary with `CANCELED` as a
cause. A successful task whose output cannot be replayed exits 38.

The JSON projection is equivalent to:

```json
{
  "code": "OUTPUT_PROJECTION_FAILED",
  "exitClass": "error",
  "message": "selected task output replay failed",
  "details": {
    "projections": [
      {
        "stream": "stdout",
        "operation": "write",
        "kind": "broken-pipe",
        "path": "...",
        "bytesWritten": 4096
      }
    ]
  },
  "causes": [
    {
      "code": "TASK_FAILED",
      "exitClass": "error",
      "message": "task smoke exited with code 7",
      "details": { "taskRun": {} }
    }
  ]
}
```

The human projection is equivalent to:

```text
error: OUTPUT_PROJECTION_FAILED: selected task output replay failed
  cause: TASK_FAILED: task smoke exited with code 7
  projection: stdout write failed (broken-pipe)
```

Precedence is:

1. containment, registry, lease, ownership, and state-integrity failures;
2. `OUTPUT_PROJECTION_FAILED`;
3. task execution outcomes such as `TASK_FAILED` and `CANCELED`; and
4. successful completion.

Summary, registry, teardown, lease, and late-cancellation errors are never
silently discarded. A later safety failure may become the primary error over a
projection failure, with the projection failure preserved as a cause.

No post-admission failure may be reported as `MODEL_ADMISSION`. Model parsing,
ABI, and admission failures retain their existing classifications. Selection
errors use `TASK_SELECTION_INVALID`; post-admission serialization or lifecycle
failures use `LIFECYCLE_FAILED` or the more specific registry/state error.

## CLI parser contract

`task-output` is an exact `--output` value. There is no `--task-output` flag and
no spelling alias such as `task_output`, `taskOutput`, or `forward`.

| invocation | result |
| --- | --- |
| `--output task-output --json` | `OUTPUT_MODE_CONFLICT`, 36 |
| `--json --output task-output` | `OUTPUT_MODE_CONFLICT`, 36 |
| `--output task-output --summary` | `OUTPUT_MODE_CONFLICT`, 36 |
| `--both --output task-output` | `OUTPUT_MODE_CONFLICT`, 36 |
| repeated metadata `--output` values | preserve existing last-value-wins behavior |
| `--output task-output --output task-output` | accepted and idempotent |
| `--json --output json` | remains valid |
| repeated `--task` | `TASK_SELECTION_INVALID`, 37 |
| invalid mode or alias | `OUTPUT_MODE_INVALID`, 35 |

Any `task-output` occurrence combined with any metadata projection conflicts,
regardless of order. Existing valid metadata-only invocations retain their
last-value-wins behavior. The existing `--summary`, `--json`, and `--both`
spellings remain supported for those modes only.

Parser diagnostics are selected from the raw flags when parsing fails:

- a conflict containing `--json` emits JSON on stderr;
- a conflict containing `--both` emits human and JSON diagnostics; and
- otherwise the diagnostic is human-readable on stderr.

Help MUST advertise `summary`, `json`, `both`, and `task-output`, explain the
direct-leaf requirement, and omit all prohibited aliases.

## Power Sim integration

Power Sim should replace its JSON/path extraction helper in
`../power-sim/flake.nix` with a direct invocation of the generated task:

```sh
forward_task_stdout() {
  exec ${generated.simulate.program} --output task-output
}
```

The profile/request-builder behavior remains in Power Sim. Its JSON request can
continue to flow through stdin to the generated task. The wrapper no longer
parses runtime JSON or reopens Nixfied evidence.

The Python request-builder's own invalid-input status remains 2. At the
exported Nixfied app boundary, a child exit 2 is classified by the existing
default task policy as `TASK_FAILED`, exit 30. Successful simulation, `--help`,
and `--list-profiles` remain exit 0. These two layers must be tested
separately.

## Contract and ABI impact

This is a public runtime contract change and follows ABI-1.

Required ABI updates:

- `runtime/crates/nixfied-model/capability.txt`: add `task-output`, the four
  error codes, and the output/cause vocabulary;
- `runtime/crates/nixfied-model/src/constants.rs`: update the Rust capability
  digest snapshot;
- `nix/spec/constants.nix`: verify Nix derives the same digest; no textual
  change is expected if it only derives the descriptor hash;
- `runtime/crates/nixfied-runtime/src/error.rs`: update the exhaustive error
  inventory and exit mapping; and
- capability/error tests and generated fixtures as described by the
  implementation plan.

The capability digest rotates even though no `model.json` field or graph
derivation changes. Old models are rejected by the existing ABI identity check
and must be regenerated. No compatibility reader, alias, migration, or dual
path is added.

No changes are expected in `nix/compiler/`, `nix/modules/`,
`docs/DERIVATION_SPEC.md`, `docs/OPTIONS.md`, or model structs.

## Rejected alternatives

- adopter-side metadata/path extraction;
- forwarding every generated app by default;
- a model-level output policy;
- gating replay on `successCodes`;
- inline task bytes in `RunOutput` JSON;
- live teeing from the child;
- concatenating composite node output;
- last-value-wins when `task-output` is combined with metadata;
- recovering evidence from serialized `RuntimeError.details`;
- replaying after teardown or cleanup; and
- adding spelling aliases or a legacy compatibility path.

These alternatives either weaken redaction and ownership, make arbitrary
binary output ambiguous, change existing generated-app behavior, add model
surface for presentation semantics, or make lifecycle/error behavior
unverifiable.

## Acceptance criteria

The implementation is complete only when it proves:

- direct leaf success with exact text, binary, empty, and no-newline output;
- accepted nonzero success codes with exit 0 and replayed output;
- task failure, timeout, and cancellation with replay and existing statuses;
- service and spawn failure with empty stdout;
- redaction preservation;
- bounded large-output replay and simultaneous streams;
- read failure, write failure, partial writes, and downstream broken pipe;
- exactly-once replay and finalization after replay failure;
- composite rejection before state or child side effects;
- unchanged summary/json/both behavior;
- generated-app flag forwarding;
- Power Sim exact output and layered exit statuses; and
- no post-admission `MODEL_ADMISSION` projection.

The concrete source map, test files, implementation commits, verification
commands, and go/no-go checklist are in `IMPL_PLAN_RFC_TASK_OUTPUT.md`.
