# Shared service owner implementation design

Status: delivered and superseded. This historical design and its companion
[RFC](RFC-shared-service-owner-sessions.md) record the preimplementation review.
The maintained [contract](CONTRACT.md), [guide](GUIDE.md#share-a-service-owner),
[architecture](ARCHITECTURE.md#registry-and-liveness) and
[verification guide](DEVELOPMENT.md#change-specific-verification) own the shipped
behavior, boundaries and checks. The design text below is historical.

## Scope and deliberate refinements

Keep the existing task/service algebra, single manifest, slot guard, SQLite
registry, session executor, and explicit shutdown. Add no service membership,
second startup lock, reusable service registry, lifecycle phase, or coordinator.

Close four implementation details beyond the architectural outline:

1. Allow `--wait-timeout-ms` on inspection as well as task waiting. Inspection
   uses it to obtain its first coherent read and identity samples, without
   following later publication.
2. Reject non-UTF-8 machine locator metadata for every new run before slot or
   application mutation. Byte-preserving environment lookup remains unchanged.
   This replaces current lossy `.display()` persistence of some paths.
3. Once checked registry history is seen, disappearance, reset, replacement, or
   event-order regression is corruption, never an opportunity to bootstrap anew.
4. A shared bootstrap reaching its local deadline reports observation timeout
   with establishment uncertainty; premature EOF or malformed replies report
   lifecycle failure. Neither cancels a possible established shared owner.

## Module ownership and dependency direction

Paths in this table are relative to `runtime/crates/nixfied-runtime/src/`.
New modules are explicitly marked. Rust `pub` here permits the package's
separate executable crate and tests to use a safe facade; it creates no supported
Rust adopter API. Fields and constructors that encode proofs remain private.

| Module | Sole responsibility and interface |
| --- | --- |
| `main.rs` | Parse, admit, dispatch, and project command results. Keep the existing executor and finalizer. Split acquisition from its common owned execution tail. |
| `state/placement.rs` | Own `SlotLocation` and run-specific `HostPlacement`, validation, relative-base anchoring, and all path derivation. No registry or source lookup. |
| `execution/plan.rs` | Own checked root-leaf selection and canonical continuation validation using the existing plan. No new derivation algorithm. |
| `admission/source.rs` | Own checked historical source decoding distinct from fresh admission. No current filesystem lookup in historical decoding. |
| `registry/session.rs` | Own request provenance publication, exact owner identity, creation-marker validation, and progress decoding. |
| `registry/observation.rs` new | Own all observation SQL, checked row decoding, history continuity, publication selection, and candidate snapshots. It returns facts, never successful usability proofs. |
| `registry/sqlite.rs`, `registry/schema.rs` | Own one native read classifier, exact schema/bootstrap validation, and writable registry/guard lifetime. |
| `service/process.rs` | Own a fallible exact leader sampler for owners and services; no process parsers in control modules. |
| `service/task.rs`, `service/registry.rs` | Carry root/preparation attribution through gated registration and write canonical process evidence. |
| `control/observation.rs` new | Own inspect/await facades, request comparison, pending/unavailable decisions, leader samples, fresh revalidation, and private successful output construction. |
| `control/ensure.rs` new | Own enrollment, seeking and guarded selection. Return an owned registry or one fixed selected run, never a second executor. |
| `background.rs` | Own framed request/reply transport, detached spawn, purpose-specific cancellation, and complete-request delivery tracking. |
| `cancellation.rs` | Own an elapsed observation budget shared by observation and transport. Existing cancellation token and signal ownership remain unchanged. |
| `output.rs` | Own checked evidence-relative locators extracted from the presenter's validator. Presenter and observation share it. |
| `error.rs` | Own typed safe observation detail construction and nonrecursive cause projection. |

`control.rs` declares the two new control submodules. SQL connection access and
platform samplers remain `pub(crate)`. Public facades return opaque checked
values or wire projections; `main` gains no raw SQL, private file descriptor,
or writable proof field. Keep output records beside their native control owner,
without a generic serializer or an additional mutable output model.

## Public command parsing

Common operands preserve current native acquisition and repetition rules:
`--manifest` and `--state-base` use the last operand, including the existing bare
trailing-operand behavior; `--slot` and `--timeout-ms` use the existing unsigned
parser and last successfully parsed value. Do not change existing commands as
a side effect of adding the new parsers.

New operands occur at most once. Acquire and parse a value before detecting its
duplicate, so a malformed repeated operand reports the operand error. Help
anywhere after a recognized command wins before other parsing or admission.
Unknown operands, positional operands, `--`, and `--flag=value` reject. Runtime
entry argument acquisition keeps the current Unicode rules.

| Command | Required new operands | Other accepted operands |
| --- | --- | --- |
| `context` | None | Common manifest/state/slot operands. |
| `observe` | Exactly one of `--current` or `--run-id <component>` | Common operands, optional `--until-step <path>`, optional `--wait-timeout-ms <positive u64>`. |
| `ensure` | `--task <id>`, `--until-step <path>` | Common operands, optional `--timeout-ms <u64>`, optional `--wait-timeout-ms <positive u64>`. |

New operand syntax, acquisition, duplication, and selector conflicts use
`MANIFEST_ADMISSION`. After admission, unknown task/root step and invalid
continuation use `TASK_SELECTION_INVALID`. Existing `run --task` parsing keeps
its existing error behavior. Numeric parsing accepts the current leading-plus
form, rejects whitespace, negative values and overflow, and additionally
rejects zero for observation timeout. Operation timeout preserves zero and the
full `u64` range.

`--run-id` reuses the bounded background identity grammar: 1–128 ASCII bytes,
each alphanumeric or `-`. Extract that predicate to its native identity owner;
placement's broader filesystem-component predicate is not the selector grammar.

Default observation timeout is 60000 ms. Default operation timeout is the
existing 5000 ms. None of the new controls accepts output-mode or daemon flags.
Control admission applies to `context` and `observe`; full run admission applies
to `ensure`, including reuse. Check root-step/continuation and inherited-stdin
rejection before dispatching a shared bootstrap.

## Wire contract

All successful fields are required. No success record emits `null`. Empty
arrays remain present. JSON member order and insignificant whitespace carry
no meaning. Lists have bytewise stable order: services by `(serviceId,
processKey)`, endpoints by `endpointId`. Duplicate service or endpoint identity
is corruption rather than something sorting can hide.

Use native tagged sums for associated-data alternatives, with the exact shapes
below. Do not add optional fields for fields belonging to a different variant.
Native output proof types implement `Serialize`, never `Deserialize`.

| Record | Exact fields and scalar domains |
| --- | --- |
| `Context` | `projectId: string`, `environment: string`, `slot: u32`, and UTF-8 path strings `stateBase`, `stateRoot`, `registryDir`, `registryPath`. |
| `RecordedSource` | `codebaseId`, `logicalRoot`, `observedRoot`, `sourceMode`, `sourceIdentity`, `dirtyPolicy`, `admissionFingerprintPolicy`. Source mode and dirty policy reuse existing closed vocabularies; fingerprint policy remains uninterpreted string data, including empty. `observedRoot` is an absolute UTF-8 path string. |
| `Request` | `computedManifestHash: lowercase SHA256 hex`, `rootTask: task id`, `source: RecordedSource`, `operationTimeoutMs: u64`. |
| `Leader` | `pid: u32 in 1..=i32::MAX`, `startIdentity: nonempty string`, `observation: live | absent | identity-mismatch`. Identity is the recorded expected platform token; a mismatch never replaces it with the newly observed token. No null start identity. |
| `EndpointCoordinate` | `endpointId: endpoint id`, `host: declared loopback host string`, `port: nonzero u16`. These are recorded coordinates, not fresh socket witnesses. |
| `ServiceLeader` | `serviceId`, `processKey`, `leader: Leader`, `endpoints: [EndpointCoordinate]`. |
| `Progress` | One of the three tagged objects below. Outcome uses the existing execution-outcome vocabulary. |
| `Session` | `runId`, `manifestPath`, `request: Request`, `progress: Progress`, `runDir`, `logsDir`, `owner: Leader`, `services: [ServiceLeader]`. |
| `CompletedRootTask` | `stepPath`, `taskId`, `processKey`, `exitCode: i32`, `stdoutPath`, `stderrPath`. Exit code is present and policy-accepted, including an accepted nonzero code. |
| `TaskObservation` | `context: Context`, `session: Session`, `task: CompletedRootTask`. |

Exact progress alternatives are:

```json
{"state":"outcome-pending"}
```

```json
{"state":"finalizing","outcome":"failed"}
```

```json
{"state":"finalized","outcome":"canceled"}
```

Top-level records are exactly:

```text
context:           Context
observe absent:    {result: "absent", context: Context}
observe inspect:   {result: "session", context: Context, session: Session}
observe await:     {result: "satisfied", observation: TaskObservation}
ensure:            {disposition: "established" | "observed",
                    observation: TaskObservation}
```

Inspection absence exits 0, including an unknown exact run ID. Awaiting that
same absence returns `SESSION_UNAVAILABLE`. Inspection may report historical
provenance under another current manifest with matching registry ABI/identity;
awaiting requires exact current/recorded manifest-hash equality before plan
interpretation. An endpoint-less service emits `endpoints: []`.

New controls use structural JSON projection with an empty redactor: only
validated metadata, constant vocabulary, and native evidence locators enter
these records. They return no captured bytes or arbitrary event payload.
Do not recursively redact keys or locators. Captured files retain existing
redaction; run admission and execution errors retain their normal redaction
before being wrapped or projected.

## Native types and construction boundaries

The following names fix responsibilities; unrelated existing arguments are
unchanged. New proof fields are private, and no fallible decoder returns a host
admission proof.

| Type | Construction and visibility |
| --- | --- |
| `SlotLocation` | Public opaque value constructed from selected slot, effective base and invocation cwd by placement. Its absolute lexical coordinates do not assert filesystem identity. |
| `HostPlacement` | Public opaque run placement constructed from `SlotLocation` and a validated run component; derive all evidence paths from these facts. Delete the dummy `"control"` run in slot-only callers. |
| `RecordedSource` | Checked historical metadata constructed by a source-owned decoder, without resolving today's source or secrets. Separate from `AdmittedSource`. |
| `DeclaredRunRequest<'a>` | Opaque borrowed view over an actual `RunAdmission`, selected task and operation timeout. Its factory checks task selection. No stored bytes can construct it. |
| `ObservedRootLeaf<'plan, 'manifest>` | Public opaque borrow of a root `PlanNode` and its position; constructor is `RunPlan::root_leaf(stepPath)`. Expose task/policy and whether a later root occurrence exists. No self-referential owned plan/admission struct. |
| `PreparationLeaf<'plan, 'manifest>` | Opaque plan-node borrow minted only by its checked `ServiceBinding` preparation iterator. |
| `TaskOccurrence<'plan, 'manifest>` | Closed `Root(ObservedRootLeaf)` or `Preparation(PreparationLeaf)`. The task runner derives node id and task from this single value instead of accepting independent conflicting node/task/attribution inputs. |
| `Publication` | Private checked run ID plus positive native creation sequence. Marker validation constructs it. |
| `PinnedRun` | Public opaque publication plus checked scope/history. No acquire/recover operation is exposed. |
| `Seeking` | Public opaque non-cloneable enrollment scope, frozen watermark and history anchor. Only enrollment or validated private resumption constructs it. |
| `RegistryRead` | Private closed `Absent`, `BootstrapEmpty`, `Busy`, or `Snapshot`, with a checked file stamp where applicable. Snapshot owns one read transaction. |
| `CandidateSnapshot` | Private canonical decoded run/root/service facts; it is not a successful observation. |
| `CompletedRootTask`, `TaskObservation` | Output types with private fields; only successful observation validation constructs them. |

Make `RunPlan`, `PlanNode`, and `ServiceBinding` construction private to the
execution owner. Replace their current public fields with immutable accessors
and checked iterators; a public mutable/fabricated plan cannot mint occurrence
proofs. This is a visibility refactor, not a second graph implementation.

The library facades needed by the binary are:

```text
observation::inspect(control admission, location, selector, budget, cancellation)
    -> RuntimeResult<ObserveOutput>
observation::await_selected(control admission, location, selector, step,
                            budget, cancellation)
    -> RuntimeResult<TaskObservation>
observation::await_root(control admission, location, pinned selector, step,
                        budget, cancellation)
    -> RuntimeResult<TaskObservation>
ensure::enroll(run admission, location, selected task, operation timeout,
               budget, cancellation)
    -> RuntimeResult<Enrollment::Pinned | Enrollment::Seeking>
ensure::acquire_shared(actual DeclaredRunRequest, location, Seeking,
                       expected request, precommit channel)
    -> RuntimeResult<SharedDecision>
```

`await_selected` fixes one selection under control admission and delegates to
`await_root`; it needs no full run admission. Ensure uses its already-pinned
path directly, without selecting again.

`SharedDecision` is a closed sum of `Selected(PinnedRun)` and
`Establish(Registry)`. If an acquired guard proves a selected publication has
released authority, `acquire_shared` checked-closes its own registry and returns
request mismatch if comparison fails, otherwise
`SESSION_UNAVAILABLE/authority-released`. It returns no reusable pinned success
handle for that path. Only `Establish` can move its registry into the common
executor. The actual request includes the independently admitted selected task
and timeout; expected wire metadata cannot substitute for those facts.

Selection/matching/waiting are native library operations. Main's shared-parent
dispatch composes these facades with `background` transport, while the hidden
owner uses `acquire_shared`. No facade returns a database connection, raw
descriptor, mutable request proof, or constructor for a successful observation.

## Persistence and exact codecs

Change registry schema version 19 to 20. No migration or old-schema reader.
Modify the single authored `SCHEMA_SQL`; existing exact schema-object comparison
continues to derive its expectation from that definition.

Add these columns and constraints to the existing table definitions:

```sql
-- runs
root_task TEXT NOT NULL CHECK (length(root_task) > 0),
operation_timeout_ms TEXT NOT NULL CHECK (
  typeof(operation_timeout_ms) = 'text'
  AND length(operation_timeout_ms) BETWEEN 1 AND 20
  AND operation_timeout_ms NOT GLOB '*[^0-9]*'
  AND (operation_timeout_ms = '0'
       OR substr(operation_timeout_ms, 1, 1) BETWEEN '1' AND '9')
  AND (length(operation_timeout_ms) < 20
       OR operation_timeout_ms COLLATE BINARY <= '18446744073709551615')
),

-- processes
root_step_path TEXT CHECK (
  root_step_path IS NULL OR
  (role = 'task' AND length(root_step_path) > 0)
)
```

```sql
CREATE UNIQUE INDEX processes_unique_root_step
ON processes (run_id, root_step_path)
WHERE root_step_path IS NOT NULL;
```

`registry/session.rs` writes root task and timeout atomically with `run.created`
for all run modes. Reuse existing manifest hash and source columns; add no
request hash or duplicate provenance JSON. Encode timeout using `u64::to_string`;
decode with full `u64` parsing plus canonical round-trip equality, never a SQLite
numeric cast. The source module decodes exactly its seven named fields with
unknown/missing/null/type rejection and lexical validation, without current
filesystem resolution. Invalid stored values report field names, never echo
untrusted contents.

Historical source validation requires `codebaseId = "main"`, the existing
confined-relative `logicalRoot` predicate (including `"."`), an absolute lossless
UTF-8 `observedRoot`, and a nonempty `sourceIdentity` that is absolute for
immutable modes. Reject NUL in paths. Preserve `admissionFingerprintPolicy`
exactly as authored; do not invent an enum or nonempty constraint for it.

Keep the existing `owner_identity` column as its one canonical record, with
exact private JSON shape `{pid: nonzero u32, platformStart: nonempty string}`.
Use a closed decoder; remove the former nullable owner-start construction. A new
run must obtain its exact self identity before committing any run row.

Run metadata path encoding must use checked lossless UTF-8, not `.display()` or
`to_string_lossy()`. Preflight manifest locator, source observed root, effective
slot coordinates, and derived run/log paths before slot acquisition for every
new run mode. Failure uses existing serialization `LIFECYCLE_FAILED` and starts
no state mutation or workload. `state_base_from_env` itself remains byte-oriented.
Historical path decoding does not canonicalize or require that old sources exist.

Process `stdout_path`/`stderr_path` remain run-relative locators. Extract one
`EvidenceRelativePath` parser into `output.rs`: exactly `diagnostics.log`, or
literal `logs/` plus one nonempty Unix normal filename. Reject NUL, extra `/`,
`.` and `..`, repeated separators, dot components, and trailing separators;
backslashes remain ordinary Unix bytes. Task evidence specifically requires the
`logs/` form. Join through the selected run placement, never through a path in
redacted JSON. This lexical tightening is part of the same ABI/schema cutover.

`TaskOccurrence::Root` lowers to the nullable root column in the same gated
process-registration transaction. Preparations/probes/services have no root
attribution. Root membership is checked before registration; uniqueness failure
rejects before the gate permits work. The task's existing terminal transaction
continues to own status, ownership and capture; attribution never upgrades them.

## Registry reads and history continuity

Add one internal read classifier shared by strict and polling consumers. Open
read-only/no-follow, set SQLite busy timeout to zero on the polling path, begin
one coherent snapshot, then classify:

| Native result | Meaning and treatment |
| --- | --- |
| `Absent` | Registry file is genuinely absent. Before enrollment, absence can fix `H = 0`. |
| `BootstrapEmpty` | The existing exact predicate holds: `user_version = 0` and `sqlite_schema` contains zero objects. No version-only approximation. |
| `Busy` | SQLite primary error is BUSY or LOCKED, including its extended codes. Polling consumers retry with their budget; this result establishes no watermark. |
| `Snapshot` | Complete exact schema, registry identity and native row domains validate in one read transaction. |
| Error | Malformed/unsupported schema, wrong identity, NOTADB, filesystem and I/O errors remain terminal. |

Existing `RegistryReader::open_existing` remains the strict facade over this
single classifier: absence returns `None`, bootstrap/busy are errors as in its
current policy. Existing `ps`, `down`, and presenter behavior stays strict;
new observer controls use the polling result. There is no second schema parser,
old-byte fallback, writer initialization by a reader, or swallowed corruption.

Track invocation-local history, not durable membership. Reuse the filesystem
owner's existing `FileIdentity` (device/inode), without a second OS codec. Its
closed phase progresses `Unseen -> KnownBootstrap -> KnownComplete`; a first
complete snapshot can skip bootstrap. Bootstrap and complete carry the checked
identity; complete also carries the greatest checked event sequence, separate
from the frozen enrollment watermark `H`. Verify identity around actual reader
opening and at subsequent snapshots. Once known, disappearance/replacement
rejects. Complete can never return to bootstrap, including at `H = 0`, and
`max(seq)` can never fall below the greatest previously checked sequence.
Bootstrap may progress only to the complete schema in that same file. WAL growth
does not replace the main database. These are sampled continuity checks, not a
durable epoch or a promise against undetected same-UID/inode reuse.

Under acquired authority, validate continuity before `open_or_create` can
initialize schema or change journal mode. Keep phase, identity and greatest
sequence in the private bootstrap cursor. The helper restores it through a
private fallible `resume_from_wire`, binding actual admitted scope/location and
checking current history while preserving `H`; decoding a DTO neither mints
`Seeking` nor reenrolls. Replies carry learned continuity back to the parent,
which validates and monotonically merges it before subsequent reads.

Validate creation provenance in both directions before selection: every
`run.created` has a non-null run reference and null process reference and names
exactly one existing run; every run has exactly one creation marker. Duplicate,
missing and dangling markers are corruption even if a query could otherwise
skip them. No redacted payload participates.

After validated enrollment, freeze `H = max(events.seq)` once. Missing/bootstrap
enrollment has `H = 0`; busy reads do not enroll. Select publication by this
native query, after the validation above:

```sql
SELECT seq, run_id FROM events
WHERE event_type = 'run.created' AND seq > ?1
ORDER BY seq ASC LIMIT 1;
```

Never join/filter before selecting the first marker. Check selected facts
afterward, including mismatch or failure. Drop every reader transaction before
waiting, acquiring the slot, or opening a writer.

## Establishment and the one executor

Refactor `run_placed` into acquisition and one common `execute_owned` tail:

```text
ordinary foreground/daemon
  admit/plan/preflight
  acquire SlotGuard -> Registry
  execute_owned(Registry by value)

ensure parent
  admit/plan/preflight -> start budget -> enroll
  Pinned -> compare -> await_root
  Seeking -> shared background request -> Established/Selected -> await_root

shared hidden owner
  decode -> authoritative admission/plan/preflight -> compare expected request
  acquire_shared(Seeking)
    Selected -> reply selected-existing, exit without a registry writer
    acquired authority + selected publication -> checked close, reject, exit
    Establish(Registry) -> execute_owned(the same Registry by value)

execute_owned
  ordinary predecessor recovery -> abandonment checkpoint -> prepare state
  claim evidence/FIFO/diagnostics -> final abandonment check -> run.created
  consume precommit channel -> acknowledgement
  existing service startup -> root task execution -> existing finalizer
  checked registry close -> slot release
```

The shared guarded branch is before predecessor recovery or application-state
preparation. Recheck first post-H publication under held authority. If one
exists, close without recovery or creation; acquiring authority proves that
publication released it. If none exists, enter the existing tail exactly once.
Do not call the acquisition wrapper again or duplicate service/finalizer logic.

Enrollment fixes the only live unfinished owner when one exists, including a
known execution settlement. Multiple live unfinished owners are corruption.
Dead pre-H owners are predecessors to recover under authority, even with live
orphan services. In `Seeking`, first post-H publication always wins, regardless
of status or request match. Once selected, that operation never acquires,
recovers, restarts or follows another run.

`execute_owned` accepts a closed establishment purpose, not `Option` plus an
unrelated sharing flag. Preserve ordinary daemon abandonment and cancellation
behavior. Shared precommit checks occur while seeking and immediately before
recovery, after recovery, and before run commit. Already-begun predecessor
recovery can finish after launcher loss; its next abandonment check prevents
state preparation and a new run. The operation timeout bounds individual stops,
not total recovery or helper lifetime; settlement and deletion retain their
existing behavior without a new global deadline. Observer timeout does not
leave the caller blocked waiting for cleanup. Existing recovery remains the
sole owner of its recorded effects and unresolved obligations.

## Private bootstrap protocol and cancellation

Rotate the private session-owner protocol magic from `NXD1` to `NXD2` when its
request changes; retain one protocol, u32 big-endian length framing and the
1048576-byte body limit. Replace the old request decoder; no dual reader.

The request is a closed sum tagged by `purpose`:

```text
daemon {runId, args}
shared {runId, execution, expectedRequest, cursor}

execution = {manifestPath, allowNonStoreManifest, task, slot, stateBase,
             operationTimeoutMs}
cursor = {watermark: nonnegative i64,
          history: {state: "unseen"} |
                   {state: "bootstrap", device: u64, inode: u64} |
                   {state: "complete", device: u64, inode: u64,
                    greatestSeq: nonnegative i64}}
```

Unseen/bootstrap requires watermark zero; complete requires greatest sequence
at least the watermark. All DTOs reject unknown/missing/null/wrong-type fields.

Shared paths are effective absolute UTF-8 metadata. The helper inherits the
runtime's invocation cwd/environment through ordinary detached spawn; workloads
still receive only their declared hermetic environment. Expected request is
non-secret metadata. The helper derives its actual request/context from its own
admission and compares before acquiring or mutating; it does not deserialize an
admission proof. Recheck source-root changes across admissions; matching mutable
secret values is deliberately not promised.

Replies are closed and tagged by `result`:

```text
established {runId, runDir, logsDir, cursor}
selected-existing {runId, createdSeq, cursor}
rejected {code, message, details, causes}
```

The shared variants carry the updated cursor; the ordinary daemon's established
variant retains its existing acknowledgement fields without a cursor. Express
that purpose difference as a closed private sum. Causes use the existing bounded
safe nonrecursive cause projection, preserving a checked-close failure without
deserializing an admission proof. Empty causes are an empty private array and
remain omitted in the public error contract.

The helper redacts admission/execution rejection diagnostics using its own
admitted secrets before attaching typed observation metadata. Native observation
errors and their exact structural metadata use the empty redactor; do not run
recursive redaction over a completed envelope or its safe nested causes.
Selected replies contain only bounded selection metadata; the parent rereads
and validates canonical facts and creation order. It never trusts returned
output as a successful observation. A shared postcommit reply failure never
signals the owner or produces a precommit rejection.

`WaitBudget` owns `started: Instant` and `limit: Duration`; remaining time is
`limit.saturating_sub(started.elapsed())`. Accept full positive u64 milliseconds
without constructing an overflowing `Instant + duration`. Poll in at most
10 ms budget slices, using checked short deadlines for existing channel helpers.
Never serialize an `Instant` or reset the budget at dispatch, reply or task wait.
The helper relies on channel abandonment before commit; no synchronized clock or
additional deadline handshake is required.

At each polling checkpoint, observer signal wins over local expiry; encountered
structural/inspection failures are returned before another poll. Recheck signal
and expiry before reporting success. Transport follows these exact rules:

| Event | Shared action and command outcome |
| --- | --- |
| Request frame not fully written | Helper cannot have admitted work. Close and terminate/reap only that unestablished helper; report the encountered transport, cancellation or timeout outcome. |
| Complete frame delivered, no conclusive reply | Never kill helper or write the owner's FIFO. Closing/half-closing requests precommit abandonment; a committed owner continues. |
| Signal, including with a reply available | `CANCELED`, retaining selection/candidate information; postcommit shared owner survives. |
| Local budget expiry | `OBSERVATION_TIMEOUT` with stage `establishment`; after complete delivery, establishment is `uncertain` unless a conclusive reply is already known. |
| EOF/malformed reply before signal/expiry | `LIFECYCLE_FAILED` with establishment `uncertain` after complete delivery. |
| Conclusive rejected reply | Report that safe rejection; no new owner was established by this helper. |
| Established or selected reply before signal/expiry | Drop transport and await exactly that selected run with the original budget. |

Delivery tracks actual frame-write completion, not whether an acknowledgement
arrived. For shared purpose, no branch invokes ordinary `interrupt()` or
`request_cancellation`. Do not add the ordinary daemon's ten-second interruption
grace. Reap helpers already known to have exited without an extra blocking
observer grace. Retain a selected-existing helper's `Child` until reaped, using
`try_wait` in ordinary observation iterations without another thread; drop any
remaining handle when the command exits. A possible established owner is left
to normal host adoption.

## Observation algorithm and rejection priority

Plain inspection selects once, including absence, then reports typed recorded
facts and leader samples. Its budget bounds read contention and sampling; it
does not wait for future publication. Awaiting fixes the selected publication,
requires exact manifest hash, derives the recorded root plan once, and resolves
the selected root leaf through the graph owner.

Read one candidate snapshot, drop its read transaction, sample exact owner and
all service leaders, open a final fresh snapshot and revalidate unchanged task,
service identities, endpoints, outcome absence and successor absence. Drop it,
sample the owner once more and require live, then check observer signal/deadline
before constructing successful output. No observation claims an atomic
multi-process snapshot, owner-thread progress, node acceptance, full containment
checkpoint, fresh listener health, or future availability.

The process layer returns a fallible sample `Absent` or
`Present {startIdentity, pgid, zombie}`. Linux parses one exact proc stat record;
macOS checks the complete native process record. Missing-process races
(ENOENT/ESRCH) are absence; permission, unsupported inspection and malformed
records are errors, never death. Recheck identities around composite samples.
For owner publication, exact self identity must be live and nonempty. For
services, require recorded start identity and pgid; owner observations require
PID/start identity without imposing service process-group rules. Zombies are
absent; stopped leaders remain live without promising progress. Before native
calls, validate PID/PGID in `1..=i32::MAX`, the supported positive `libc::pid_t`
domain, rather than casting arbitrary `u32`. Managed-service registration
preserves its existing `pgid == pid` invariant. Reuse the owning platform parsers,
not the current Option-returning
helpers as an absence oracle or `ps`'s obligation liveness.

Classify each fully decoded snapshot in this order. Structural decoding/history
errors take precedence over these semantic outcomes:

| Priority | Predicate | Result |
| --- | --- | --- |
| 1 | Requested declaration differs | `REQUEST_MISMATCH`, first field in order manifest, root-task, source, operation-timeout. |
| 2 | Selected absence | `SESSION_UNAVAILABLE/absent`. |
| 3 | Guarded selection proved authority release | `SESSION_UNAVAILABLE/authority-released`. |
| 4 | Owner identity mismatched/absent or later publication exists | `owner-replaced` or `owner-gone`; later publication uses `owner-replaced`. |
| 5 | A recorded run outcome exists | `SESSION_UNAVAILABLE/execution-settled`, even if the root task also failed. |
| 6 | Exact root task has committed non-success terminal status/outcome | `SESSION_UNAVAILABLE/task-failed`. |
| 7 | Exact task has insufficient finished evidence or failed settlement as defined below | `SESSION_UNAVAILABLE/evidence-incomplete`. |
| 8 | Existing service row is terminal/settled | `SESSION_UNAVAILABLE/service-settled`. |
| 9 | Existing service leader is absent/mismatched | `service-gone` or `service-identity-mismatch`, service-id order breaks multiple failures. |
| 10 | Startup/task/capture remains legitimately in progress | `Pending`; refresh under the same budget. |
| 11 | All success predicates and final freshness checks hold | Construct `TaskObservation`. |

Extra/duplicate services and mismatched endpoint sets reject immediately.
Before a root row exists, only missing not-yet-started services are legitimate
pending startup. Root attribution proves startup completed before workload
release; after its row exists, missing expected services are also corruption.
A root step outside its bound plan, contradictory terminal facts, or a missing
creation marker is corruption at any point.

Active/unresolved tasks with pending capture can be in progress even after an
observed exit. Active/unresolved with any finished capture outcome, including
`complete`, means settlement failed: report `evidence-incomplete` immediately.
Incomplete/unknown capture also means incomplete evidence, unless an earlier
task/run failure takes precedence. Recovery can validly leave a canceled,
settled task with pending capture; that is unavailable, not corruption.
Terminal success with pending capture is contradictory. A source-produced
partial publication can instead have `succeeded`, settled ownership and complete
capture with both outcome and exit absent when exit recording failed; report
`evidence-incomplete`, never success or endless pending. Known contradictory
outcomes, an exit without an outcome, or a succeeded outcome without a present
policy-accepted exit remain corruption.

Canonical success requires task status and execution outcome `succeeded`, a
present policy-accepted exit, ownership `settled` and capture `complete`.
Each service is `ready`, unresolved, and has no execution outcome. The run has
no committed outcome. All exact leaders sample live. Do not decode summaries,
parse human diagnostics, infer current health, or reconstruct missing errors.

## Error details and output delivery

Keep error codes/exit statuses from the RFC: `SLOT_BUSY` 39,
`REQUEST_MISMATCH` 40, `OBSERVATION_TIMEOUT` 41, `SESSION_UNAVAILABLE` 42.
They use exit class `error`. Existing codes retain their existing numbers.

Add one typed nested `details.sessionObservation` envelope; add that one key to
safe nonrecursive cause projection. Fixed nested records, constant reason enums
and validated locators are constructed by native error owners. Do not add broad
allowlisting of arbitrary `reason` or `field` data.

```text
Selection = {state: "unenrolled"}
          | {state: "absent", selector: {kind: "current"} |
                                       {kind: "run-id", runId: string}}
          | {state: "seeking", watermark: nonnegative i64}
          | {state: "pinned", runId: string}
Establishment = {state: "not-requested"}
              | {state: "not-established", candidateRunId: string}
              | {state: "uncertain", candidateRunId: string}
              | {state: "established", runId: string}
```

`slot-busy` details are `{kind: "slot-busy", context}`. Mismatch details are
`{kind: "request-mismatch", field, selection}`. Timeout and observer cancellation
use `{kind: "timeout" | "canceled", stage, selection, establishment, stepPath}`;
`stepPath` is omitted when no root wait was requested. Bootstrap uncertainty
uses `{kind: "bootstrap-uncertain", selection, establishment}`. Unavailability
uses `{kind: "unavailable", reason, selection, stepPath?, evidence?}`, with
evidence exactly `{stdoutPath, stderrPath}` when checked paths exist. Optional
members represent genuine unavailable information, never a different variant.
Do not invent a watermark before the first checked enrollment snapshot.

Stages are `selection`, `establishment`, `task`. Reasons are exactly the RFC's
ten unavailable reasons. No mismatch detail contains expected/actual secrets.
Metadata sources contain no resolved secrets or arbitrary registry strings.
An original unobserved startup failure is not reconstructed from an outcome.

For example, expiry after complete shared dispatch can report:

```json
{
  "code": "OBSERVATION_TIMEOUT",
  "exitClass": "error",
  "message": "session observation deadline expired",
  "details": {
    "sessionObservation": {
      "kind": "timeout",
      "stage": "establishment",
      "selection": {"state": "seeking", "watermark": 12},
      "establishment": {"state": "uncertain", "candidateRunId": "run-example"},
      "stepPath": "owner.initialize"
    }
  },
  "manifestPath": "/nix/store/example-manifest/manifest.json",
  "computedManifestHash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}
```

Empty causes are omitted under the existing error contract. Plain new-control
success emits only its JSON value; help remains ordinary text. Pre-serialization
path/type failure keeps the existing `LIFECYCLE_FAILED` classification; caller
stream write failure is `OUTPUT_PROJECTION_FAILED`. Neither output failure
cancels an owner. Known acquired-registry closure is always attempted; combine
an earlier operation error with a close error through existing `error::both`,
preserving the first failure and recording the close failure as a safe cause.
Route recognized `context`, `observe` and `ensure` errors to JSON even when
parsing fails before admission; the existing `--output`-based projection switch
cannot choose their format because those commands do not accept that flag.

## Nix and ABI attachment

| Authored owner | Required atomic change |
| --- | --- |
| `capability.txt` | Add context/observe/ensure, every output variant record, closed reason/field/stage/result vocabulary, error codes, and new private-bootstrap semantics. Rotate the digest; preserve manifest shape. |
| `nix/meta/outputs.nix` | Declare each exact variant record and the source record; reuse existing scalar/outcome vocabularies. Native sums remain handwritten. |
| `nix/meta/commands.nix` | Declare new commands/operands/defaults/help; permit a list of output-record references in the small runtime helper so every observe variant is discoverable. No union generator. |
| `nix/project-publications.nix`, project apps/reserved-name owner | Publish exactly context/observe/ensure through existing pinned runtime wrappers, add descriptions and reserved names from the publication owner. |
| Native Rust owners | Serialize typed variants with `result`/`state` tags and existing outcome wire values; attach generated unit vocabularies without duplicating a configurable outcome registry. |
| ABI/check owners | Regenerate existing generated enum/binding files and ABI snapshot; update coverage, raw output goldens, parser/help fixtures, protocol tests and exact schema expectations. |
| Documentation | Replace affected contract/guide/architecture descriptions in the same delivery. Keep manifest and marker numeric versions; registry becomes 20 and private protocol becomes NXD2. |

Do not extend the metadata language with a generic union interpreter or generate
runtime output behavior from a second schema. Output variant records describe
the native sum's fields; native exhaustive matches own construction and
serialization. Follow the existing freshness and whole-inventory checks.
For nested sums such as progress and error selection, use the existing
`OpenJson` metadata annotation plus separately inventoried exact variant records,
as with socket identity. Native Rust fields remain closed enums, never arbitrary
`Value`. No new generic metadata union language is needed.

## Independent acceptance and engineering handoff

Ownership/concurrency, lifecycle/evidence, public-contract/integration, and
implementing-engineer reviews found no remaining design blockers after revision.
The contracts and boundaries above are the handoff baseline; this review is not
an implementation or test result.

Implement deterministic barriers using the existing Rust/Nix fixtures; do not
add production test-only CLI flags, another process scanner, or a second owner
implementation. The engineer must satisfy these independent oracles:

| Proof | Location and expected observation |
| --- | --- |
| Wire and parser | `tests/output_structure.rs`, `main_tests.rs`, help fixtures: literal absent/session/satisfied/ensure and every error shape; exact null/omission, empty arrays, ordering, duplicate/malformed operand precedence, help before all effects. |
| Codecs and schema | `tests/registry.rs`: zero, i64 maximum plus one, u64 maximum, leading zeros/signs/whitespace/overflow; closed source/owner decoding; root uniqueness and role gates; no admission proof from stored bytes. |
| Locator admission | `tests/context.rs`, `tests/state.rs`, output tests: relative base/cwd equivalence, lossless UTF-8 preflight, non-UTF-8 new-run rejection with no state/child effects, locator traversal/alias/NUL rejection and literal backslashes. |
| Publication | New `tests/observation.rs` with barriers before schema commit, before run commit, after commit before reply, before/after guarded recheck: first of multiple publications wins even if failed; no recovery after selection; schema busy differs from corruption. |
| History | Delete/replace/reset/rewind after checked enrollment, including complete history at H=0, phase/high-water learned by the helper, and inject missing/dangling/duplicate creation markers: reject, never establish a fresh owner or skip the first marker. |
| Shared transport | Partial frame, complete frame/no reply, malformed/EOF, cancellation at commit, local expiry, delivery write stall: exact error/stage/certainty, no deadline reset or post-delivery helper kill, no shared FIFO cancellation. |
| Process identity | Library process tests plus Linux/macOS integration: absent, zombie, PID reuse, group move, permission/malformed inspection, death between samples; no uncertainty becomes absence or descendant obligation becomes leader liveness. |
| Task evidence | Root/preparation same spelling, accepted nonzero exit, active/pending capture, finished-capture/failed settlement, failed exit recording, recovery-canceled/pending capture, duplicate service/endpoint, and final freshness settlement: exact pending/unavailable/corrupt result. |
| Execution reuse | Existing output/lifecycle suites plus shared tests: foreground, ordinary daemon and shared helper all enter one executor; initiating/other observer cancellation leaves a committed shared owner and other observers alive. |
| Scope and shutdown | Endpoint-less, one/multiple endpoints and fixed groups; `down`/cleanup retain selected-session handoff and retention gates; ordinary launcher's cancellation remains effective on an observed ordinary owner. |
| Real adopter | Both Power Sim paths invoke one native ensure; Rust launcher validates captured target, connects/adopts expected schema and spawns clients independently; cancel each client, explicit owner shutdown, stale target and schema rejection. |

For race tests, use independent run/process counts and child-produced markers,
not the implementation's own selection helper to compute expected answers.
Particular barrier names are fixture internals; their required positions and
expected outcomes above are the contract.

Stage private refactors and invariant tests first, then deliver the public
contract atomically. Each commit leaves one coherent current contract. Run
focused Rust tests through the pinned fixture environment, `.#test`, `.#ci`,
and the documented downstream dirty-source gate, plus release builds and native
Linux/macOS coverage. PostgreSQL/client acceptance is additional to generic
fixtures. A design sign-off is not evidence those implementation checks passed.

Engineering may choose local function layout, borrow/lifetime spelling, SQL
formatting and fixture organization. It must not invent wire variants, new
matching equivalence, fallback registry handling, retries after selection,
alternate cancellation propagation, or stronger readiness claims. Such changes
return to design review.
