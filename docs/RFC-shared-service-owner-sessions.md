# RFC Shared service owner sessions

Status: proposed. Scope: generic runtime controls and observation. This proposal
does not change the current [contract](CONTRACT.md).

## Decision

Support concurrent callers that start or observe one detached owner session and
wait for its declared application initialization. Keep one exclusive slot owner,
ordinary tasks and services, static graphs, and explicit owner shutdown.

Add native resolved context, exact root-task observation, and an explicit `ensure`
control that composes detached establishment with observation. A caller's
cancellation ends its observation without stopping an established owner. The
runtime remains unaware of PostgreSQL, schemas, queues, indexes, or application
target formats.

The unit of reuse is a session selected by its declared execution request. It
may own one service or a fixed group. Observers acquire no process ownership,
graph membership, lease, or authority to extend its lifetime.

## Problem situation

Power Sim needs one persistent PostgreSQL owner shared by concurrent clients.
Each client must be independently cancellable. Stopping a client must leave the
database and other clients running; explicit owner shutdown must settle all
processes that owner started. Data retention and process lifetime are separate.

Investigation identified four integration problems.

1. **Establishment precedes startup completion.** `run --daemon` commits a run
   and returns `{runId, runDir, logsDir}` before starting its services. The
   acknowledgement proves framework admission and session establishment, not
   readiness, initial health, or application initialization. Power Sim therefore
   polls run summaries, identifies its initialization task, parses its output,
   and admits the returned application target. See
   [`run_placed`](../runtime/crates/nixfied-runtime/src/main.rs#L989).
2. **Competing starts reject without coalescing.** The occupied slot rejects a
   second owner as `CLEANUP_REFUSED`, which also describes other refusals. Power
   Sim adds a startup lock, but its default launch and explicit database-start
   entrypoints do not consistently use it. Native exclusion prevents duplicate
   ownership, while a racing caller can fail unnecessarily. See
   [`SlotGuard::acquire`](../runtime/crates/nixfied-runtime/src/state/ownership.rs#L27).
3. **Placement has no public resolved query.** A caller cannot obtain the owner
   target's native state location through public discovery. Power Sim mirrors
   environment precedence, platform defaults, and application-root layout. See
   [`state_base_from_env`](../runtime/crates/nixfied-runtime/src/state/placement.rs#L103).
4. **Public ready status precedes initial health.** The readiness transaction
   sets `registryStatus` to `ready`; the subsequent health transaction preserves
   that status. A caller cannot infer completed health from `ps`. Moving
   initialization into a probe obscures permanent application rejection behind
   retry exhaustion and does not make premature target publication safe. See
   [`commit_service_check`](../runtime/crates/nixfied-runtime/src/service/registry.rs#L331)
   and [probe rounds](../runtime/crates/nixfied-runtime/src/service/process.rs#L441).

These behaviors follow the current contract. Root graph tasks execute only
after every required service has completed readiness and initial health.
Health is one startup phase with its own retry rounds; later supervision
observes process liveness and containment, without continuous protocol health.

The retained generic probe is at
`/tmp/power-sim-nixfied-lifecycle.VAwM9TjN`. Its pinned source matches the cited
files at repository revision `750301f648d977b808e16a31929982372f2757f8`.
It demonstrated early acknowledgement, same-slot contention, readiness followed
by health and dependent execution, and complete owned-process settlement after
`down`. Its service was an endpoint-less synthetic process. It establishes no
PostgreSQL, simulation, physics-replay, or cross-platform integration result.
The Power Sim entrypoint discrepancy is adopter investigation evidence rather
than a property exercised by this probe.

## Invariants and authority

| Invariant | Owner and rejection boundary |
| --- | --- |
| At most one session mutates a slot and owns its service processes | Rust slot authority rejects competing owners before application-state mutation or workload release. |
| Each observation refers to one immutable run | Native selection fixes the run once; failure or replacement never retargets an observer to a successor. |
| Reuse means the same declared owner request | Establishment records immutable selection provenance from the actual owner's admission; native matching rejects a different request before reporting reuse. |
| Successful observation requires committed task evidence and separately checked leader identities | Native readers reject incomplete capture, ambiguous root attribution, contradictory records, and unavailable selected owners. |
| Observer cancellation affects only that observer | The detached launch protocol separates precommit abandonment from postcommit owner cancellation. |
| Placement has one implementation | Rust derives all coordinates; context lookup never creates state or acquires mutation authority. |
| Application admission remains application-owned | An ordinary initialization task and each consuming client reject their domain-specific invalid targets. |

Use private admitted request types, a non-cloneable slot guard, typed selectors,
and exhaustive observation outcomes. Do not encode alternatives with correlated
booleans or use absence to hide failure. Registry readers hold no mutation
authority. Completed evidence and current liveness are separate product facts.

## Proposed controls

The names and operands below define the proposed public behavior. All generated
apps supply their pinned manifest and support help before admission or effects.

| Control | Purpose | Effects |
| --- | --- | --- |
| `context` | Return selected slot identity, state base, application root, and registry coordinates | Control admission and native path derivation only. |
| `observe` | Inspect one selected session or await one exact root task occurrence | Checked registry reads and OS identity observations only. |
| `ensure` | Start one detached session when idle, or observe the same declared owner request | Ordinary run admission, exclusive establishment when needed, then read-only observation. |

Existing `run`, daemon acknowledgement, `ps`, `down`, and cleanup retain their
ownership and phase meanings. Slot contention receives a distinct `SLOT_BUSY`
outcome; other refusals keep their own classifications. Endpoint startup-lock
contention remains the existing distinct `PORT_CONFLICT` case.

All three controls use `--manifest`, `--slot`, and `--state-base` with existing
origin, default-slot, and environment-precedence rules. The existing non-store
escape hatch remains framework-test-only. New operands are:

| Control | Operands and validation |
| --- | --- |
| `context` | Common operands only. |
| `observe` inspection | Exactly one of `--current` or `--run-id <id>`. |
| `observe` awaiting a task | The same selector plus `--until-step <stepPath>` and optional `--wait-timeout-ms <positive u64>`. |
| `ensure` | Required `--task <id>` and `--until-step <stepPath>`; optional `--wait-timeout-ms <positive u64>` and existing `--timeout-ms <u64>` runtime operation timeout. |

Each new value operand and selector occurs at most once. Observation timeout
defaults to 60000 ms and is accepted only when waiting. `ensure`'s operation
timeout defaults to the existing run default of 5000 ms. It remains distinct
from task deadlines and observer timeout. None accepts `--daemon` or `--output`.
Help exits 0 before admission; otherwise success emits one JSON value on stdout
and failure emits the current structured `RuntimeError` on stderr. Diagnostics
and captured workload output are read from returned evidence paths.

### Resolved context

`context` accepts the existing manifest, slot, and state-base selection operands.
It returns `projectId`, `environment`, `slot`, `stateBase`, `stateRoot`,
`registryDir`, and `registryPath`. It requires no registry or live owner and
does not resolve source material or secrets.

Resolve relative state bases against the command's invocation directory through
the same native path owner used by execution. The result describes selected
coordinates, without claiming canonical filesystem identity for nonexistent
paths. Preserve non-UTF-8 paths internally; JSON projection rejects paths it
cannot represent rather than replacing bytes. System path aliases remain
subject to the existing descriptor-based execution checks.

Separate slot coordinates from run-specific evidence paths in the native
placement types, so a context query needs no invented run identity. Host paths
remain absent from the manifest. PostgreSQL data subdirectories and application
target formats remain adapter or application choices.

### Session observation

`--current` selects the newest unfinished run once in a checked snapshot,
including absence. It may select a dead or finalizing run for honest inspection;
it never follows later publication. An exact run selector also permits
historical inspection. Plain inspection reports provenance, recorded progress,
and owner/service leader observations, without interpreting another manifest's
root plan or exposing a general event API.

Before interpreting `--until-step`, require the supplied raw manifest hash to
equal the selected run's hash, then derive its recorded root plan. The step must
name one root leaf. Registration writes its `root_step_path` to the canonical
task process row before workload release. The nullable field means exactly
"this process has no root occurrence attribution" when absent, as with
preparations, services, and probes. Enforce non-null attribution only for task
rows and uniqueness of `(run_id, root_step_path)` when present. Derive `taskId`
from that root plan; `processKey` already identifies the execution occurrence.

Awaiting requires canonical task status and execution outcome both `succeeded`,
a present policy-accepted exit code, settled process ownership, and complete
capture. The run must have no committed execution outcome. Each service in the
root's entire derived group must have canonical status `ready`, unresolved
ownership, and no execution outcome, with an identity-checked live leader.
This includes transitive and preparation-only services. The recorded service
set and endpoint coordinates must match the admitted plan. A terminal service
row with a still-live process cannot satisfy the predicate. Do not substitute
`ps.live`: it can remain true because descendants survive a dead leader.

Read attribution, outcomes, capture, exit codes, and paths from typed structural
records. Never recover identity from source labels, filenames, command strings,
or redacted event payloads. Existing redaction can rewrite JSON keys and values,
so a redacted `TaskRun` is evidence for humans, not a canonical decoder input.
The new output schemas keep fixed keys; their projections emit structural
non-secret facts and exact native paths to already-redacted evidence without
resolving secrets. A locator is never reconstructed from a redacted string.

Root attribution proves that the startup loop completed readiness and initial
health before that root task's execution. No additional health table, startup
flag, node-completion state, or phase-event API is required.

After collecting owner and service leader samples, open a final fresh checked
snapshot. Revalidate the exact task predicates, absent run outcome, unchanged
active service process identities, and absence of successor publication. Take
a final owner identity sample and require it live before returning. Newly
visible settlement, replacement, or invalid stored relationships reject.

The observer deadline starts after admission, before enrollment, and covers
selection waits, bootstrap reply waits, and task observation without resetting.
It bounds cancelable waits; host I/O and scheduling can add latency. A timeout
or cancellation ends only this command. A selected absent run fails awaiting
immediately. Inspection of absence or historical failure still exits 0.

Fresh coherent snapshots are opened between waits. Retaining the current
`RegistryReader` transaction would hide subsequent progress. A file's existence
or a process's successful exit is insufficient evidence: task summaries are
written before the terminal transaction, and capture can fail after exit.

Return historical task evidence and current leader observations separately.
Task terminal success precedes the executor's post-task checkpoint, so this
result can precede node rejection or holding-task startup. A missing committed
run outcome is not proof that its execution thread still runs graph work: an
outcome-write failure can coexist with teardown. The observation does not
reproduce the owner's capture-worker or descendant-containment checkpoint.
Current identities and liveness are independent samples, without an atomic
multi-process, supervision, or future-availability guarantee.

Once run A is selected, known execution settlement, owner death, or replacement
ends the wait; it never follows run B. Consumers perform present connection and
application admission after reading the returned task evidence.

### Output and failure contract

The following are closed proposed records. Names refer to their owning native
typed projections; no record contains arbitrary registry payloads or a
domain-specific target. Optional values describe genuine absence.

| Record | Fields |
| --- | --- |
| `Context` | `projectId`, `environment`, `slot`, `stateBase`, `stateRoot`, `registryDir`, `registryPath`. |
| `Request` | `computedManifestHash`, `rootTask`, `source`, `operationTimeoutMs`; `source` is the existing admitted-source record, decoded by its native owner. |
| `Leader` | `pid`, nonempty opaque UTF-8 string `startIdentity` from the existing native process-identity codec, and tagged `observation`: `live`, `absent`, or `identity-mismatch`; service leaders also validate their recorded process group. A published owner cannot have a missing/null start identity. Uninspectable identity is an error. |
| `ServiceLeader` | `serviceId`, `processKey`, `leader`, `endpoints`; endpoints are immutable recorded `{endpointId, host, port}` coordinates, not fresh listener or health witnesses. |
| `Progress` | `outcome-pending`, `finalizing {outcome}`, or `finalized {outcome}`, corresponding exactly to recorded execution/finalization facts; `outcome-pending` means only an absent committed execution outcome. |
| `Session` | `runId`, `manifestPath`, `request`, `progress`, `runDir`, `logsDir`, `owner`, `services`. |
| `CompletedRootTask` | `stepPath`, `taskId`, `processKey`, `exitCode`, `stdoutPath`, `stderrPath`; construction requires the checked success, settled ownership, and complete capture above. |
| `TaskObservation` | `context`, `session`, `task`; construction requires the successful await predicates above. |

Inspection returns `{result: "absent", context}` or
`{result: "session", context, session}`. Awaiting returns
`{result: "satisfied", observation: TaskObservation}`. `ensure` returns
`{disposition: "established" | "observed", observation: TaskObservation}`.
Endpoint-less services have an empty endpoint list. Accepted nonzero task exit
codes remain accepted. No separate occurrence ordinal or task-summary decoder
is introduced.

Four new error codes use exit class `error`:

| Code and exit status | Closed reason or selection |
| --- | --- |
| `SLOT_BUSY` 39 | The selected slot's exclusive authority is contended; never implies a reusable run. |
| `REQUEST_MISMATCH` 40 | `field` is `manifest`, `root-task`, `source`, or `operation-timeout`; report identifiers and field name, without secret values. |
| `OBSERVATION_TIMEOUT` 41 | `stage` is `selection`, `establishment`, or `task`; retain the fixed watermark or selected `runId` and step as applicable. |
| `SESSION_UNAVAILABLE` 42 | `reason` is `absent`, `owner-gone`, `owner-replaced`, `service-gone`, `service-identity-mismatch`, `service-settled`, `execution-settled`, `task-failed`, `evidence-incomplete`, or `authority-released`; include selected `runId`, `stepPath`, and evidence paths when known. |

Selector syntax and duplicate operands use `MANIFEST_ADMISSION`; unknown tasks,
invalid root steps, and invalid `ensure` continuation use
`TASK_SELECTION_INVALID`. Waiting with a different manifest uses
`REQUEST_MISMATCH`. Observer signals use `CANCELED`; uncertain bootstrap replies
use `LIFECYCLE_FAILED` with the allocated run ID and explicit uncertainty.
Admission, registry, filesystem, identity-inspection, and output-projection
errors keep their existing classifications. An accepted task with incomplete
evidence can never satisfy waiting.

The observer does not promise to reproduce an unavailable original error. Run
records currently store outcomes, while redacted lifecycle diagnostics are not
complete structured errors. Unavailability reports available native facts and
evidence paths; it neither invents a readiness timeout nor parses diagnostics
to reconstruct causes. Original execution, containment, capture, finalization,
and cleanup outcomes retain their distinct records and meanings.

### Declared request matching

Every run records its selected root task and runtime operation timeout at the
existing `run.created` commit, alongside manifest hash and admitted source.
These immutable facts are owned by the run row. Do not create a service table,
configuration hash, reusable service identity, or required sidecar.
Operation timeout retains its full `u64` range, including zero as currently
accepted; the registry codec must not narrow it to SQLite's signed integer range.

For reuse, match the slot's checked registry identity, raw manifest hash,
selected root task, the existing admitted-source fields, and operation timeout.
Observer deadlines and requested observation steps are caller-local
and do not alter the owner request. A different manifest is a mismatch even if
its changes appear descriptive. Broader semantic equivalence is out of scope.

Matching establishes the same declared request, not equivalence of changing
external inputs. Live-workspace files may have changed under the same admitted
root. Resolved secrets remain those already admitted by the owner. `ensure`
does not rotate credentials, reload files, or compare persisted secret hashes.
Applications requiring a fresh configuration use explicit shutdown and a fresh
run. Client application admission remains mandatory where its domain needs it.

`ensure` uses full run admission before execution-capable startup or reuse.
Missing current source or secret material therefore rejects `ensure` without
touching an existing owner; `observe` can still inspect it. Changed available
secret values under the same descriptors do not make an existing request
different. A new owner records provenance from its own admission, not from an
earlier launcher observation. Its bootstrap compares the actual admitted
non-secret request against the launcher's admitted request before mutation;
source changes between those admissions reject rather than establishing a
different request.
`context` and `observe` use control admission and need no secret material.
The matching policy must not weaken existing source, ABI, target, closure,
containment, or state checks.

### Start or observe

`ensure --task <owner> --until-step <stepPath>` establishes a detached owner
request or observes a matching request, then uses the same
observation implementation as `observe`. Its timeout is an observer timeout;
the existing run operation timeout remains a separate execution-policy fact.
The selected task must be a composite, and the waited leaf must not be last in
its canonical root execution order. This is a syntactic continuation check;
neither task timeout absence nor graph structure proves indefinite execution.
The application authors a dependent holding task and owns that intent.

Matching ordinary foreground and daemon owners are eligible. An observer gains
no control over their original launcher: that launcher's cancellation, explicit
`down`, normal completion, or failure can end them. Only a new owner created by
`ensure` receives the shared-establishment cancellation purpose below.

After admission, enroll in one coherent validated snapshot. Freeze `H`, the
latest native event sequence there, before any acquisition attempt. Use
`run.created` event types and their structural `run_id` columns for creation
order, never redacted payloads. The snapshot is the logical beginning of this
operation's coalescing; a run completed before it is a predecessor even if its
CLI overlapped this caller's admission.

Enrollment permits at most one live unfinished owner; contradictory multiple
live owners reject as registry corruption rather than choosing one arbitrarily.

Validate every creation marker's run reference and uniqueness; a malformed first
marker or duplicate creation marker is corruption, never a reason to join/filter
past it. Busy reads before enrollment wait without inventing or advancing `H`.
Genuine absence has `H = 0`. Serialize the non-secret request and context before
bootstrap dispatch; preserve the existing 1 MiB framed request limit. Helpers
return bounded run selection, while task output remains in captured files.

| State | Checked observation | Action |
| --- | --- | --- |
| Enrollment | An unfinished run with a live exact owner is published | Pin it, including a known settled execution outcome; matching or awaiting then rejects unavailable progress as appropriate. |
| Enrollment | Only completed runs or definitively dead predecessors exist | Enter `Seeking(H)`; live orphan services remain recovery obligations. Identity uncertainty is an error. |
| Seeking | At least one `run.created` has sequence greater than `H` | Pin the first such run, even if already failed, finalized, dead, or incompatible; never skip to a later match. |
| Seeking | No post-`H` publication and slot authority is busy | Refresh snapshots and wait cancelably. |
| Seeking | Slot authority is acquired | Under that authority, repeat the same first-publication query using the original `H`. |
| Acquired | A post-`H` publication exists | Release authority without recovery or new creation. Pin that run and report its terminal/unavailable result; acquisition proves it has already released authority. |
| Acquired | No post-`H` publication exists | Perform ordinary predecessor recovery and establishment of at most one new run. Its `run.created` commit fixes this observer to that run. |
| Pinned | Any subsequent state | Observe only that run. Never acquire authority, recover, retarget, or restart it. |

Preserve `H` through polling and detached-bootstrap dispatch. The launcher
never holds mutation authority; the existing hidden owner performs the guarded
decision and returns either a new establishment or a selected competing run.
A mismatch never cancels or recovers the selected run. An acquired guard is
released before returning to read-only observation.

Registry initialization currently exposes SQLite before schema commit. Add a
native read result distinguishing absence, exactly the already-admissible empty
unversioned bootstrap state, a complete checked snapshot, and narrowly retryable
SQLite busy/locked results. Retry only those explicit transient states within
the observer deadline; malformed or unsupported schema remains terminal.
An empty bootstrap snapshot has `H = 0` and no published run. Plain `observe`
reports absence with its fixed selection; only `ensure` seeks later publication.
Never catch arbitrary `REGISTRY_CORRUPT` as startup contention.

Corrupt or unsupported registry bytes, inspection uncertainty, request mismatch,
and failed admission reject; none is evidence of an idle slot. A selected run's
startup failure never triggers an automatic retry or restart. A later explicit
invocation may start a fresh owner after normal recovery.

### Cancellation and shutdown

The detached launch request uses a closed purpose type distinguishing ordinary
daemon launch from shared establishment. Both use the same admitted owner and
exclusive session executor.

Before commit, observed launcher abandonment may prevent establishment. At the
run-record commit, a shared owner becomes independent of all observers. Closing
or interrupting any observer after that point never writes to the owner's
cancellation FIFO, even if that observer initiated establishment. An uncertain
reply reports the allocated run identity and uncertainty, without inventing
success or automatically canceling a possible shared owner.

Ordinary `run --daemon` preserves its existing interruption behavior. The new
purpose is explicit rather than silently changing all background launches.
There is no persistent observer, waiter registry, refcount, last-client stop,
automatic restart, or new resident coordinator.

`down` retains select-once cancellation and authority-handoff proof. Explicit
shutdown can stop the owner while clients exist. `clean` retains exclusive
recovery, confinement, process, generation, and retention gates. Waiting neither
authorizes cleanup nor repairs stale ownership.

## Application composition

An owner is an ordinary static composite:

```text
owner root
  initialize  finite application task requiring the owned service group
  hold        long-lived ordinary task depending on initialize
```

The service startup loop completes preparation, start, readiness, and initial
health before `initialize`. An exit rejected by its task policy is a task failure,
without readiness retries. `hold` preserves the session lifetime through the
existing algebra; its eventual exit or failure triggers normal finalization.
No manifest startup flag, new lifecycle phase, or new semantic kind is needed.

Power Sim packages a Rust launcher as an ordinary custom Nix app with its pinned
runtime, manifest, and client executable. It invokes the runtime from the
intended project source root. Both launch paths call
`ensure --task owner --until-step owner.initialize` for the dedicated owner
slot. The launcher reads the returned captured stdout path, parses Power Sim's
target format, connects, and validates its expected database/schema. It then
directly spawns the client with explicit arguments and environment. Cancellation
reaches that client only; the launcher never calls owner `down` on client exit.

Output is redacted evidence; credentials must not become target metadata. A
named target file located through `stateRoot` is also application data, requires
current application validation, and can never authorize runtime ownership or
cleanup. Prefer the exact captured initialization output to avoid accepting a
file left by a different process generation.

Clients run outside the owner session. If clients are themselves Nixfied runs,
they use distinct slots or project contexts and receive the external target
through ordinary declared configuration. Their `${stateDir}` and endpoint
placeholders remain local to their own declared dependencies. This RFC adds no
dynamic command arguments, inherited child environment, borrowed service edges,
or external dependency kind. A project needing per-invocation dynamic target
injection into an existing generated task app needs a separate authoring design.

The same composition applies to a database, queue, indexer, or endpoint-less
worker. A service without endpoints returns no invented address. A fixed group
has one shared owner and shutdown boundary; independently managed groups use
different slots.

## Alternatives considered

| Alternative | Reason to reject |
| --- | --- |
| Only context and observation | Leaves each caller implementing startup coalescing and bootstrap cancellation semantics. |
| Only ensure | Cannot discover placement or inspect an existing run without current source and secret admission; the three controls share native projections and waiting. |
| Delay daemon acknowledgement until ready | Establishment and readiness are different facts; application initialization can occur after initial health. |
| Treat every occupied slot as reusable | Occupancy identifies neither the requested task nor successful startup. |
| Treat `ps` ready as startup completion | Ready precedes health and survives later listener loss while the process remains live. |
| Move schema initialization into a probe | Probe retry semantics obscure permanent rejection and repeat potentially mutating initialization. |
| Add an adopter lock and native-registry parser | Duplicates lifecycle coordination and native evidence decoding across projects. |
| Hash service configuration into a new registry | Creates another identity authority while missing selected root behavior and mutable application inputs. |
| Share ownership or count clients | Changes service lifetime, recovery, cleanup, and graph semantics beyond observation. |
| Add a persistent coordinator or dynamic service graph | Unnecessary for a single-host slot with existing exclusive authority and durable session evidence. |

## Required proofs

| Boundary | Independent acceptance and rejection cases |
| --- | --- |
| Placement | Explicit base, environment precedence, empty overrides, Linux/macOS defaults, relative cwd, non-UTF-8 projection, occupied and absent slots; query leaves application and registry state unchanged. |
| Admission | Invalid task, root step, final-in-plan continuation, stdin, source, closure, target, ABI, and secret reject before workload release. |
| Request matching | Same declared request coalesces; different manifest, selected task, source root, or operation timeout rejects without touching the owner; changed available secret values reuse prior owner values, while missing current source/secrets reject ensure and leave inspection possible. |
| Exclusion | Many simultaneous successful observers name one established run and exactly one service group; ordinary competing `run` reports `SLOT_BUSY`. |
| Publication races | Barriers during SQLite schema publication, recovery, before run commit, after commit before reply, and between read and acquisition; preserve H across bootstrap dispatch; choose the first of multiple post-H publications even when it failed and a later matching owner lives. |
| Cancellation | Before commit an abandoned attempt may disappear and other seekers progress; after commit cancel any waiter including the initiator before reply, leaving established owner and other observers unaffected; never cancel a successor. |
| Evidence | Delayed/failed initial health, permanent initialization failure, accepted nonzero exit, partial summary, capture failure, failed terminal commit, repeated task ids, preparation/root collisions, vocabulary-matching redaction, and snapshot refresh. |
| Observation limits | Task success followed by node-checkpoint rejection, outcome-write failure during teardown, terminal service with a live leader, surviving descendants after leader death, and settlement during leader sampling never produce stronger executor or leader claims. |
| Owner failure | Dead pre-H predecessor with live orphan services recovers; dead post-H run is pinned failure; hold failure, stopped owner, identity reuse, ordinary-launcher cancellation, and successor replacement have explicit outcomes. |
| Service scope | Scalar endpoint-less probes, one endpoint, multiple endpoints, and a service group preserve current endpoint and containment guarantees. |
| Shutdown | Explicit `down` settles selected owner processes and respects successor handoff; active clients grant no retention or cleanup authority. |
| Adopter acceptance | Both startup entrypoints race through one path; two clients run concurrently, cancel independently, and reject stale or incompatible application targets. |

Use Rust process/socket fixtures for deterministic runtime races and literal wire
cases for outputs and errors. Use independently authored Nix and Rust vectors
for any changed graph facts. Generic fixtures prove the feature without a
database; real PostgreSQL adopter tests additionally prove the motivating use
case. Linux and macOS must both exercise owner identity and cancellation.

## Contract cutover and delivery

Implementation requires one coherent replacement contract. Update the capability
inventory, output and command declarations, ABI digest snapshot and constants,
native parsers, generated apps/help/reference, Rust serialization, fixtures,
contract, guide, and architecture together. Add immutable request provenance to
the existing registry and bump its schema version; reject old registries without
an in-place migration or historical event rewrite. The manifest shape and marker
version need no change merely to add runtime controls.

Implement in ordered logical commits that each leave a coherent current tree:

1. Define the public vocabulary and native placement/observation projections,
   immutable run/root attribution, typed contention, and independent boundary
   proofs.
2. Compose shared establishment with run-pinned observation, request matching,
   explicit launch purpose, and concurrency/cancellation proofs.
3. Publish the generated controls and adopter-shaped generic acceptance cases,
   replace Power Sim coordination through its own pinned update, and run its
   database/client acceptance tests.

Each public addition rotates the ABI as required by the current delivery rules;
do not publish a command with incomplete semantics while staging internals.
Follow [development verification](DEVELOPMENT.md): narrow focused checks,
fixture-backed `.#test`, then `.#ci` for cross-layer delivery and a downstream
gate with `--dirty` when testing uncommitted framework changes. Before upgrading,
use the matching old runtime to stop the old owner and preserve its state.
Use a fresh state base for the new contract, with application-owned export and
import when existing data must survive. Old registry or ABI-bound marker bytes
are not admitted merely because shutdown completed; copying or rewriting markers
is not adoption. No compatibility fallback or historical rewrite is introduced.
