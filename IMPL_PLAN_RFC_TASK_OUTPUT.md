# Implementation plan: RFC task output projection

- RFC: `RFC_TASK_OUTPUT.md`
- Status: ready for engineering implementation planning
- Date: 2026-08-03
- Scope: Nixfied runtime, runtime ABI, generated-app acceptance, and Power Sim migration

This document turns the accepted task-output contract into a dependency-ordered implementation plan. It names the current call sites, ownership boundaries, exact public errors, tests, files, commits, and verification gates.

No code is implemented by this plan.

## 1. Design invariants

The implementation must preserve these repository-wide rules:

- `model.json` remains the only semantic seam;
- Nix continues to evaluate, validate, build, and realise;
- Rust continues to admit, execute, reconcile, and clean;
- captured output is redacted before persistent write;
- the runtime replays only redacted evidence files;
- no host-absolute placement or secret values enter the model;
- shell/adopter code does not own graph, registry, validation, liveness, cleanup, or runtime evidence paths;
- `summary`, `json`, and `both` preserve their current valid behavior;
- `exitPolicy.successCodes` determines status only;
- no post-admission failure is projected as `MODEL_ADMISSION`;
- no serialized JSON projection is used as an internal evidence store; and
- no aliases, compatibility shims, legacy readers, migrations, or dual paths are introduced.

The ABI changes because the public runtime output/error vocabulary changes, but the model has no new field and the task graph has no new edge or node kind.

## 2. Current source trace

### `run_m0`

`runtime/crates/nixfied-runtime/src/main.rs` currently:

1. handles help;
2. parses run flags;
3. installs process signal handlers;
4. loads and admits the model;
5. creates the redactor;
6. calls `run_m0_admitted`; and
7. lets `main` project any returned error.

The new parser errors must be formed before model loading. The final runtime error must still be projected only by the top-level error path, after the replay/finalization barrier.

### `run_m0_admitted`

The current function checks cancellation, selects a slot, derives placement, and calls `run_m0_placed`. Move semantic task selection validation here, immediately after admission and before slot selection or placement.

The required order is:

```text
parse mode syntax
→ admit model
→ validate task/output selection
→ check cancellation
→ select slot
→ derive placement
→ run_m0_placed
```

Admission may read and validate model/store/source/secret metadata. It must not materialize state, open or reconcile the registry, acquire a lease, reserve a service, execute prepare, or spawn a child before a composite task-output selection is rejected.

### `run_m0_placed`

The current function materializes the registry root, opens/reconciles the registry, upgrades slot state, selects the task, computes the plan, records the run, starts services and prepare tasks, executes nodes, writes summaries, tears down services, stops the lease, and writes the aggregate summary/footer.

It has several early-return branches for service startup, readiness, cancellation, task failure, stop failure, and lease failure. Those branches must be consolidated behind one run-finalization owner. No branch may return before transferring or consuming the selected replay ticket.

The current failure path also recovers a `TaskRun` from `error.details["taskRun"]`. Delete that recovery. The task runner must return typed evidence directly.

### `run_dependent_task_cancellable`

The current function:

1. checks cancellation and dependencies;
2. derives log paths and substitutions;
3. spawns the child using `child_output`;
4. records the process;
5. waits for the child;
6. joins redaction relays;
7. calculates success from cancellation, timeout, exit code, and `success_codes`;
8. creates `TaskRun`;
9. writes the task summary;
10. serializes `TaskRun` for the registry payload;
11. marks the task terminal; and
12. returns `TaskRun` on success or puts it in JSON error details on failure.

Change the return path so the typed `TaskRun` survives both successful and failed execution. For the directly selected task, open both replay sources before task-terminal registry updates. A failure of summary writing or registry marking must retain that typed evidence and ticket while being reported to the run finalizer.

Redaction relay failure remains fail-closed: if the relay did not produce a complete trusted redacted file, do not replay the incomplete evidence. Return the existing `SECRET_LEAK_BLOCKED` classification with no replay ticket.

### Prepare and service lifecycle

`PrepareRunner` currently returns `RuntimeResult<()>`, and `start_service_for_slot` consumes that result. Propagate the internal typed task outcome through the prepare boundary so prepare failures do not require JSON extraction. Prepare evidence is retained for summaries/errors but is not selected for replay.

`teardown` currently returns `()` and ignores service stop/cancel failures. Change it to return collected typed failures. `RunLeaseHeartbeat::stop` already returns a failure; callers must append it instead of using `?` in a way that replaces the primary outcome.

`write_failure_run_summary` currently suppresses write errors with `.ok()`. Replace that suppression with error collection.

## 3. Runtime ownership model

### New internal output module

Add `runtime/crates/nixfied-runtime/src/output.rs` and register it in `src/lib.rs`. Keep replay and projection details out of the model crate; this is runtime behavior.

The module should own:

- `ReplayTicket` and `ReplaySource`;
- bounded file-to-stream copying;
- per-stream worker joins;
- partial-write and interrupted-write handling;
- broken-pipe classification;
- redaction-safe `ProjectionIssue` values;
- output-projection cause aggregation; and
- test injection points for read/write faults.

The selected task must produce a move-only `ReplayTicket` containing one source for stdout and one for stderr. A source may hold an open file or a typed open/read failure. This allows stderr to replay even if stdout cannot be opened, and vice versa.

The finalizer owns `ReplayPlan = None | Selected(ReplayTicket)`. `Selected` is consumed exactly once. `None` is used for metadata modes, composite execution, prepare nodes, and execution paths without a terminal selected-task outcome.

### Opening and retention

Open both redacted log files immediately after the child capture is complete and before `mark_task_finished`, run-terminal transitions, lease release, or cleanup can remove their directory entries. Holding the file descriptors keeps the evidence readable on supported Unix systems even if a path is unlinked.

Do not reopen a path later from `TaskRun`, `RunOutput`, or serialized error details.

### Finalization owner

Introduce one internal run-session/finalizer owner in `main.rs` or a narrowly scoped runtime module. It must own:

- started services;
- the mutable registry;
- the optional run lease;
- collected node/task evidence;
- the selected replay plan; and
- accumulated finalization failures.

The owner must run all cleanup/finalization stages even after an earlier stage fails. It must wait for both replay workers before any final error is returned.

The finalizer's result is either the existing `RunOutput` or one merged `RuntimeError` with a primary error and typed causes. It must not print the final error itself; `main` remains the one public error projection point.

### Concrete API sketch

The following is the implementation-facing shape. Exact Rust names may be
refined, but the variants and ownership rules must remain:

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

`ReplayTicket::open` opens stdout and stderr independently. A read/open error
for one stream is retained in that stream's `ReplaySource` so the other stream
can continue. `ReplayTicket::replay(self)` consumes the ticket and is the only
replay entry point.

The task function's ownership sequence is:

```text
child exits
→ redaction relays join
→ construct TaskRun
→ open ReplayTicket when ReplaySelected
→ write task summary
→ attempt registry task-terminal update
→ return typed TaskExecution
```

An expected task result (`TASK_FAILED`, timeout, or `CANCELED`) is
`Ok(TaskExecution::Failed { ... })` with typed evidence. A summary or registry
failure after capture is
`Err(TaskExecutionError::AfterTerminal { ... })` with that same evidence.
Spawn, capture, and redaction failures use `BeforeTerminal` and have no replay
ticket.

The run finalizer should have one equivalent entry point:

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

This is an ownership/lifecycle sketch, not a requirement for these exact
identifiers. It is a requirement that replay is consumed before teardown and
that every later finalization stage runs without an early `?` return.

## 4. Public error implementation

### ErrorCode additions

Add these variants and explicit exit mappings in `runtime/crates/nixfied-runtime/src/error.rs`:

| Rust variant | wire code | exit |
|---|---|---:|
| `OutputModeInvalid` | `OUTPUT_MODE_INVALID` | 35 |
| `OutputModeConflict` | `OUTPUT_MODE_CONFLICT` | 36 |
| `TaskSelectionInvalid` | `TASK_SELECTION_INVALID` | 37 |
| `OutputProjectionFailed` | `OUTPUT_PROJECTION_FAILED` | 38 |

Keep all existing values 12 through 34 unchanged. Update the exhaustive code sentinel and capability-token tests.

### Cause shape

Add an optional serialized `causes` array to `RuntimeError`, omitted when empty. Each non-recursive cause contains `code`, `exitClass`, `message`, and redaction-safe `details`.

Projection details should be typed internally and serialized as `details.projections[]` entries containing `stream`, `operation`, `kind`, `path`, and `bytesWritten`.

Never include captured bytes, secret values, or raw unredacted OS messages.

The public compound projection should be equivalent to:

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

The human projection should be equivalent to:

```text
error: OUTPUT_PROJECTION_FAILED: selected task output replay failed
  cause: TASK_FAILED: task smoke exited with code 7
  projection: stdout write failed (broken-pipe)
```

### Precedence

Use this order when multiple post-task errors exist:

1. containment, registry, lease, ownership, and state-integrity failures;
2. `OUTPUT_PROJECTION_FAILED`;
3. task execution outcomes (`TASK_FAILED`, `CANCELED`, dependency failure, and similar); and
4. successful completion.

A task failure plus replay failure therefore returns 38 with `TASK_FAILED` as a cause. Timeout remains `TASK_FAILED` 30 with `timedOut: true`; cancellation remains `CANCELED` 27. A successful task plus replay failure returns 38.

Summary, registry, teardown, lease, and late-cancellation errors are appended and never silently discarded. A later safety failure may become the primary error over a projection failure.

Audit every `ModelAdmission` construction reachable after `Admission::admit`. Replace post-admission selection, serialization, and lifecycle misclassifications with `TaskSelectionInvalid`, `LifecycleFailed`, or the specific registry/state error.

## 5. Parser implementation

### `RunOutputMode`

Add `TaskOutput` to `RunOutputMode`.

`emit_summary()` should remain true for `TaskOutput`, because human runtime diagnostics and the run footer belong on stderr. `emit_json()` must remain false for `TaskOutput`.

Keep the current metadata-only last-value-wins behavior. Track whether `task-output` and any metadata projection have appeared so order cannot hide a conflict.

Required behavior:

| input | result |
|---|---|
| `--output task-output --json` | 36 |
| `--json --output task-output` | 36 |
| `--output task-output --summary` | 36 |
| `--both --output task-output` | 36 |
| `--output summary --output json` | existing last-value-wins behavior |
| `--output task-output --output task-output` | valid |
| repeated `--task` | 37 |
| `--task-output` or alternate token | 35 |
| missing/unknown output token | 35 |

`error_output_projection` currently parses flags lossily. Replace it with a raw-argument diagnostic scan that follows the same JSON/human/both rule even when normal option parsing fails:

- any `--both` conflict: human plus JSON;
- otherwise any `--json` conflict: JSON only; and
- otherwise: human only.

No valid task-output path emits JSON metadata to stdout.

### Help

Update runtime help in `main.rs` to list:

```text
--output <mode>         Select summary, json, both, or task-output
```

Add a short explanation that task-output requires one directly selected leaf and replays redacted output. Do not list `--task-output` or any alias.

## 6. Selection implementation

Add a selection-validation function called after admission and before slot selection. It should inspect the already separated execution maps:

```text
ExecutionModel.tasks       → direct leaves
ExecutionModel.composites  → composite DAGs
```

Rules:

- task-output with no explicit task: 37;
- unknown task: 37;
- repeated task: 37 during parsing;
- task-output with a leaf: accepted;
- task-output with a composite: 37;
- admitted empty composite/zero-node defensive case: 37;
- summary/json/both composite behavior: unchanged.

Model admission errors retain precedence over selection errors because an unadmitted model cannot safely be inspected. Once admission succeeds, no selection error may be emitted as `MODEL_ADMISSION`.

The selected task identity must remain separate from aggregate `task_runs`. For task-output, the selected direct plan node is the only candidate that may receive a replay ticket.

## 7. Replay implementation details

Use two worker threads, one per stream, with 32 KiB or 64 KiB buffers. The workers must:

- read from already opened redacted files;
- write to the corresponding process stream;
- handle partial writes in a loop;
- retry `Interrupted` where safe;
- stop only their own stream on failure;
- return a typed issue with stream, operation, kind, path, and byte count; and
- always be joined by the finalizer.

Install `SIGPIPE` handling in the existing signal guard or an output-specific guard. Restore the previous disposition on teardown. The runtime supports Linux and macOS only; use the existing platform abstraction and do not add a Windows path.

Runtime progress/footer/error writes that occur under task-output must be fallible. A closed stderr destination must not panic through `eprintln!`.

The tests must assert per-stream ordering only. Cross-stream interleaving is explicitly unspecified.

## 8. Exact file plan

### Required runtime edits

- `runtime/crates/nixfied-runtime/src/main.rs` — parser, mode, early selection, run finalizer, error projection, help;
- `runtime/crates/nixfied-runtime/src/error.rs` — four codes, exit mapping, causes, exhaustive tests;
- `runtime/crates/nixfied-runtime/src/service/task.rs` — typed task outcomes and replay-ticket creation;
- `runtime/crates/nixfied-runtime/src/service/process.rs` — typed prepare propagation and returned teardown/lifecycle failures;
- `runtime/crates/nixfied-runtime/src/service/mod.rs` — internal type exports as needed;
- `runtime/crates/nixfied-runtime/src/cancellation.rs` — SIGPIPE-safe guard if signal ownership is placed there;
- `runtime/crates/nixfied-runtime/src/lib.rs` — register the new output module;
- `runtime/crates/nixfied-runtime/src/output.rs` — new bounded replay and projection implementation; and
- `runtime/crates/nixfied-test-child/src/main.rs` — deterministic large, binary, no-newline, and simultaneous-output cases.

### Required runtime test edits

- new `runtime/crates/nixfied-runtime/tests/output.rs`;
- `runtime/crates/nixfied-runtime/tests/service.rs`;
- `runtime/crates/nixfied-runtime/tests/command_help.rs`;
- `runtime/crates/nixfied-runtime/tests/common/mod.rs`;
- unit tests in `src/output.rs` and `src/error.rs`;
- `runtime/crates/nixfied-runtime/tests/capability_coverage.rs` if explicit runtime projection tokens are added; and
- `runtime/crates/nixfied-model/src/constants.rs` for the ABI snapshot.

### ABI and Nix edits

- `runtime/crates/nixfied-model/capability.txt`;
- `runtime/crates/nixfied-model/src/constants.rs`;
- `nix/spec/constants.nix` verification of derived digest;
- `nix/gate-runtime/nixfied.nix`; and
- `nix/gate-nix.nix`.

`nix/project-apps.nix` should be inspected but requires no production change: its generated task apps already forward `"$@"`. The gate must prove that forwarding remains intact.

### Documentation edits

- `RFC_TASK_OUTPUT.md`;
- `docs/CONTRACT.md`;
- `docs/ARCHITECTURE.md`;
- `docs/GUIDE.md`; and
- `docs/DEVELOPMENT.md`.

No edits are expected in `nix/compiler/`, `nix/modules/`, `docs/DERIVATION_SPEC.md`, `docs/OPTIONS.md`, model structs, or graph golden fixtures.

### Power Sim migration files

In the later adopter commit:

- `../power-sim/flake.nix`;
- `../power-sim/README.md`; and
- a Power Sim integration/acceptance test, preferably under `../power-sim/tests/integration/`.

`../power-sim/nixfied.nix` should not need a model change: the simulate task already inherits stdin and uses the existing task boundary.

## 9. Test matrix

### Focused runtime tests

Add a dedicated output integration suite covering:

| criterion | required assertion |
|---|---|
| direct leaf success | exact stdout/stderr bytes and exit 0 |
| accepted nonzero code | exact replay and exit 0 |
| task failure | replay occurs, exit 30, typed `TASK_FAILED` |
| timeout | replay occurs, exit 30, `timedOut: true` |
| cancellation | replay occurs, exit 27, `canceled: true` |
| service failure | stdout empty, existing service error |
| spawn failure | stdout empty, existing spawn/lifecycle error |
| redaction | secret absent from file and replay |
| binary/no newline | exact byte equality |
| empty output | zero replay bytes, successful status |
| large output | bounded copy and exact per-stream bytes |
| simultaneous streams | both complete; no cross-stream order assertion |
| read failure | exit 38, stream issue, original outcome cause |
| write failure | exit 38, partial byte count, no panic |
| broken pipe | exit 38 with `broken-pipe`, other stream continues |
| composite | exit 37 before child/state side effects |
| no selection | exit 37 before placement/state effects |
| unknown selection | exit 37 and no late `MODEL_ADMISSION` |
| repeated task | exit 37 |
| mode conflict | exit 36 in either flag order |
| invalid alias | exit 35 |
| failure plus replay failure | projection primary, task cause |
| cancellation plus replay failure | projection primary, cancellation cause |
| late teardown/lease failure | replay and cleanup still complete |

Fault injection for read/write failures belongs in `output.rs` unit tests or a small injectable sink/source abstraction. Do not add production-only failure hooks merely to make integration tests pass.

Use parent drain threads for large-output and pipe tests so the test harness, rather than the runtime contract, does not introduce backpressure deadlocks.

### Existing runtime suites

Update `service.rs` to preserve exact existing assertions for:

- summary/json/both success;
- summary/json/both task failure;
- cancellation and timeout;
- service failure before any task node;
- composite evidence and step paths; and
- registry terminal statuses and summary paths.

The failure path assertions should additionally prove that the failed `TaskRun` is available from typed internal evidence and remains present in the public projection without JSON round-tripping.

Update `command_help.rs` to assert:

- `task-output` is listed;
- `--task-output` is not listed;
- alternate spellings are rejected; and
- help still works before model/state admission.

### Nix gates

In `nix/gate-runtime/nixfied.nix`, add first-class acceptance cases for:

- direct leaf task-output exact bytes;
- accepted nonzero success code;
- redaction;
- composite rejection with no child marker and no state/registry material;
- timeout/cancellation status; and
- unchanged summary/json/both projections.

In `nix/gate-nix.nix`, add generated-app cases for:

- `generated leaf -- --output task-output` exact stdout;
- generated composite rejection with exit 37; and
- existing generated summary/json/both behavior.

### Power Sim gates

Test both layers:

1. Python request-builder helper invalid input remains exit 2.
2. Exported Nixfied `.#simulate` success remains exit 0.
3. Exported Nixfied child exit 2 is classified as `TASK_FAILED`, exit 30.
4. `--help` and `--list-profiles` remain exit 0.
5. Successful stdout is the exact child result envelope.
6. No runtime JSON parsing or `stdoutPath` reopening remains in the wrapper.

## 10. ABI and documentation cutover

Update `capability.txt` as one public contract change. It must include at least:

```text
run-output-mode: summary json both task-output
error-code: ... OUTPUT_MODE_INVALID OUTPUT_MODE_CONFLICT TASK_SELECTION_INVALID OUTPUT_PROJECTION_FAILED
output-schema run-task-output: redacted-stdout redacted-stderr human-diagnostics
output-schema runtime-error-cause: code exitClass message details
output-schema runtime-error-projection: stream operation kind path bytesWritten
```

The exact descriptor vocabulary must follow the existing descriptor style. Do not hand-edit a generated digest; update the Rust snapshot only after the descriptor content is final and verify Nix/Rust agreement.

Update normative documentation as follows:

- `docs/CONTRACT.md`: add the fourth output mode, direct-leaf selection, replay timing, causes, and no-late-admission-error rule;
- `docs/ARCHITECTURE.md`: document runtime-owned replay tickets, bounded concurrent replay, and finalization ownership;
- `docs/GUIDE.md`: document command usage, exact-byte behavior, status checking, and Power Sim-style application use;
- `docs/DEVELOPMENT.md`: add the output suite and gate mapping; and
- `RFC_TASK_OUTPUT.md`: retain only the normative contract and accepted alternatives, with this plan as the implementation companion.

## 11. Dependency-ordered commits

Use focused commits with one coherent design and no partial public cutover. Suggested lower-case one-line subjects:

1. `add typed task output replay lifecycle`
   - internal completion/evidence types;
   - replay ticket and bounded workers;
   - unit tests;
   - no public mode or ABI change if possible.

2. `cut over task-output runtime contract`
   - parser and mode;
   - selection timing;
   - finalizer and error precedence;
   - four public error codes;
   - capability descriptor and ABI snapshot;
   - runtime tests and normative docs.

   This is the atomic ABI cutover. Do not land the descriptor without the runtime consumer, or the runtime consumer without the descriptor/snapshot.

3. `cover task-output in framework gates`
   - Nix runtime gate;
   - generated-app acceptance;
   - help and forwarding assertions.

4. `migrate power sim to task-output`
   - remove metadata/path extraction;
   - update Power Sim documentation;
   - add exact-output and layered-exit-status acceptance.

Before each commit, inspect the staged diff. Do not include unrelated worktree changes.

## 12. Verification sequence

Run focused checks first, then the repository gates. The authoritative Nix fixture-backed test is `.#test`; raw Cargo tests are supplemental only.

Suggested sequence during implementation:

```sh
cargo fmt --all -- --check
cargo test -p nixfied-runtime --test output
nix run .#check
nix run .#test
nix run .#gate -- --dirty
nix run .#ci -- --dirty
```

After the change is committed or the tree is clean, repeat:

```sh
nix run .#gate
nix run .#ci
```

If the implementation changes release-facing runtime artifacts, include the repository's release build/check path described in `docs/DEVELOPMENT.md`. Report platform and release checks that were not run.

## 13. Risks and mitigations

### Broken pipe and diagnostic writes

Default `SIGPIPE` behavior or `eprintln!` panics could terminate the runtime before typed projection handling. Install SIGPIPE handling and route replay, footer, and task-output diagnostic writes through fallible sinks.

### Cleanup race

Opening the files before terminal registry transitions is required. Reopening paths after teardown is prohibited. Unit-test task-only cleanup races where possible.

### Error masking

Every early-return branch currently risks replacing the original outcome with a lease, teardown, or summary error. The finalizer must collect all failures and apply the documented safety → projection → task precedence.

### Cross-stream ordering assumptions

Two workers are required for independent progress. Tests must compare each stream independently and must not assert a reconstructed stdout/stderr global interleaving.

### ABI drift

The capability descriptor, Rust digest snapshot, Nix-derived digest, error enum, help, and docs must be changed together. The ABI gate must reject stale models rather than silently accepting them.

### Power Sim status confusion

The Python helper's exit 2 and the exported Nixfied task's exit 30 are different boundaries. Test and document both; do not restore path extraction merely to preserve an accidental status.

### Redaction failure

An incomplete or failed redaction relay must never be replayed. Preserve the existing fail-closed `SECRET_LEAK_BLOCKED` behavior and leave stdout empty.

## 14. Go/no-go checklist

Engineering implementation is ready to start when:

- [ ] the four error names and exit values are accepted;
- [ ] the optional `RuntimeError.causes` wire shape is accepted;
- [ ] task-output requires an explicit direct leaf;
- [ ] composite rejection is before slot/placement/state effects;
- [ ] no `details["taskRun"]` recovery remains;
- [ ] replay tickets are opened before terminal cleanup transitions;
- [ ] both replay workers always join before final error projection;
- [ ] broken pipes and closed stderr cannot panic the runtime;
- [ ] old summary/json/both behavior has regression coverage;
- [ ] the capability descriptor and Rust/Nix ABI snapshot are an atomic cutover;
- [ ] generated-app forwarding is tested without changing model/compiler code;
- [ ] Power Sim helper-level and Nixfied-level statuses are both covered;
- [ ] `nix run .#check`, `.#test`, `.#gate`, and `.#ci` pass; and
- [ ] the staged diff contains no aliases, compatibility paths, model/graph changes, or unrelated worktree modifications.
