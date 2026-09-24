# RFC: simplify ownership and remove duplicated layers

Status: draft. Session-owned service processes, a single persistence policy for data retention,
one active session per slot, interrupted-session cleanup followed by a fresh
start, independent compilation/presentation consumers of native definitions,
structural type generation limited to shared wire meaning, runtime derivation
of execution graph facts without carried duplicate answers, executable
dependency identity based on selected outputs and executables, help scoped
to Nixfied-generated apps, per-session FIFO cancellation for `down`, and removal
of framework-owned application-data compatibility checks are
accepted architectural direction. Remaining mechanisms and other solutions below
remain proposals unless explicitly marked accepted.

Started 2026-09-24. Problem baseline:
[PROBLEM_DUPLICATION_LAYERS.md](PROBLEM_DUPLICATION_LAYERS.md), reviewed against
`a889fa3`. This RFC owns the evolving solutions; the problem document preserves
the diagnosis and evidence. No runtime behavior is changed by writing this RFC.

[CONTRACT.md](docs/CONTRACT.md) remains the current behavioral authority until
each accepted change is implemented as a coherent contract cutover. This RFC
does not authorize silently weakening containment, cleanup, or redaction.

## 1. Accepted product direction

**A session owns all its service processes. State policy independently determines
whether application data survives that session.**

Services do not remain standing for later invocations to borrow. Tasks inside a
session may share its services. Finishing a leaf task does not end the session or
stop a dependency still needed by the rest of its task graph.

| `nixfied.state.persistence` | Processes when the session ends | Framework-owned slot data |
| --- | --- | --- |
| `persistent` | Stop | Retain until explicitly purged after ownership and safety checks. |
| `run-scoped` | Stop | Automatically delete after safe process teardown. |

For PostgreSQL with persistent state, the runtime stops PostgreSQL at session end
and leaves its database directory in the slot. A later session starts a new
PostgreSQL process against that directory. With run-scoped state, successful
finalization removes the application state so the next session starts fresh.

This retention promise concerns data under the framework-owned slot state root.
It does not extend cleanup ownership to arbitrary files written elsewhere. The
current setting is slot-wide; this RFC does not introduce per-service retention.
Logs, summaries, registry history, and application state are distinct resources.
Deleting run-scoped application state must not indiscriminately delete run evidence.

Today, run-scoped state is eligible for ordinary manual cleanup. Automatically
cleaning it at session end is a behavioral change, not just renaming a lifetime.

Cross-session process reuse and its warm-start convenience are deliberately lost.
Persistent application data remains supported. Long-running development services
require a live owning session rather than detachment.

**One active session exclusively owns a slot through finalization.** A competing
session targeting that slot rejects before application-state mutation or child
spawn, even if it uses the same manifest. Concurrent sessions use different slots.
Tasks within the owning session still share its services.

The ownership sequence is: acquire the slot, recover any interrupted predecessor,
run the session, stop its processes, retain or safely delete application data,
record the final outcome, and release ownership. Persistent data can remain after
ownership is released; persistent processes cannot become available for borrowing.
Recovery failure refuses new execution rather than treating the slot as ready.

Explicit cleanup and state-changing recovery use the same exclusive
ownership boundary. They cannot independently mutate application state while a
live session owns it. This decision accepts the ownership model, not a particular
lock primitive or filesystem/registry transaction protocol.

**Recovery is cleanup followed by a fresh start, never adoption of surviving
services.** If a service fails while the runtime remains alive, the runtime stops
the other session processes, applies the data policy, and releases the slot. If
the runtime itself dies, the next invocation performs interrupted-session cleanup
under exclusive slot ownership before starting its own session. Unsafe or failed
cleanup refuses new execution and reports the unresolved state.

Persistent application data stays; run-scoped application data is cleaned after
process teardown. Service-specific startup recovery belongs to the service, such
as PostgreSQL recovering its own database. Nixfied does not repair that database
or delete persistent data merely because startup fails.

**The application and user own data compatibility.** Remove `stateEpoch` and its
mismatch/upgrade machinery. After ownership and recovery checks, Nixfied starts
the configured executable against retained persistent data, reports startup
failure if any, and safely finalizes the session without deleting that data.
Configuration changes do not trigger framework-managed migration or data reset.

**Native definitions feed compilation and presentation independently.** Compilation
uses native bindings directly and validates executable intent. Documentation and
help consume the shared definitions and validate their own presentation
relationships. Release checks require both consumers to pass. A broken topic
reference must fail documentation checks without blocking an otherwise valid
manifest solely because compilation obtains its bindings through that reference.

## 2. Scope and solution map

| Problem | Proposed solution | Main implementation owner |
| --- | --- | --- |
| F1: package-name collisions | S1: derive synthesized closure identity from actual selected outputs and executable selection. | Nix compiler |
| F2: teardown loses containment policy | S2: one session owner throughout startup, execution, teardown, and recovery; remove persistent handoff. | Runtime session/process control |
| F3: cleanup races execution | S3: exclusive slot mutation authority shared by acquisition, recovery, and cleanup. | Runtime slot/state ownership |
| F4: partial cleanup loses its marker | S3: durable deletion intent tied to an identified tree, with explicit interrupted-transition recovery. | Runtime state cleanup |
| F5: pathname checks race traversal | S3: directory-identity-based traversal with non-following operations. | Runtime filesystem boundary |
| F6: manifest replacement defeats service reuse | S4: remove service reuse and epoch-driven replacement; retain ownership/retention checks and leave data compatibility to the application/user. | Runtime identity/state admission |
| F7: presentation gates compilation | S5: native definitions feed independent compilation and presentation consumers. | Nix modules/metadata/docs |
| F8: schema controls private Rust representation | S6: restrict generation to shared wire structure; Rust owns internal representation. | Manifest schema and native Rust modules |
| F9: repeated facts and inert policies | S7: remove redundant wire derivations and non-operational fields; retain explicit runtime validation. | Compiler/manifest/runtime lowering |
| F10: help restricts module integration | S8: construct help from generated apps rather than probing the final caller flake. | Project apps/help |
| Compatibility and incidental CLI behavior | S9: keep compatibility claims explicit and avoid unrelated protocol expansion. | ABI/constants/native parsers |
| Registry coordination left over from shared services | S10: remove the sharing protocol; retain session evidence, explicit recovery obligations, and cleanup history. | Runtime registry/session/control |

The session decision also addresses the polling/containment concern underlying
F2. Session ownership alone does not prove containment; S2 must establish the
remaining host guarantees before runtime implementation is considered complete.

## 3. S1: preserve executable dependency identity

Accepted: synthesized tool identity comes from the actual selected store output
and executable selection. Package names are readable labels, not unique identities.

**Invariant:** each invocation uses the package selected by its author.

Replace `tool-${lib.getName package}` as the sole synthesized identity. Use a
deterministic identity derived from the selected store output and executable
selection, retaining a readable label if useful. Identical closure inputs may
deduplicate; different outputs must never silently collapse. Check any remaining
key collision against the actual identity instead of trusting a label or digest.

Two versions or overrides with the same package name must remain distinct when
their selected outputs differ. Repeated use of the same output and executable may
share one synthesized entry. No global map insertion may silently choose one of
two unequal identities and rewrite both invocations to it.

Explicitly named closures remain supported references. Compiler construction owns
collision rejection before manifest emission. Do not add a package alias registry
or a second resolution path to compensate for collisions.

The resulting representation cannot map two unequal closure identities to one
accepted synthesized entry. Exact key spelling and its effect on operation
bindings must be specified during implementation.

**Proof:** compile two same-name package outputs and assert their selected
executables and PATH roots independently; verify repeated identical packages
deduplicate and explicit/synthesized collisions reject. Execute distinguishable
tool fixtures so a self-consistent but incorrectly rewritten graph cannot pass.

## 4. S2: make the session the process lifetime owner

### Session boundary

One runtime `run` execution is one session covering its selected task graph,
prepare tasks, services, probes, child output, and finalization. Long-running tasks
use that same boundary. Optional background placement is discussed below; no
separate supervisor or arbitrary runtime scheduling is introduced.

One owner retains process handles and teardown responsibility for the whole
session. Services can be shared inside that owner, but there is no transfer to
standing ownership and no borrower lease from another session.

Remove `serviceLifetime`, `until-idle`, `persistent-until-down`, cross-session
borrower acquisition, standing commits, and detached output handoff. Remove their
wire fields and branches rather than retaining aliases with identical behavior.
Audit registry records individually: durable execution evidence and interrupted
session recovery remain necessary, but borrower coordination no longer justifies
their own tables or transitions.

### Task-owned session duration and optional background launch

Accepted direction: retain existing task entrypoints. Tasks are the selected work
that determines session completion; services are dependencies whose lifetime is
owned by that session. A task may be finite work or a long-running user program.
Do not force an HTTP server into the service category merely because it runs
indefinitely.

```sh
nix run .#serve  # Start PostgreSQL/Redis dependencies, then the user's HTTP task.
nix run .#down   # From another shell: cancel that same session.
```

The foreground invocation remains alive while the task graph executes. Task
completion or failure, service failure, or cancellation enters the common
finalizer, which stops the remaining processes and applies retention. Services
do not become reusable detached processes. A second framework execution on the
same slot still refuses; external clients may connect without acquiring ownership.

Withdraw the proposed `--service` selection, service-root mode, and related
selection/output extensions. No third manifest kind, service bundle, artificial
sleep task, task injection, or attachment protocol is needed.

**Invariant:** foreground or background placement does not change task execution,
slot ownership, cancellation, retention, or recovery. One live runtime owns the
whole session until finalization.

**Accepted: task invocation `timeoutMs` is optional, with no finite default.**
Omission means run until the task exits, fails, or the session is canceled. An
explicit positive millisecond value imposes a finite invocation deadline. Do not
substitute 30 seconds, a huge sentinel value, or zero for an absent deadline.
Nix defaulting, manifest emission/decoding, lowering, and execution must preserve
that absence rather than reconstructing a hidden finite timeout.

The invocation executor owns finite deadline enforcement; declaration and wire
validation reject invalid supplied values before execution. No deadline never
means uncancelable: service failure, Ctrl-C, and FIFO `down` still stop the work
through the session finalizer. Foreground and background execution use the same
rule, with no task-name-specific exceptions.

Keep readiness, probe attempts, and teardown bounded by their own lifecycle
limits; specify preparation's relationship to those limits explicitly. The shared
invocation representation must not erase a probe's enclosing deadline. Runtime
operation timeouts must not become an implicit task or session deadline. This
decision adds no composite-wide deadline or timeout inheritance policy.

Observe every owned service while the task graph or preparation runs, including
transitive and prepare-only dependencies. Unexpected service failure cancels the
remaining work and reaches the one finalizer. Preserve actual failure evidence
when cancellation follows it; intentional teardown exits are not fresh execution
failures. Reuse readiness and startup-health behavior without silently introducing
periodic application-health scheduling.

**Optional proposal:** `--daemon` on existing task entrypoints could launch the
entire runtime session in the background and return the shell:

```sh
nix run .#serve -- --daemon  # Proposed; not currently implemented.
nix run .#down
```

This is background placement of the same session owner, not a separate persistent
supervisor, cross-session service lifetime, or service ownership transfer. Task
exit still ends the session. Owner death still invokes the accepted recovery path.

Do not equate this optional mode with merely appending a shell ampersand. Specify
terminal/session detachment, standard-input policy, and redacted output destinations
around the accepted acknowledgement boundary below. In particular,
launch acknowledgement cannot claim that a long-running task completed. Background
children must not retain caller-terminal streams, and unsupported interactive
input must reject before workload execution. The exact flag/handshake semantics
remain proposed; do not build attach, restart, or general daemon-management APIs.

### Accepted background-launch acknowledgement and result identity

For background execution, launch success and task success are distinct outcomes.
The launcher returns success only after the background session owner has passed
admission, acquired exclusive slot ownership, completed predecessor recovery,
committed its session identity, established the cancellation endpoint, and set up
redacted output capture independently of the caller's terminal.

Return that immutable session ID and its evidence location. This acknowledges an
established session, not service readiness or task completion. Service startup and
task execution may proceed in the background; subsequent failures belong to the
selected session's final outcome. The rule applies equally to `serve`, `ci`, and
other task entrypoints, without task-name-specific branches.

Invalid configuration, occupied slot, failed recovery, or failed session/output
establishment prevents successful launch acknowledgement. A later test failure
or service-readiness failure is a failed session result. Owner death leaves an
unfinished session until recovery settles its obligations; it is never inferred
to be successful because the launching command returned zero.

Every final result is attributed to the immutable session ID. A newer occupant
of the same slot cannot replace the result being observed. The owner/finalizer
records execution outcome separately from finalization completeness as specified
in S10. Slot selection remains useful for live control, not as durable result
identity.

The acknowledgement transport, launcher-death behavior during handoff, and exact
output schema still need implementation specification. Do not add status commands
or a result-query protocol here; those public API details remain with the deferred
API audit. No acknowledgement can guarantee the owner remains alive afterward.

**Proof:** reject every pre-acknowledgement failure; report an established session
without claiming readiness or task success; preserve later startup/task failures;
retain unfinished evidence on owner death; resolve results by the original ID
after successor startup; preserve terminal-independent redacted output. Validate
the actual launch handoff on Linux and macOS.

### Session execution cutover and proof

Implementation must update the existing task timeout contract, lowering/execution,
and coupled authoring/wire/docs/tests atomically. Optional daemon launch adds only
its justified command/control surfaces after those semantics are specified.

**Proof:** omission survives authoring-to-runtime transformations and a task can
outlive the removed 30-second default; explicit finite deadlines still work;
invalid supplied values reject before effects; no-deadline tasks remain cancelable;
probe/shutdown limits remain effective; its declared services
remain owned until task completion/cancellation; service failures reach the owner
during all phases; Ctrl-C and FIFO `down` converge on finalization; same-slot
conflicts refuse; both retention modes and owner-death recovery remain correct.
If background launch is adopted, prove equivalent session ownership plus terminal
independence, checked startup failures, truthful launch acknowledgement, and
preserved redacted evidence on Linux and macOS.

No runtime behavior has changed by this specification.

### Proposed session-wide supervision

Status: architect specification for review. The accepted requirement is that
service failure stops the session; the mechanism and output-order adjustment
below remain proposals, with implementation and platform proofs outstanding.

**Invariant:** every direct child has one live owner and one reaper; every owned
service remains observed throughout startup, task execution, and finalization.
Typed observations feed one lifecycle owner. Observation failure stops further
work and preserves unresolved obligations; it never means an empty process tree.

#### One owner, one observation cycle

Use one private session supervisor owned by `RunSession`, driven by the existing
execution thread. Consolidate child ownership into its single in-memory collection:
`Child` handles, immutable process identities, role, containment/stop policy,
lifecycle phase, descendant evidence, and checked capture-worker results. Do not
copy that collection into a second liveness registry or background monitor.
The SQLite registry remains durable recovery evidence, not a live process owner.

Only the supervisor reaps direct children. Task, probe, readiness, and stop helpers
operate through its checked wait/observation operations rather than independently
calling `try_wait` on the same child. Keep the existing sequential static task DAG;
add no actor runtime, generic event bus, parallel scheduler, or new daemon.

Represent roles explicitly: service, task occurrence (including preparation),
and lifecycle/probe invocation. Gated-launcher/running/stopping/reaped are lifecycle
phases, not another semantic workload kind. Use private transitions to prevent
release before registration and repeated reap/stop authority. A reaped child may
still have unresolved descendant or capture obligations; process exit alone is
not finalization.

The observation cycle polls all children and applicable containment evidence,
collects completed capture-worker outcomes, and returns typed facts. Check the
whole owned collection, not only the current task's direct requirements. This
includes transitive and preparation-only dependencies. Remove silent scanner
errors, poisoned-lock-as-empty fallbacks, and ignored joins from the old monitors.

| Observed result | Owner interpretation |
| --- | --- |
| Service exits before intentional stopping, including code zero | Service failure; stop admitting work and finalize the session. |
| Task succeeds | Complete that occurrence; the remaining graph determines what follows. |
| Task fails | Retain task failure and stop further graph execution. |
| Probe invocation fails | Failed attempt under the existing bounded retry policy. |
| Observation/containment cannot be established | Typed failure and unresolved process obligations; no further workload release. |
| Required capture fails | Typed output failure; finalize without discarding process ownership. |

Successful probes still require fresh service-liveness and endpoint-ownership
checks before readiness. This proposal adds no periodic application-health checks
or service restart policy.

#### Registration and execution checkpoints

Use the gated launcher for every child role. Keep its local `Child` handle during
startup, verify identity, commit the durable process record, and transfer the
owned launcher into the supervisor before sending the execution request. A
session checkpoint between registration and release prevents release after
cancellation or an already observed terminal failure. Failure to register live
ownership cannot be followed by workload permission.

A child may fail immediately after permission; observe it as an ordinary startup
result. Do not claim a checkpoint makes all later process failures impossible.

Every runtime-controlled wait must continue the same session observation cycle:
task completion (including no-deadline tasks), exec probes, retry delays, TCP
connection attempts, capture completion, shutdown/containment, and output replay.
Operation deadlines remain local to their operation and do not suspend observation
of other processes. Take a checkpoint before admitting another graph node and
before settling task/session success.

Replace blocking TCP probe waits with one nonblocking connection attempt driven
through bounded polls to its existing attempt deadline. Repeated short connection
attempts would silently change retry semantics and are not equivalent.

Document polling cadence and test responsiveness under controlled conditions.
Do not equate a ten-millisecond polling interval with a universal teardown bound:
scan cost, OS scheduling, registry I/O, and uninterruptible operations still matter.
Known runtime wait boundaries must cooperate; arbitrary kernel/filesystem stalls
are not solved by inventing another observer.

Keep existing capture/replay workers where streams need concurrent processing;
collect their results through checked completion polling. They own stream work,
not children, signals, the registry, or session finalization. The existing FIFO
receiver still only requests cancellation. It is not a second lifecycle owner.

#### Failure attribution and intentional stopping

Keep external cancellation separate from service/task failure, deadline expiry,
observation failure, and capture failure. Do not collapse internally observed
failures into the atomic user-cancellation flag and lose their cause.

On a terminal trigger, the owner stops new graph nodes and launcher releases,
retains observed facts, and enters finalization once. It settles task/prepare/probe
children and stops services in dependency-safe order while continuing observation.

Before intentionally signaling a child, observe pending exits and transition it
to expected stopping, then attempt durable stop-intent recording. A registry
failure remains a failure, but must not prevent containment and reaping attempts.
An exit already observed before the transition remains unexpected. An exit
observed afterward is classified against stopping intent, retaining its actual
status. Do not claim to reconstruct the physical ordering of an unobserved exit
and a concurrent signal. Services not yet marked stopping remain monitored for
unexpected failure during teardown of another service.

Preserve execution outcome separately from finalization failures/completeness.
Retain the current safety-first public error precedence unless separately revised:
a registry/containment/state failure may be the primary diagnostic while the
initiating service/task error remains a cause. Preserving an original failure
does not mean replacing the documented priority rules with first-event-wins.

#### Output backpressure and required ordering revision

Child exit does not establish capture completion. Descendants may retain writers;
workers may fail while the workload remains alive. Observe checked worker failures
and panics, and retain the existing bounded capture-shutdown requirements.

Current `ReplayTicket::replay` starts replay workers and synchronously joins them
before service teardown. An indefinitely blocked sink can therefore suspend
supervision and cleanup. A separate monitor thread would detect failure but would
not free that blocked owner to finalize.

Proposed revision: begin eligible replay while sources are retained, poll its
completion while observing the session, and allow process teardown to proceed
on failure/cancellation while projection remains pending. Preserve exact redacted
bytes, per-stream ordering, and checked evidence; never call incomplete projection
successful. This explicitly revises strict replay-complete-before-service-stop
ordering and requires a coupled output/lifecycle contract change.

The exact production sink interruption/progress mechanism remains open. Polling
a worker does not make an arbitrary blocked `Write` or its join interruptible.
Do not claim bounded finalization, release unresolved ownership, or silently detach
workers on that basis. Specify either interruptible production sinks or an honest
remaining completion limitation before implementation acceptance. A generic async
runtime is not a substitute for that decision.

#### Subtraction and proof

Replace rather than supplement per-service `ProcessMonitor` threads, independent
task/probe wait loops, service-local child reaping, direct-dependency-only ongoing
monitoring, and child-local aggregate run settlement. Main owners are `main.rs`,
`service/process.rs`, `service/task.rs`, `service/readiness.rs`, `redaction.rs`,
`output.rs`, and `service/registry.rs`. Keep `cancellation.rs` as the external
request mechanism, not a lossy container for all failure reasons.

A separate observation thread would require synchronized child state, checked
failure transport, and stop/join coordination while still needing interruptible
waits in the owner. Per-child wait workers add more of that coordination. Neither
is the proposed final architecture for the existing sequential executor.

**Required proofs:** service exit during another service's readiness/TCP attempt;
transitive/prepare-only dependency exit during preparation or no-deadline task;
role-specific exit-zero handling; unchanged probe retries; observer errors and
worker panics; cancellation before launcher release; task-success/failure races;
no new work after an observed terminal trigger; expected-stop classification;
failure of another service during ordered teardown; capture failure and retained
writers; projection backpressure without suspended process duties; exactly one
reaper; containment failure blocking data deletion; registry failures preserving
teardown and unfinished evidence; equivalent foreground/background supervision.
Preserve error precedence, redaction, retained data, and run-scoped cleanup.
Use deterministic barriers, then fixture-backed runtime and cross-layer checks
on Linux and macOS. No implementation tests were run for this specification.

### Finalization

**Invariant:** application state is not deleted while any session-owned process
may still use it. Success, task failure, timeout, and graceful cancellation all
enter the same finalization owner.

The proposed ordering constraints are:

1. Stop scheduling new work and settle active task/prepare/probe children.
2. Begin required output projection while retaining its sources and continuing
   session observation; pending projection must not prevent failure/cancellation
   teardown under the proposed supervision revision above.
3. Stop services in dependency-safe order; contain descendants and reap owned
   children. Stop probes and join capture workers as their owning processes end.
4. Apply application-state retention only after process quiescence is established.
5. Retain final evidence and cleanup outcomes, then release session ownership.

These are ordering constraints, not a new alternative output pipeline. The
supervision proposal explicitly revises replay/teardown ordering; preserve the
other replay/redaction guarantees and attempts to complete later cleanup
stages after an earlier failure. A cleanup failure makes finalization unsuccessful;
it must not erase the original task result or its evidence.

Model executable session phases through private construction and consuming
transitions where practical. Cleanup must require quiescence authority produced
by teardown or recovery, not a public boolean that callers can manufacture.

### Accepted recovery behavior and its limits

Distinguish service failure from runtime-owner failure. While the runtime is
alive, it follows normal finalization and releases ownership only after teardown
and the applicable state handling. A forcibly killed runtime cannot execute that
sequence. An owner-only OS lock may become available while children survive;
acquiring it permits recovery, not immediate execution or application-data deletion.

On the next run, the new owner inspects interrupted-session evidence, stops
surviving owned children as necessary, completes pending run-scoped cleanup while
preserving persistent data, and then starts a fresh session. It does not resume
the interrupted task graph, adopt a surviving service, or run an automatic restart
loop. If cleanup cannot complete safely, report the failure and refuse new
execution against the unresolved slot.

The design target is reliable cleanup for supported process behavior with explicit
failure when recovery cannot proceed. It is not proof against every possible
external interference. Runtime-owner death alone does not justify adding a daemon
or a general containment subsystem. Recovery may safely refuse rather than promise
automatic success in every case.

Normal teardown and recovery must still respect supported process ownership,
including children allowed to create other groups, and must not signal unrelated
processes based on a recycled PID. Specify handling of interruption between spawn
and durable registration; do not claim a process-record scan covers children that
were never recorded. These are implementation obligations for cleanup or refusal,
not reasons to reopen the accepted session model. Preserve the current contract
until any necessary guarantee revisions are explicitly specified and cut over.

`ps` may continue to inspect session and recovery evidence. A live-session `down`
uses the accepted per-session FIFO to request cancellation from the sole
finalization owner; recovery of a dead owner is a separate case. S10 specifies
the control mechanism and its required proofs before cutover.

**Proof:** ordinary shutdown of a separately grouped descendant, graceful
cancellation, abrupt runtime death, readiness failure, capture failure, retained
task evidence, and cleanup before a fresh session starts. Test service death with
a live runtime separately from runtime death with surviving services. Interrupt
startup before and after process registration, verify safe refusal where cleanup
cannot be established, and verify that persistent-data startup recovery remains
the child service's responsibility. Repeat applicable cases on supported platforms
and against the shipped runtime artifact.

## 5. S3: separate data retention from safe deletion authority

### Slot exclusion

Accepted: one active session owns a slot. A competing session rejects before
application-state mutation or spawn; parallel sessions use different slots. Two
independent invocations must never start PostgreSQL against the same slot's data
directory, including when their manifests are identical.

The session keeps exclusive authority through process teardown, state retention
or cleanup, and final outcome recording. Completion of the root task alone does
not release the slot. Explicit cleanup cannot pass a one-time idle check and then
delete concurrently with a newly acquired session.

Use one exclusion protocol for session acquisition, recovery, and explicit
cleanup. Its stable coordination object must live outside the
application tree being deleted. Preserve host endpoint coordination: a slot lock
does not prevent port collisions with another slot, state root, or host program.

The registry records evidence and recovery state. It does not independently grant
permission to mutate a slot already owned by another session. Choose the actual
lock/transaction arrangement in a focused design; do not create two competing
ownership authorities.

On abrupt owner death, an available lock is only permission to attempt recovery;
it is not proof of process quiescence. The successor must resolve surviving owned
processes and pending deletion before starting new work. If finalization fails and
the owner exits, retain durable unresolved evidence so another invocation recovers
or refuses the slot rather than executing against ambiguous state.

Slot exclusion coordinates Nixfied participants. It does not by itself prevent
another filesystem writer from replacing paths, so the deletion-identity and
non-following traversal requirements below remain necessary.

### Accepted: one persistence policy

`nixfied.state.persistence` is the sole application-data retention policy. Remove
`cleanupPolicy` and its `delete-on-clean`/`protected` alternatives rather than
maintaining precedence rules or rejecting combinations of overlapping controls.

| Persistence | End of session | Ordinary manual clean | Explicit purge |
| --- | --- | --- | --- |
| `run-scoped` | Delete after safe teardown | Allowed when idle and owned | Allowed when idle and owned |
| `persistent` | Retain | Refuse | Allowed when idle and owned |

**Invariant:** persistence determines whether deletion is permitted; runtime
ownership and process checks determine whether deletion is safe. Session
finalization supplies the automatic cleanup trigger. Explicit purge overrides
retention only: it cannot bypass slot exclusion, process teardown, filesystem
confinement, or ownership checks.

Represent the two retention alternatives directly so conflicting policy pairs
cannot be constructed. Nix declarations and the wire decoder reject the removed
option/field; do not accept an ignored compatibility setting. Runtime cleanup owns
the remaining host-dependent rejection boundary before destructive effects.

Failure of a task does not implicitly change its data policy to persistent. If
teardown cannot establish safety, retain data, report the unresolved cleanup
obligation, and recover or refuse before another session starts.

Removing the policy affects manifests, state markers, identity inputs, cleanup
records, fixtures, and public documentation. Cut over their producers and consumers
together. Existing state must not have its protection silently downgraded by
reinterpreting old marker bytes; specify the explicit old-runtime cleanup or data
preservation procedure under the existing no-migration contract.

**Proof:** exercise both retention alternatives across success, failure,
cancellation, and interrupted-session recovery. Persistent state survives ordinary
clean and is deleted only by explicit purge; run-scoped state is deleted after
safe finalization. Both manual deletion modes refuse active or unowned state.
Verify obsolete policy input rejection and preservation of run evidence.

### Crash-safe deletion: proposed marker-last protocol

Status: architect specification for review, replacing the earlier rename-first
proposal. It does not change the current contract or claim implementation proof.

**Decision proposed:** delete in place, keep the root ownership marker until all
payload entries are gone, and resume through the existing `cleanups` table.
Exclusive slot ownership already prevents a successor from using partially deleted
state. Quarantine therefore buys no execution concurrency here, while adding a
namespace, rename/commit interruption window, and cross-filesystem constraints.
Do not introduce a background collector or a second cleanup registry.

#### Ownership and supported boundary

**Invariant:** one slot owner deletes one authorized data generation; unfinished
deletion remains recoverable and blocks new application execution. The stable
slot lock, registry, and retained evidence live outside the deletion target.

Every cleanup entrypoint requires the same borrowed slot-ownership context used
by session finalization and recovery. Establish process quiescence before deleting:
an available lock, exited leader, or absent listener is insufficient when explicit
process obligations remain. Purge overrides retention only, never ownership or
process safety.

This protocol operates in a runtime-managed namespace. Participating runtimes obey
slot exclusion; external writers must not concurrently rewrite its ancestry or
application tree. Descriptor-relative non-following operations prevent symlink
traversal and reduce pathname races; they do not prove confinement against an
arbitrary malicious same-user actor who can move opened directories, copy markers,
or alter the registry. Refuse observed replacements or ambiguous evidence. Do not
claim that a stored device/inode pair is permanent identity across inode reuse.

#### Minimal durable facts

Add a fresh runtime-generated `dataGeneration` to the ownership marker when a
new application tree is created. Preserve it through subsequent sessions and
provenance updates. It distinguishes successive application-data trees in history;
it does not declare application-data compatibility or replace the removed
`stateEpoch`. It is not a service reuse identity and is not authored in the manifest.

Strict exclusion and blocked recreation already protect ordinary interrupted
cleanup. The generation's additional purpose is to distinguish later recreated
marked trees from earlier intent/history, not to prove permanent physical identity
against copied markers or arbitrary external replacement.

The existing cleanup row retains:

- A unique cleanup ID and the exact runtime-owned relative target.
- The data generation and validated original marker snapshot.
- Deletion authorization: predecessor persistence and explicit purge, if supplied.
- Observed root device/inode as corroborating evidence.
- Pending or completed status, with safe attempt failures in diagnostics/events.

The marker remains the policy owner; its immutable cleanup snapshot survives
marker removal. Do not add another mutable policy record. At most one cleanup may
be pending for the slot's application tree. A retry uses that same operation ID.
A failed attempt remains pending, even if it removed some data. Closed native
types and checked decoding reject incoherent generation/authorization/status
records before deletion.

An old completed cleanup never authorizes deletion of a new tree at the same
path. Do not select arbitrary historical rows by pathname and treat them as
current permission.

#### Deletion sequence

1. Acquire the stable slot guard and settle interrupted process obligations.
2. Resolve pending cleanup before application-root materialization, fresh-marker
   admission or provenance refresh.
3. Open the managed ancestry, target parent, and target without following
   symlinks. Reject the state base, registry, an ancestor, or an unexpected target.
4. Read the marker through the held target directory; validate ownership,
   generation, root observations, and the predecessor's retention policy.
5. For a new operation, authorize run-scoped deletion or explicit persistent
   purge, then commit the intent and redacted event together. No destructive
   operation precedes a successful commit.
6. Delete payload entries through directory descriptors, excluding the root
   marker. Do not rewrite the marker during cleanup.
7. Verify that only the validated marker remains. Establish the required
   filesystem durability barrier for payload removal before removing the marker.
8. Unlink the marker and remove the now-empty root through its held parent.
   Establish the required parent-directory durability barrier.
9. Commit completion and its event. Only then may finalization release the slot
   or a successor create a fresh data generation.

There are only two durable operation states. Marker-present, markerless-empty,
and root-absent are filesystem observations, not separate persisted phase flags.
A registry completion failure leaves the pending intent for another attempt.

A committed purge remains authorization for that operation during recovery; the
next invocation does not need another purge request. Recovery still revalidates
ownership and process safety. Conversely, an incoming run-scoped manifest cannot
retroactively authorize deletion of persistent predecessor data. Retain the
generation's persistence policy; a conflicting downgrade requires explicit
purge/recreation, not a provenance rewrite. General retention-policy conversion
is outside this refactor.

#### Recovery matrix

All observations and actions below occur under exclusive slot ownership.

| Observed condition | Required action |
| --- | --- |
| No pending intent; matching marked tree | Perform fresh authorization before creating intent. |
| Pending intent; matching generation, marker, and root | Resume the same operation, including partially removed payload. |
| Pending intent; marker absent, matching root empty | Remove the empty root only; never recurse or recreate a marker. |
| Pending intent; marker absent, root nonempty | Refuse: this is inconsistent with ordered marker-last deletion. |
| Pending intent; root absent | Establish the required parent barrier and complete that same operation. |
| Pending intent; target is a symlink, file, or different generation/root | Refuse without deleting the replacement. |
| Completed operation; target absent | Report idempotent observation, without inventing a new deletion. |
| Completed operation; new marked generation present | The old operation grants no authority over it. |
| Completed operation; supposedly deleted generation reappears | Refuse contradictory history; do not call it fresh data. |
| Permission/I/O failure during deletion | Keep intent pending; record safe failure; block new execution. |
| Root removed; completion/event transaction fails | Keep intent pending and settle it from absence on retry. |
| Registry missing/corrupt or authorization inconsistent | Refuse destructive recovery. |
| Observed/evidenced external move of the root | Do not search by inode or guess another target; refuse ambiguous evidence. |

A missing target without a current pending operation may mean there is no
application data, according to the control command's documented output contract.
It does not justify attributing that absence to an arbitrarily old cleanup.
Markerless-empty recovery relies on the pending operation, corroborating identity,
and managed-namespace exclusion together; inode equality alone is insufficient.
A hidden external rename can be indistinguishable from completed removal when
the target is absent. The managed-namespace assumption excludes that interference;
the protocol does not claim to detect every external move.

#### Traversal rules

Replace canonicalize/check/recurse-by-path with a focused descriptor-relative
walker. Validate single path components; descend with directory/no-follow/
close-on-exec flags; inspect opened objects with `fstat` and entries without
following symlinks. Unlink ordinary entries and symlinks relative to the containing
directory; remove directories only after visiting their contents. Do not open
device or FIFO entries merely to delete them. Unlinking a regular-file hardlink
must not truncate the inode's other links.

Revalidate the root entry against the held root before final removal. Unexpected
type or identity changes fail, rather than trigger unbounded retries. Holding a
directory FD pins an object, not its ancestry against hostile external renames.
[POSIX unlink/unlinkat](https://pubs.opengroup.org/onlinepubs/9699919799/functions/unlink.html)

Do not traverse unexpected nested mounts. Device equality alone cannot detect
same-device bind mounts; explicitly define supported mount assumptions and
platform checks before promising that boundary. Do not introduce mount management
as part of cleanup.

#### Durability and proof limits

Runtime death and host power loss are distinct failure models. The ordered
protocol supports process-death recovery or refusal; a SIGKILL test does not prove
persistent ordering after power loss.

For host-crash claims, establish and test these dependencies:

- Intent is durable before payload deletion.
- Payload-name removal is durable before marker removal.
- Root removal is durable in its parent before completion commits.
- Initial registry and marker publication have their own durable creation protocol.

Use explicit SQLite durability settings rather than build defaults: the proposed
baseline is WAL with `synchronous=FULL`; evaluate Darwin `fullfsync` support.
WAL NORMAL does not provide the same power-loss durability.
[SQLite synchronization](https://www.sqlite.org/pragma.html#pragma_synchronous)

Linux requires separate containing-directory synchronization for directory-entry
durability. Apple documents limits of ordinary `fsync` and stronger
`F_FULLFSYNC` ordering, but those descriptions alone do not establish a tested
APFS directory-barrier recipe.
[Linux fsync](https://man7.org/linux/man-pages/man2/fsync.2.html),
[Apple fsync](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html)

Qualify supported filesystems and test actual platform barriers before advertising
host-power-loss recovery. Do not give network or unsupported filesystems an
unverified guarantee. A failed barrier leaves cleanup pending and must not cause
the runtime to discard the remaining marker or report durable completion. Rename
quarantine would require persistence barriers too and does not resolve this issue.

#### Coupled changes and verification

Keep registry/WAL/SHM, the stable lock, retained logs/summaries/artifacts, and session
FIFO storage outside the application tree. FIFO removal follows session control
lifecycle, not recursive application cleanup. The current `run_dir` placement
beneath `state_root/runs` must change in the same cutover.

Concrete change sites:

- `state/cleanup.rs`: replace arbitrary-order marker deletion and path recursion;
  resume one pending operation instead of creating a new attempt identity.
- Registry status/schema: replace terminal failed-cleanup semantics with pending
  obligation plus failure evidence. Enforce one pending cleanup and generation
  binding; remove historical-path lookup as deletion authority.
- `state/marker.rs`: add data generation, preserve it across reuse, prevent silent
  retention downgrade, and define atomic/durable initial marker publication.
- `state/placement.rs` and `state/upgrade.rs`: move evidence and perform pending
  recovery before any root creation or marker rewrite; remove epoch handling.
- Finalizer/control: require slot ownership throughout deletion and completion.

Update marker/schema/ABI declarations, producers, fixtures, and normative docs
together. Reject incompatible old bytes; do not add migration readers or rewrite
history. Specify old-runtime shutdown and data preservation before the cutover.

**Required independent proofs:** interrupt every commit/removal/barrier and deep
traversal; retain pending work after partial permission failures; resume with the
same operation ID; refuse markerless nonempty or replacement trees; never apply
old completion to new generations; preserve persistent predecessor data across
configuration changes without purge; prevent outside symlink effects; preserve
logs, registry and control access; prove slot exclusion through completion; inject
transaction/event/sync failures without early deletion or false completion.
Exercise supported replacement/mount boundaries without claiming universal
hostile-filesystem containment. Run Linux and macOS integration proofs and
separate filesystem crash tests for any promised host-power-loss guarantee.

No runtime implementation or platform durability tests were performed by adding
this specification.

## 6. S4: remove reuse and data-compatibility machinery; preserve ownership

Accepted: Nixfied owns process lifecycle and data retention. The application and
user own application-data compatibility and migration. Remove `stateEpoch`,
epoch mismatch admission, and epoch-driven cleanup/replacement. Do not replace
them with inferred compatibility from package versions, executable hashes,
manifest hashes, or service names.

With no standing services, a new session does not decide whether to borrow an old
process. Remove compatibility hashes and comparisons used solely for that decision,
and remove whole-manifest teardown as the prelude to reuse. Do not perfect the old
reuse hash and then delete it.

Retain identities with real remaining consumers: project/environment/slot
ownership, process start identity for safe recovery, raw manifest provenance for
evidence, endpoint ownership, and data-generation evidence for safe cleanup as
proposed in S3. These facts establish ownership and history, not data compatibility.

**Invariant:** a changed manifest does not authorize deletion of persistent data.
After ordinary admission, ownership checks, and interrupted-session recovery,
start the configured executable against the retained persistent data. If startup
fails, report the application's error through existing redaction/output rules,
stop other session processes, and retain persistent data. The user handles any
migration, repair, rollback, or explicit purge. Nixfied does not interpret a
database format, certify successful migration, or reset data in response to a
startup error.

For example, changing PostgreSQL 16 to PostgreSQL 17 starts the new executable
against the same persistent directory. A compatibility refusal from PostgreSQL
is a startup failure to report, not a framework instruction to clean or migrate.
Applications may perform their own normal startup recovery or upgrades; Nixfied
does not promise that the application leaves its files byte-for-byte unchanged.

Run-scoped data still follows session teardown and cleanup independently of any
configuration change. Preserve the original retention authorization: changing a
manifest from persistent to run-scoped cannot silently downgrade retained data.
Interrupted run-scoped cleanup must finish before new slot execution, regardless
of whether the next manifest happens to have the same hash.

Remove the option, manifest/marker fields, identity inputs, generated views,
comparisons, and `UpgradeEpoch` branches together. Update the capability inventory,
schema/marker versions where their semantics change, fixtures, examples, and
normative docs; reject removed declarations rather than accepting ignored fields.
Preserve exact framework ABI/schema admission: it checks Nixfied's own records,
not application-data compatibility. Do not add old-marker readers or automatic
data deletion to make an incompatible framework-state cutover succeed.

**Proof:** executable/configuration changes attempt startup without an epoch gate;
a controlled startup failure is reported while persistent data remains; no
migration/reset command is invented; run-scoped cleanup follows normal lifecycle;
ownership mismatch still refuses; retention cannot silently downgrade; obsolete
epoch declarations reject; recovery does not signal unrelated reused PIDs; raw
manifest hashes remain accurate evidence rather than replacement authority.

## 7. S5: make presentation a downstream consumer

Accepted: share native facts without making compilation depend on presentation
validation. Both products remain required for a passing release check.

```text
Native definitions
    ├── Compilation and execution validation → manifest → runtime
    └── Presentation and reference validation → documentation/help

Release checks require both branches to pass.
```

Provide native module arguments directly from their owning definitions. Derive
documentation and publication reference data from those definitions through a
separate consumer. Remove the bootstrap cycle in which checked presentation needs
raw providers that exist to construct the checked presentation facade.

Manifest constructors must validate their own structural inputs without forcing
unrelated output storage metadata or documentation topics. Whole-inventory and
reference checks remain required release checks, but they are not prerequisites
for obtaining unrelated native bindings.

**Invariant:** invalid execution declarations fail compilation; invalid
presentation relationships fail reference construction and CI. Neither check
claims the other's authority. Retain native option merging, laziness, defaults,
and executable string context.

**Tradeoff:** a revision with broken documentation can still compile an otherwise
valid project. Releases still require the documentation checks to pass.

**Proof:** poison a reference and require docs failure with successful unaffected
manifest compilation; test actual option behavior, lazy bindings, final exports,
and context/closure isolation independently.

For example, rename a documentation topic while leaving one publication reference
pointing at the old name. Building the reference and running release checks must
fail with the reference error. Compiling an otherwise valid PostgreSQL declaration
must still succeed. Invalid executable declarations must continue to fail their
own compilation checks. Shared descriptions and types retain one native owner;
this change removes the dependency round trip, not their shared source.

This acceptance concerns dependency direction. S6 separately records the accepted
boundary for structural type generation.

## 8. S6: bound generation to shared wire meaning

Accepted: generate genuinely shared wire structure; write internal Rust types and
their conversions in Rust. Shared schema facts keep producer and consumer aligned
at the serialized boundary without prescribing private runtime representation.

```text
Shared wire definitions → generated boundary types
                                     ↓
                        native validation/conversion
                                     ↓
                        native Rust execution types
```

Keep a small shared definition of actual wire records and vocabularies where it
prevents producer/consumer drift. Move private Rust storage, visibility, borrowing,
lifetimes, and local output representation back to native Rust definitions.
Separate owned wire types from internal borrowed views and convert explicitly at
existing serialization, error, and redaction boundaries.

For example, a serialized result's field names and wire types can remain shared
schema facts. An internal view borrowing those values through `&str` or slices is
a native Rust choice checked by rustc. Changing that view's lifetime or storage
must not require editing Nix declarations. Generator code will still choose a
concrete Rust representation for wire types; it must not become a configurable
language for describing arbitrary private Rust types.

Introduce separate types and conversions only where they express a real boundary,
such as untrusted wire data becoming admitted execution input or native evidence
being projected for serialization. Do not duplicate equivalent types solely to
match the diagram. Preserve private construction and exhaustive conversions where
they enforce admission invariants.

Remove generator support only after its final native consumer is cut over. Do not
replace the existing machinery with a new general schema or reflection framework.
Rust compilation owns internal representation correctness; wire validation owns
untrusted serialized shape and native lowering owns executable meaning.

**Tradeoff:** some explicit conversion code returns. It exposes a real boundary
and can be checked exhaustively, rather than teaching Nix a partial Rust type system.

**Proof:** literal raw-wire rejection and output tests, native path serialization
failure, redaction before persistent writes, and exhaustive lowering. A private
Rust storage refactor should no longer require editing the Nix schema. Verify that
native representation changes preserve serialized bytes where no wire change is
intended, and retain freshness checks for the boundary definitions still generated.

## 9. S7: reduce the manifest to operational contract inputs

### Accepted: derive execution facts at runtime admission

Accepted: remove carried `servicesRequired` and derived `operationBindings` from
the wire. Runtime admission independently derives the graph facts it executes.
Nix may continue computing facts for early authoring diagnostics and disposable
views. Preserve authored narrowing gates at their owning authoring boundary and
specify any remaining authorization requirement separately from derived metadata.

**Invariant:** the admitted execution plan has one runtime owner. The manifest
carries graph inputs; runtime admission validates their relationships and derives
the facts needed for execution before any child starts. Do not serialize a second
answer solely to compare it with the runtime's own calculation.

```text
Authored graph → manifest graph inputs → runtime validation/derivation → execution
       └── Nix diagnostics and disposable documentation
```

Authored restrictions are choices, not redundant answers. Preserve their intended
enforcement boundary during the cutover; do not silently remove a restriction
because it previously shared a field name with derived metadata. This decision
does not prohibit necessary operation identifiers or independent Nix validation.

**Tradeoff:** lose per-manifest detection of disagreement between the two graph
derivations. Independent conformance vectors and behavioral tests replace that
specific assurance; they do not prove agreement for every possible input.
Runtime references, cycles, capacity, and scoped templates still reject before
spawn. This accepted change requires an explicit DERIVE-1 revision.

**Proof for the accepted change:** independent literal graph vectors and actual
execution ordering/service requirements, plus rejection of invalid references,
cycles, and insufficient capacity before child effects. Update both derivations,
the authored wire inventory, consumers, fixtures, and documentation together, and
remove obsolete equality checks and decoder fields without compatibility aliases.

### Remaining proposals: metadata and policy surface

The following changes are separate from the accepted graph-fact decision and
remain open. API behavior changes are deferred to the separate
[API behavior audit](docs/API_BEHAVIOR_AUDIT.md) after the main refactor. The
proposals below are retained for traceability, not bundled into registry work.

Remove descriptive `stateRefs`, `logRefs`, `artifactRefs`, and `summaryRefs` from
the execution ABI. Useful descriptions may remain Nix-only. Remove source policy
knobs whose distinctions have no implemented effect rather than adding a generic
policy interpreter. Specify live versus immutable source behavior directly;
preserve supported source selection and confinement during that simplification.

Propose removing the redundant closure listener attestation where endpoint
declarations already express managed addressability. Endpoint ownership checks
remain. This does not claim to prevent arbitrary child programs from opening
undeclared sockets. Audit other effect annotations before removing them.

Propose fixed runtime outcome vocabulary instead of configurable terminal labels,
unless an actual external consumer demonstrates why authorable labels are needed.
Do not conflate accepted child exit codes with presentation labels.

**Proof:** independent graph vectors, no-child admission negatives, actual task and
service ordering, endpoint ownership, source confinement, and literal output
outcomes. Delete superseded fields and readers together; do not leave ignored
compatibility fields in the decoder.

## 10. S8: generate help without dictating project layout

Accepted: the help catalog lists Nixfied-generated framework controls and exported
project verbs. It is not a catalog of every app in the final flake.

Build the help catalog from the same native app definitions that generate the
framework controls and exported verbs. Render it during construction and have the
help program print that catalog. Remove final-caller-flake probing, source-context
checks, and the help program's own `nix flake metadata`/`nix eval` calls. Ordinary
outer `nix run` evaluation still occurs. Do not add a custom-app metadata interface
as part of this change.

**Invariant:** generated apps and their catalog obtain names and descriptions from
the same owning definitions. Help construction checks the metadata it consumes;
it does not inspect the caller's final flake to rediscover apps. No second authored
list of project tasks is required.

This should allow ordinary supported Nix module forms without requiring a literal
root-level `nixfied.nix` solely for help provenance. Installer scaffolding can keep
its familiar layout; compiler integration need not mandate it.

**Tradeoff:** custom apps merged later into the final flake remain runnable but
are excluded from this catalog. Later overrides of generated app descriptions in
the final flake are also not reflected; the generated definitions own the catalog
text. Label help's scope accurately instead of claiming to list every flake app.

The expected code-size reduction is modest: the renderer and basic help checks
remain. The primary benefit is removing caller-context dependencies and the
project-layout restriction, not deleting a large subsystem. No existing service
or task configuration needs to move from `nixfied.nix` into `flake.nix`.

**Proof:** caller-directory independence, non-root module placement and module
composition, reserved-name rejection, generated description changes, and explicit
exclusion of separately merged apps and later metadata overrides. Preserve help
argument handling and the absence of runtime state or lock-file mutation. Replace
context-mismatch rejection tests with successful caller-independent catalog tests.

## 11. S9: retain honest compatibility checks

Initial proposal: retain the current exact ABI rejection and capability-descriptor
mechanism during the functional cutovers. Record semantic changes deliberately.
Do not add a structural digest or build fingerprint merely to make the existing
digest appear stronger. It certifies recorded inventory bytes, not all behavior.

Replacing it with an explicit protocol version remains a separate decision if it
removes enough maintenance burden. There is no accepted choice to replace it yet.

Keep native command parsing outside schema interpretation. Treat intended user
behavior separately from incidental malformed-input quirks; revise specific quirks
only through a deliberate parser contract change, not as an undocumented side
effect of this refactor.

**Proof:** mismatched contracts reject, generated definitions remain fresh, and
literal behavior tests cover semantic changes. A digest snapshot alone is not
acceptance evidence for runtime correctness.

## 12. S10: reduce the registry to session evidence and recovery

Status: proposed registry specification following an independent architect audit
of the current schema and its callers, with per-session FIFO cancellation
explicitly accepted below. The accepted session model motivates these removals;
the remaining record and mechanism proposals still require review.
No schema or runtime implementation has been changed by this specification.

### Ownership and scope

**Invariant:** one slot owner authorizes mutation, one session owner executes and
finalizes its work, and durable records retain unfinished obligations when that
owner disappears. Registry rows do not grant a second invocation execution rights.

Keep the per-slot SQLite database. Remove its role as a coordinator of independent
users borrowing standing services. Do not replace SQLite with files, add a second
registry, or turn the event history into an executable event-sourcing engine.

The implementation boundaries are:

- The slot exclusion mechanism arbitrates mutating invocations. A private borrowed
  ownership context is required by mutation entrypoints, including control repair.
- Child execution records local process and outcome evidence. It cannot declare
  the session finalized.
- The session finalizer aggregates the outcome and settles teardown and retention.
- An exclusive recovery successor settles an interrupted predecessor before new
  execution. It never adopts that predecessor's services for continued use.
- OS process identity and held filesystem identity justify signaling and deletion;
  a status string, expired timestamp, or absent lock does not establish safety.

### Current schema and proposed disposition

The current [schema](runtime/crates/nixfied-runtime/src/registry/schema.rs) is
version 7 with eight tables. The proposed target has six responsibilities backed
by tables. Table count is an expected consequence, not the acceptance criterion.

| Current table | Disposition | Retained facts and removed responsibilities |
| --- | --- | --- |
| `registry_meta` | Keep | Immutable project/environment/slot and schema/ABI/toolchain identity. Preserve exact ownership and compatibility rejection. |
| `events` | Keep | Append-only per-slot sequence, redacted payloads, and transition/event atomicity. Replace cross-session service-instance references with run plus declared service identity where needed. |
| `runs` | Reshape | Keep `run_id` as the session identifier, immutable manifest/source/target provenance, and evidence paths. Add owner recovery identity and distinguish execution outcome from unfinished finalization. No second session-ID registry. |
| `services` | Remove | Delete the reusable service identity/lifetime registry. Move the declared service label to process/event records. Slot state ownership remains with the marker; do not create a replacement service registry. |
| `processes` | Reshape | Keep session association, process key, PID/PGID/start identity, redacted command evidence, and supported descendant evidence. Record role and unresolved teardown explicitly, independently of task outcome or ports. |
| `ports` | Narrow to endpoint evidence | Keep endpoint ID/address/port, session/process attribution, and observations required for readiness and recovery. Remove per-slot sharing arbitration and lease-derived reservation ownership. Host endpoint locks and kernel ownership checks remain. |
| `run_leases` | Remove | Delete per-(run, service) leases, owner tokens, heartbeat/expiry timestamps, borrower counts, and TTL transitions. Do not replace them with session TTL leases. |
| `cleanups` | Keep and reshape | Retain intent tied to the exact owned deletion object, authorization evidence, attempts, and completion. Remove the old cleanup-policy dimension; preserve persistence/purge and unconditional safety checks. |

The current `services` table holds a service name, address/endpoint/state/runtime/
target hashes, lifetime, and state root. Most of that exists to decide whether
another run can reuse a service. Service readiness is already derived from other
records; deleting this table need not create another persisted service status.
Session-local service definitions and dependency ordering remain in execution.

Retain concrete recovery facts that currently disappear with the live owner:
the runtime owner's process identity; each child's role and original process
identity; containment and stop requirements needed to terminate it without
depending on a newly supplied manifest. A task occurrence and a service label
must remain distinguishable. Include prepare and probe children in the ownership
analysis, even when they have no service endpoint. Persist only the redaction-safe
facts needed for recovery, never resolved secrets or a duplicate executable graph.

### Remove the sharing protocol throughout its callers

Deleting the tables while keeping their state machine in another form would not
deliver the simplification. Remove these concrete paths together:

| Current owner | Removal or replacement |
| --- | --- |
| [registry/leases.rs](runtime/crates/nixfied-runtime/src/registry/leases.rs) | Remove `RunLeaseHeartbeat`, heartbeat updates, five-second tick, thirty-second TTL, dedicated connection/thread, and stop/join error paths. |
| [main.rs](runtime/crates/nixfied-runtime/src/main.rs) | Remove the session's heartbeat and service-lifetime fields and startup plumbing. Keep the existing finalization/evidence owner. |
| [service/process.rs](runtime/crates/nixfied-runtime/src/service/process.rs) | Remove `BorrowedService`, borrowed alternatives, `BorrowServiceRequest`, `borrow_reusable_service`, prepare heartbeats, standing handoff, and detached output. All successful acquisitions yield locally owned services. |
| [service/registry.rs](runtime/crates/nixfied-runtime/src/service/registry.rs) | Remove `record_service_borrow`, `release_service_borrow`, `mark_service_standing`, `ServiceReuseGuard`, reuse snapshot comparisons, lease conflict gates, and scattered lease updates. Replace startup lease admission with already-held slot ownership and explicit startup evidence. |
| [control.rs](runtime/crates/nixfied-runtime/src/control.rs) | Remove expiry queries/sweeps, `reconcile_until_idle_services`, borrower counting, and borrower-based shutdown refusals. Split observation from mutation under the slot owner. |
| [service/identity.rs](runtime/crates/nixfied-runtime/src/service/identity.rs) and lowering | Remove hashes and fields whose only consumer is cross-session reuse. Retain concrete process/state/endpoint identity needed by safety and evidence. |
| [state/upgrade.rs](runtime/crates/nixfied-runtime/src/state/upgrade.rs) | Remove manifest-hash-filtered process replacement, `ProcessFilter::ManifestHashNot`, and epoch-driven cleanup. Recover interrupted sessions before ownership/retention checks and fresh startup; keep only metadata operations with a remaining evidence purpose. |

The new service startup path is conceptually:

```text
exclusive slot acquired and predecessor recovered
    → dependency/prepare execution with required endpoint startup guards
    → process startup and durable ownership registration
    → verified readiness
    → session-local owned service
```

There is no pre-lock reuse attempt, post-lock reuse retry, borrower reservation,
prepare heartbeat, or choice between borrowed and owned finalization. Preserve
host startup guard scope wherever prepare/spawn/readiness require it; removing
database sharing checks does not make endpoint collisions disappear.

### One writer of session completion

Today `mark_task_finished` updates its process, `runs.status`, lease statuses, and
an event. Service settlement also updates the run; expiry reconciliation can
update it again. Guarded updates then try to preserve other participants' outcomes.

Replace that distributed aggregation with one finalizer. Persist two distinct
facts: execution outcome, and whether finalization remains unfinished. A useful
conceptual representation is:

```text
run_id
    execution outcome: not yet known / succeeded / failed / canceled / interrupted
    finalization: unfinished / complete

process records: local execution evidence + unresolved ownership obligations
cleanup records: claimed deletion objects + incomplete/completed operations
```

These are conceptual domains, not finalized SQL column names or new wire enums.
Use closed native types and checked decoding to prevent incoherent combinations.
Complete finalization requires all process obligations settled and the chosen
retention action completed or deliberately retained under persistent policy.
Successful task execution with unresolved teardown is unfinished. Failed execution
with completed teardown can be finalized. A registry write failure cannot turn
either into successful completion.

Child transitions commit local evidence and events; only the live session owner
or its exclusive recovery successor settles run finalization. Recovery preserves
recorded task outcomes and appends recovery evidence rather than fabricating a
successful result for interrupted work. Do not keep an independent aggregate
status writer in each service/task helper.

### Endpoint evidence must not hide process obligations

Current [status.rs](runtime/crates/nixfied-runtime/src/registry/status.rs), through
`unresolved_escape_sql`, treats an escaped process as actionable when matching
open port rows remain. Removing port reservations without replacing this coupling
would lose recovery information, especially for endpoint-less processes.

Represent an unresolved process obligation directly. A leader exiting or a task
reaching a terminal outcome does not prove all descendants are gone. Retain
endpoint observations for attribution and readiness, linked to the session's
process record rather than a reusable service instance. A gated launcher can
precede registration; the protocol must state what each phase establishes.

Endpoint records do not reserve sockets against the host. Keep exact ownership
observation and host startup locks. Never signal an unrelated listener to settle
a database reservation. Unknown ownership yields cleanup refusal, not an invented
association based solely on matching port numbers.

### Observation and control

Proposed `ps` becomes read-only: read a coherent registry snapshot, inspect current
OS process identities, and report recorded facts separately from current
observations. It does not expire records, finalize sessions, stop until-idle
services, or create an absent registry. Observations can become stale after return;
mutating commands revalidate before acting. Remove `borrower_count` and
`service_lifetime` from output rather than returning meaningless constant values.
This revises the existing mutating reconciliation behavior and requires an atomic
public-output/contract update.

For a live owner, `down` requests session cancellation and leaves teardown to that
owner. For a dead owner, `down` takes the slot and uses the same interrupted-session
recovery path as the next `run`. The accepted transport for `down` is the
per-session FIFO below. Ordinary external signals remain cancellation inputs;
`down` does not use PID signaling or an alternative transport fallback. No
competing finalization writer is introduced.

`clean` takes the same slot authority, resolves interrupted process obligations,
then applies persistence/purge authorization and safe deletion. It has no lease
TTL or borrower-count gate. A live owner causes refusal rather than competing
cleanup. Configuration changes follow the same recovery boundary and never use
provenance as permission to delete persistent data.

### Accepted: one-bit per-session cancellation FIFO

Status: accepted design following three architect reviews. Implement one
per-session FIFO transport on both Linux and macOS. Ordinary SIGINT, SIGTERM,
and SIGHUP remain supported cancellation inputs. Implementation and platform
proofs remain outstanding; acceptance does not claim those tests have passed.

This choice avoids a private macOS signaling dependency. Current placement allows
arbitrary state-base paths and uses a relatively long macOS home-based default. A filesystem
FIFO uses ordinary pathname limits; a Unix socket introduces a much shorter
address limit and may require separate short-path placement. Avoid adding that
placement mechanism solely for one cancellation bit.

**Invariant:** `down` requests cancellation of one selected session, never a
replacement. Only the session finalizes live work; only an exclusive recovery
successor repairs interrupted work. The control endpoint carries no lifecycle
state and grants no authority to perform cleanup.

Use one FIFO in a private, never-reused session directory under runtime control
storage, outside application-data cleanup. Derive its location from existing slot
placement and immutable run identity; add no endpoint registry or persisted
address. Exact layout joins the already-required evidence-layout cutover. Reject
identity collisions; never recreate an old session's control endpoint or use a
slot-wide `current` FIFO. The path identifies the session, so no run-ID message,
packet schema, acknowledgement, or negotiation is necessary.

```text
session: acquire slot → recover → create private session FIFO
         → open nonblocking reader, then separate keeper writer
         → start scoped receiver → publish session → execute

down:    select session once → open that FIFO nonblocking
         → write one byte → observe selected session's finalization

receiver: read a byte → set existing cancellation token
```

Any byte means cancel; the endpoint has no other operation. Duplicate requests
are harmless. Use separate read and write opens in the owner: the keeper writer
prevents idle EOF/HUP from making the receiver spin. Do not use Linux's `O_RDWR`
FIFO shortcut, whose behavior is not a portable POSIX guarantee. Sender open with
no reader fails; owner death after open may instead fail the write with EPIPE.
Retain the existing SIGPIPE handling for control commands. Full buffers, EINTR,
and unavailable readers require bounded handling, not blocking indefinitely.

Keep the current infallible atomic `CancellationToken::is_canceled()` free of
hidden I/O. A small scoped receiver owns the FIFO descriptors and polls with a
bounded shutdown interval. It has no registry connection or teardown authority.
A read sets the token; unexpected transport failure requests normal finalization
and returns a typed error for the owner to record. Owner shutdown stops/joins the
receiver and closes all its descriptors, then removes its endpoint while still
holding the slot. This worker replaces no supervision responsibility and has no
heartbeat or expiry semantics. Do not require it to remain alive after the
monotonic cancellation bit is set; later senders must still observe completion.

Open within validated private directories without following symlinks; verify
the opened object is a FIFO before writing. Protect parent traversal as well as
the final component. Mode/ownership checks use the existing same-user trust
boundary, not a promise against hostile processes with equivalent access to the
registry. Protect all descriptors against child inheritance. An open writer stays
attached to that FIFO object; a successor never opens the predecessor's reader.

Missing endpoints, failed writes, or receiver exit do not prove finalization.
`down` observes the originally selected run and may recover only after acquiring
the slot and rereading records. A successor already holding the slot is never
canceled by that old request. Timeout reports incomplete shutdown, without
automatic owner SIGKILL or independent child teardown. A hung/stopped owner can
still time out; this limitation is shared with cooperative signal cancellation.

Alternatives reviewed:

| Alternative | Decision rationale |
| --- | --- |
| Unix datagram `Cancel(run_id)` | Not selected: requires bounded packet decoding and a socket path that fits both platforms; the FIFO uses the existing session namespace. |
| Unix stream socket | Unnecessary accept/partial-frame/client-lifetime state when no reply is needed. |
| Registry cancellation request | Legitimate separation of requested input from lifecycle ownership, not inherently a second supervisor. Adds conditional control writes outside slot ownership, fresh-read polling, transaction/event choices, and database error/contended-writer behavior. Durability buys little because dead sessions require recovery anyway. |
| PID check followed by `kill` | Check-to-delivery reuse race remains. kqueue observations and advisory locks do not make delivery atomic. |
| Linux pidfd plus private Darwin API | Retains platform-specific availability/maintenance concerns for a request deliverable through public portable IPC. |
| Persistent helper or Mach control rights | Adds lifetime or platform-specific capability distribution machinery; does not simplify delivery directly to the session. |

Validation performed: a temporary Linux primitive experiment confirmed no-reader
open rejection, no idle HUP with the separate keeper writer, multiple cancellation
bytes, and continued attachment of an already-open writer to the original FIFO
after pathname replacement. The replacement received no byte. The replacement
scenario tests descriptor behavior, not permission to reuse session paths. No
experiment files were retained. macOS and actual Rust/runtime integration remain
unverified; public API documentation supports the selected primitives, not a
claim of completed platform testing.

**Required proof:** early/late publication and cancellation; duplicate requests;
private path/type/symlink rejection; absent/dead reader and broken writes; full
buffer timeout; no idle busy loop; worker error and bounded join; no inherited
descriptors; owner death before/after submission; exactly one recovery owner;
untouched successor; and cancellation through readiness, output, and cleanup.
Test selected-session targeting even when a requester pauses across old-owner
exit and successor startup. Run the same behavioral suite on Linux and macOS.
Remote cancellation IPC does not resolve identity-safe signaling of orphaned
workload processes; that remains a separate recovery obligation.

Sources: [Apple nonblocking FIFO open behavior](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/open.2.html),
[FIFO semantics and portability](https://man7.org/linux/man-pages/man7/fifo.7.html),
[pipe behavior](https://man7.org/linux/man-pages/man7/pipe.7.html),
and [Rust Unix datagram alternative](https://doc.rust-lang.org/std/os/unix/net/struct.UnixDatagram.html).

### Rejected remote-signaling alternative: source investigation

Remote `down` uses the accepted FIFO above. This section preserves the rejected
signal candidate's evidence and costs; its macOS dependency is no longer a blocker
for remote cancellation. The sequence below describes that alternative, not a
second transport to implement. Ordinary external signals remain supported.

**Invariant:** only the selected live session finalizes its work. `down` requests
cancellation and observes completion; a successor repairs unfinished work only
after acquiring slot ownership. Identity or permission failures reject signaling
before delivery. A timeout does not confer recovery authority.

Install cancellation handlers before publishing the owner. After acquiring the
slot and recovering predecessors, commit the session's owner identity, host boot
identity, and platform signaling identity before application children start. The
owner remains dedicated to that session; it must not release the slot and reuse
the same process for another session that an old request could cancel.

`down` selects one session, sends SIGTERM to its owner, then observes that session's
durable finalization. Never signal PID zero, a negative PID, or the owner's process
group. Do not reuse live child-group teardown from the current control command.
SIGINT, SIGTERM, and SIGHUP continue to converge on the existing cancellation
token. Repeated requests remain idempotent, without automatic owner SIGKILL.
Child escalation belongs to the owner's bounded teardown.

Lock contention before owner publication means startup is unresolved: retry
observation within a deadline, not guessed signaling. Do not wait for the exclusive
slot lock before requesting live cancellation. If the owner dies, try acquiring
the slot and reread records before recovery; another caller may already own it.
Completion of the selected session ends `down`, even if a replacement has started.
This command does not promise to keep the slot empty. A late signal cannot rewrite
an already finalized outcome. Cleanup continues to settle or safely retain its
obligations when cancellation arrives during finalization.

No request table, heartbeat, acknowledgement exchange, or persistent control
endpoint is required. Initial completion observation can poll the selected run's
record with a bounded deadline; process-death notifications are optional
optimizations, not permission to mutate state.

**Linux candidate:** obtain `pidfd_open` before validating the observed owner
identity, then send SIGTERM through `pidfd_send_signal`. Reject boot/identity
mismatches and unsupported or denied operations without falling back to `kill`.
The handle prevents delivery to a recycled numeric PID; the persisted start and
boot identity establish that it refers to the selected session.

**macOS candidate:** the owner obtains its own `TASK_AUDIT_TOKEN` and records it
with boot identity. `proc_signal_with_audittoken` can validate the execution
generation while delivering a signal with ordinary permission checks. The owner
must not exec after publication. Token knowledge is not a substitute for OS
permission and does not justify obtaining another process's privileged task port.

The source availability investigation found:

| Apple XNU revision inspected | Token-signaling wrapper and kernel dispatch |
| --- | --- |
| `xnu-8792.61.2` | Absent in the inspected wrapper/kernel sources. |
| `xnu-10002.1.13` | Absent in the inspected wrapper/kernel sources. |
| `xnu-10063.121.3` | Present; kernel checks PID generation and permissions on a retained process reference. |
| `xnu-11215.1.10` | Present; internal request handling differs from the preceding revision. |

This is release-source evidence, not a tested minimum macOS version or proof of
SDK exports. The header describes its interfaces as private and subject to
change. The wrapper returns zero or an error number directly; do not interpret it
as a conventional minus-one/errno interface. Use the OS library wrapper if this
design is adopted, never copied syscall numbers/structures: inspected revisions
change internal buffer-size handling. Signal zero is not a portable probe for this
API; the inspected implementation requires a positive valid signal.

The current CI has a `macos-14` job, but a runner label neither establishes the
oldest supported point release nor proves this new API. The investigation host
was Linux. No macOS SDK compilation, export lookup, or behavioral test was run.
Before adoption, record actual OS/SDK/architecture and test library availability,
self-token acquisition, same-user sibling-process signaling, wrong-generation
rejection without delivery, exited targets, and permission errors. Include a
signal handler/pipe acknowledgement and independent canary process so success is
observed behavior, not merely an API return code. Resolve the minimum supported
platform and private-interface maintenance cost explicitly.

If this macOS dependency is unsuitable, evaluate one private session socket as
the replacement transport; do not silently downgrade to check-then-`kill`, ship
both transports speculatively, or declare macOS unsupported without a product
decision. Ordinary external signals and Ctrl-C remain cancellation inputs either
way.

**Proof:** cancellation before publication, before child authorization, during
execution and every finalization phase; repeated/concurrent controls; stale
identities and boot mismatch; owner death during delivery/waiting; one recovery
owner; untouched successor sessions; stopped/hung owner timeout without competing
child teardown; no ownership-descriptor leakage. Run on both platforms through
the actual runtime/registry path. Source review does not satisfy these proofs.

Sources: [Linux pidfd signaling](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html),
[early XNU wrapper](https://github.com/apple-oss-distributions/xnu/blob/xnu-10002.1.13/libsyscall/wrappers/libproc/libproc.c),
[later XNU wrapper](https://github.com/apple-oss-distributions/xnu/blob/xnu-10063.121.3/libsyscall/wrappers/libproc/libproc.c),
[XNU identity-checked delivery](https://github.com/apple-oss-distributions/xnu/blob/xnu-10063.121.3/bsd/kern/proc_info.c),
[subsequent XNU implementation](https://github.com/apple-oss-distributions/xnu/blob/xnu-11215.1.10/bsd/kern/proc_info.c),
and [interface declarations](https://github.com/apple-oss-distributions/xnu/blob/xnu-10063.121.3/libsyscall/wrappers/libproc/libproc.h).

### Registration and interruption

Current child startup precedes durable PID/PGID/start-identity registration. The
registry simplification must not assume that deleting leases closes this window.
Specify gated startup, attachment of actual process identity, and permission for
the workload to proceed as separate steps with explicit interruption outcomes.

A gated parent/child startup handshake is one candidate, not an accepted mechanism.
Writing intent before spawn alone cannot identify a child created afterward. If
recovery lacks sufficient ownership evidence, retain data and refuse new execution.
Cover tasks, prepare invocations, probes, and service leaders with the same
ownership reasoning instead of special-casing only listening services.

The minimum guarantee remains cleanup or refusal for supported process behavior,
not successful automatic recovery from every external scenario. Unknown workload
ownership still requires refusal; the inert launcher case below has a narrower,
explicit safety argument.

### Startup mechanism investigation

Independent architect verdict: retain the launcher approach, but simplify the
protocol and distinguish inert launcher liveness from workload recovery. This is
an architect recommendation pending implementation proof on both platforms.

Use a small trusted launcher that waits for one private execution request, then
replaces itself with the workload. The request carries execution configuration
and is itself permission; there is no separate configuration/permit negotiation.
This could be an early internal mode of the packaged runtime or a packaged helper;
choose packaging separately. It is not a persistent supervisor or a new semantic
manifest seam. Any internal command/protocol must still receive contract review.

Current service startup calls `Command::spawn` before `record_service_start`.
Tasks similarly call `spawn_bounded_exec` before `record_task_started`; exec probes
use bounded execution without that task registration. Ordinary error cleanup
cannot run when the owner is killed between those operations.

| Candidate | Assessment |
| --- | --- |
| Write intent, then directly spawn the workload | Intent alone does not identify a child born before the owner dies. It supports refusal, but does not close the execution gap. |
| Block inside Rust `pre_exec` | A naive wait-for-parent gate can deadlock: the normal spawn path waits for exec/error before returning the child handle. A custom fork/IPC implementation is possible but brings async-signal-safety and descriptor machinery into the runtime. |
| Spawn a trusted launcher, commit its identity, then permit exec | Preferred candidate: ordinary spawn completes into the launcher; the workload remains gated. One startup protocol can serve both platforms and all child roles. |
| Linux parent-death signal | Useful only as an optional additional mechanism. It is Linux-specific, tied to the creating thread, has an installation race, and is cleared in forked descendants. It is not durable ownership or tree containment. |
| Linux cgroup containment | Stronger group membership/termination facilities, but requires available delegated authority and a Linux-specific implementation. Moving an already-running workload into a cgroup does not itself close the initial gap. |
| Darwin suspended spawn | Provides a platform-specific starting point, but a suspended child can remain if the parent dies before recording it. Suspension alone supplies neither durable registration nor automatic orphan cleanup. |

The proposed common protocol is:

```text
session holds slot and has its durable session record
    → spawn trusted launcher with private gate and intended process group
    → obtain and verify launcher PID/group/start identity
    → commit recoverable process identity and event together
    → send one bounded execution request, only after successful commit
    → launcher closes protocol descriptors and execs the declared program
```

Only the session owns the request writer. The launcher must not retain a writer,
slot lock, or database descriptor. Prevent writer leakage into other executed
children. Close-on-exec does not prevent transient inheritance during fork; prove
the actual descriptor setup and account for delayed EOF. EOF before a complete
request, malformed/truncated input, and bounded startup timeout exit without
executing the workload. Validate the complete bounded frame before effects; no
shell evaluation, negotiation, retries, or second execution request. Use explicit
length framing and handle partial reads/writes; pipe capacity is not a message
size guarantee. Gate descriptors must not consume workload
stdin. Handle failed writes without uncontrolled SIGPIPE termination.

Before receiving a complete request, the launcher must not touch application
state, run hooks, fork descendants, or perform normal runtime admission. Launch
it from a safe working directory with a controlled bootstrap environment, never
the workload environment: loader variables such as `LD_PRELOAD` or `DYLD_*` could
otherwise execute workload code before the gate. Deliver executable, arguments,
environment, and working directory privately in the request without durable secret
records or diagnostic arguments. Apply them only after validation. Do not add a
separate helper-ready exchange merely to obtain process identity; normal spawn
and parent-side observation establish the identity to record.

Remove the proposed per-child durable pre-spawn intent. Before a process identity
commit, no execution request can have been sent. An unregistered launcher is
therefore never workload-authorized under this protocol. Its possible delayed exit
is a resource-liveness issue, not an unknown application process that permanently
blocks slot recovery. This explicitly narrows the recovery contract for sterile
bootstrap processes: it does not claim every helper is already gone. Keep bounded
startup waiting and reap helpers while the owner lives; do not create another
launcher registry. If bootstrap sterility or commit-before-send cannot be proved,
this simplification is invalid and recovery must refuse.

Treat a committed identity as **possibly executing** from then onward. Do not add
a second authoritative `released` flag: permission delivery and SQLite commit
cannot be atomic, and recovery must handle both a waiting launcher and a workload
using the same record. Receiving a complete request does not prove exec succeeded
or a service became ready. Preserve separate exec-failure reporting, readiness
checks, timeouts, capture, and cancellation semantics. A close-on-exec error
channel can report setup/exec failures, but EOF alone is not proof of successful
exec: the launcher may have died without reporting. Settle that case using child
exit evidence; never fabricate successful execution or service readiness.
The launcher must not report successful exit without executing the target.
Startup error messages contain fixed stage/error codes, not raw request values.

| Owner dies | Consequence |
| --- | --- |
| Before identity commit | No execution request has been sent. The launcher exits on gate closure when scheduled; it cannot execute the workload. No separate child intent blocks recovery. |
| After commit, before permission | Durable identity exists. Gate closure prevents workload execution; recovery can settle the registered child. |
| During/after permission | The workload may execute, even after owner death if permission was already buffered. Recovery has the committed identity and must stop it before new execution. |

This closes the unregistered-workload window, not every descendant-containment
problem. It does not prove an unregistered launcher has already exited, discover
arbitrary daemonized descendants, or make PID checks race-free. Define the
remaining containment/recovery boundary independently, including boot identity
where needed. Database durability must precede permission; process-kill evidence
is not proof of power-loss durability.

Investigation evidence: a temporary Linux Python mechanism experiment exercised
five owner-SIGKILL trials at each of three barriers: before registration, after
registration but before permission, and after permission. All 15 passed. The first
two produced no workload effects and the launcher exited; the third preserved
PID, PGID, and Linux start ticks through exec and allowed test cleanup. This used
a synced file as registration evidence, not the production SQLite/Rust path.
No experiment files were added to the repository.

Before acceptance, prove the Rust implementation on Linux and macOS, including
concurrent descriptor inheritance, failed registration/event commits, cancellation
racing permission, invalid/partial messages, launcher/exec failure, very fast
workload exit, and interrupted recovery. Verify process start identity across exec
on both platforms and preservation of hermetic environment, stdin, and redaction.
macOS behavior, SQLite durability, and runtime integration remain unverified.

Source review supports the macOS identity strategy: XNU preserves `p_start` during
exec, and `proc_bsdinfo` exposes that value as the start timestamp used by this
runtime. Apple's exec documentation also preserves PID and process group. This
supports, but does not replace, tests on supported macOS versions. If the runtime
binary hosts the launcher, dispatch before ordinary signal-guard/admission setup;
establish the target's required signal mask and dispositions before workload exec.
Use atomic close-on-exec descriptor creation where available; otherwise coordinate
creation and flag installation with every spawn path. Never temporarily make a
parent descriptor globally inheritable to pass it to one child.

Primary references: [Rust Unix spawn implementation](https://doc.rust-lang.org/src/std/sys/process/unix/unix.rs.html),
[Rust pre-exec constraints](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html),
[pipe closure semantics](https://man7.org/linux/man-pages/man7/pipe.7.html),
[parent-death signal semantics](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html),
[Linux cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html), and
[Darwin spawn flags](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/spawn.h),
[XNU exec start-time preservation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_fork.c#L1123),
[XNU process-info projection](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c#L703), and
[Apple exec semantics](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/execve.2.html).

### Data and evidence survive on different terms

Recovery uses the predecessor's recorded ownership and retention authorization;
the next manifest cannot retroactively make its persistent data disposable. Keep
the marker as the state ownership/retention authority and preserve the necessary
snapshot in cleanup intent because deletion destroys that marker. Do not add a
second mutable table of slot policy that must remain synchronized with it.

Recovery must not require the old source checkout or secret values merely to stop
owned processes or refuse ambiguous cleanup. Its persisted inputs must already be
safe to read and report without reconstructing the old secret environment.

There is a concrete layout dependency: [placement.rs](runtime/crates/nixfied-runtime/src/state/placement.rs)
currently puts `run_dir` beneath `state_root/runs/<run_id>`, while the registry is
outside that tree. Move retained run evidence outside the application-data deletion
target, or split that target explicitly, before enabling automatic run-scoped
deletion. Exact paths remain runtime-owned and require a coordinated layout change.
Do not preserve history in SQLite while deleting all files its evidence paths name.
Define which artifact files are retained evidence and which belong to disposable
application state; introduce no new cache or retention manager here.

### Preserve the small durable core

Keep transactionally coupled row changes and redacted events through the existing
[events.rs](runtime/crates/nixfied-runtime/src/registry/events.rs) boundary. Preserve
per-slot sequence ordering; timestamps remain diagnostic. Never store secrets and
redact afterward. Keep closed decoding and exact ownership/ABI/schema checks.

Remove repeated environment/slot columns from per-slot tables only where immutable
`registry_meta` can supply them without losing required output. This optional
normalization is lower priority than removing the sharing protocol. Retain
self-contained provenance where a distinct evidence consumer needs it.

Removing the heartbeat removes a concurrent writer and its failure paths, but is
not sufficient reason to remove WAL, transaction boundaries, or busy handling.
Evaluate those against the remaining observation and control access. Keep native,
focused record operations; do not replace service-specific SQL with a generic ORM
or configurable transition interpreter.

### Cutover and independent proof

Implement the session/registry change as one coherent contract, including controls,
outputs, statuses, cleanup gates, and evidence layout. Supporting refactors may be
separate commits only when each leaves one working current contract. Do not ship
the new table model beside dormant old sharing semantics.

Required evidence:

1. Two same-slot sessions admit one owner; the loser starts no child and mutates
   no application state. Different slots retain real host endpoint conflict checks.
2. Multiple tasks/services share within one session with no leases or heartbeats.
   Completing one task cannot finalize the run while work or teardown remains.
3. Process, task, capture, and cleanup failures preserve distinct evidence; the
   finalizer cannot mark completion with unresolved process or deletion obligations.
4. Kill the owner around spawn/registration, readiness, final task outcome, process
   termination, deletion intent, marker removal, and partial deletion. The successor
   cleans before new execution or safely refuses. Test endpoint-less survivors.
5. Process identity checks prevent signaling unrelated reused PIDs. `ps` verifies
   liveness without database writes or signals; live `down` has one finalizer.
6. Persistent state survives ordinary clean and new process startup; run-scoped
   cleanup preserves registry history and retained logs/summaries. A new manifest
   cannot downgrade old persistent data into automatic deletion.
7. Failed event insertion rolls back its coupled record transition. Secret
   sentinels never enter persistent rows, captured evidence, or error projections.
8. Required old-schema/ABI rejections remain exact, while new raw records reject
   incoherent states. New controls never read old rows through compatibility logic.

Change the registry schema version for its new semantics and update ABI inventory,
authored declarations, generated surfaces still needed, fixtures, and docs together.
Do not migrate or reinterpret old history. Document old-runtime shutdown and an
explicit preservation/export procedure for persistent data before incompatible
registry/marker replacement. Do not automatically delete old history to make the
new schema initialize successfully.

Measure the result by removal of the borrower/standing/heartbeat/replacement
protocol and competing completion writers, plus preserved recovery proofs. Six
tables is a proposed target; no deletion-line count or performance gain is claimed
before implementation and measurement.

## 13. Delivery and verification

Use ordered coherent commits rather than one rewrite. Suggested sequence:

1. Reproduce and fix package selection independently.
2. Complete the session/containment/control and state-exclusion design, including
   S10's registry records and interruption protocol, before dependent behavior.
3. Cut over session ownership, persistence finalization, recovery, and their wire
   surfaces together. Remove standing/borrower alternatives, their registry
   coordination, and competing session-completion writers in the same cutover.
4. Remove obsolete reuse/replacement and epoch machinery; verify retained state
   ownership, retention, and application-owned compatibility behavior.
   Changes inseparable from step 3 belong in that same coherent commit.
5. Reverse presentation dependencies and narrow internal type generation while
   preserving observable behavior where possible.
6. Remove accepted redundant wire assertions in a scoped contract change. Defer
   unrelated inert-field and source-policy changes to the API behavior audit.
7. Simplify help and any separately accepted compatibility/parser changes.

For each cutover, update authored declarations, capability inventory, generated
definitions, fixtures, examples, guides, contract, and independent proofs together.
Change numeric versions only for their defined semantics. Reject obsolete bytes;
do not retain compatibility readers, silently reinterpret old state, or rewrite
append-only history. Specify how users stop old sessions and preserve persistent
data before moving between incompatible runtime contracts.

Use focused behavioral proofs first, then the checks required by
[DEVELOPMENT.md](docs/DEVELOPMENT.md). Runtime changes need fixture-backed `.#test`;
cross-layer cutovers need `.#ci -- --dirty`. Verify shipped release behavior and
supported platforms, not only compilation of the release artifact. Every delivery
must report remaining platform, release, and integration gaps.

Completion means fewer competing authorities with preserved or explicitly revised
guarantees. Counts of deleted lines, generated records, or passing tests are not a
substitute for demonstrating those guarantees.

## 14. Open decisions

The accepted process/data separation, exclusive slot ownership, cleanup-then-
restart recovery behavior, independent compilation/presentation dependency
direction, shared-wire-only structural type generation, removal of carried
duplicate graph answers, persistence as the sole retention policy, executable
identity based on selected outputs, help scoped to generated apps, per-session
FIFO cancellation, application/user-owned data compatibility, and optional task
timeouts with no finite default are not
reopened by this list. These items remain to make implementation concrete:

- Specify S2's optional `--daemon` detachment/input/handoff mechanics and the
  precise relationship between preparation and enclosing lifecycle deadlines.
  Existing task entrypoints, optional task timeouts with no finite default,
  task-owned duration, the background acknowledgement boundary, and immutable
  result identity are settled; `--service` selection is withdrawn.
- Specify supported process cleanup and recovery-or-refusal mechanisms on Linux
  and macOS, including interruption during process registration.
- Review S2's shared supervisor and cooperative wait design, including the
  explicit output-order revision. Specify production output-sink interruption
  before claiming bounded finalization under backpressure.
- Specify the FIFO's exact runtime-owned placement and complete its Linux/macOS
  implementation proofs; transport selection and single-owner finalization are
  settled. Dead-session recovery follows the accepted sequence.
- Specify the slot exclusion and filesystem/registry deletion protocol, including
  durability, identity, and every interrupted transition.
- Review S10's six-table target, finalizer-owned session completion, read-only
  observation, recovery records, and evidence layout before implementing the
  registry cutover. Those details are architect proposals, not yet accepted schema.
- Accept or revise S9; defer S7's remaining metadata/policy proposals to
  the API behavior audit after the main refactor. Review their explicit losses
  and required proofs before accepting them as commitments.

This draft contains design and verification obligations only. None of the proposed
runtime behavior has been implemented or validated by adding this document.
