# RFC: simplify ownership and remove duplicated layers

Status: draft. Session-owned service processes, a single persistence policy for data retention,
one active session per slot, interrupted-session cleanup followed by a fresh
start, independent compilation/presentation consumers of native definitions,
structural type generation limited to shared wire meaning, runtime derivation
of execution graph facts without carried duplicate answers, executable
dependency identity based on selected outputs and executables, and help scoped
to Nixfied-generated apps are accepted
architectural direction. Concrete mechanisms and other solutions below remain
proposals.

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

Explicit cleanup and state-changing recovery or upgrade use the same exclusive
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
| F3: cleanup races execution | S3: exclusive slot mutation authority shared by acquisition, recovery, cleanup, and state upgrade. | Runtime slot/state ownership |
| F4: partial cleanup loses its marker | S3: durable deletion intent tied to an identified tree, with explicit interrupted-transition recovery. | Runtime state cleanup |
| F5: pathname checks race traversal | S3: directory-identity-based traversal with non-following operations. | Runtime filesystem boundary |
| F6: manifest replacement defeats service reuse | S4: delete cross-session service reuse and broad manifest-triggered process replacement; retain state compatibility. | Runtime identity/state admission |
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

Proposed mapping: one runtime `run` execution is one session covering its root
task graph, prepare tasks, services, probes, child output, and finalization. This
reuses the existing execution boundary instead of introducing a second workflow
kind. The exact foreground development command remains to be specified; this RFC
does not add a daemon, attachment protocol, or arbitrary runtime scheduling.

One owner retains process handles and teardown responsibility for the whole
session. Services can be shared inside that owner, but there is no transfer to
standing ownership and no borrower lease from another session.

Remove `serviceLifetime`, `until-idle`, `persistent-until-down`, cross-session
borrower acquisition, standing commits, and detached output handoff. Remove their
wire fields and branches rather than retaining aliases with identical behavior.
Audit registry records individually: durable execution evidence and interrupted
session recovery remain necessary, but borrower coordination no longer justifies
their own tables or transitions.

### Finalization

**Invariant:** application state is not deleted while any session-owned process
may still use it. Success, task failure, timeout, and graceful cancellation all
enter the same finalization owner.

The proposed ordering constraints are:

1. Stop scheduling new work and settle active bounded children.
2. Complete required output projection while its sources remain available.
3. Stop services in dependency-safe order; contain descendants and reap owned
   children. Stop probes and join capture workers as their owning processes end.
4. Apply application-state retention only after process quiescence is established.
5. Retain final evidence and cleanup outcomes, then release session ownership.

These are ordering constraints, not a new alternative output pipeline. Preserve
the existing replay/redaction guarantees and attempts to complete later cleanup
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
needs one cancellation/finalization owner; recovery of a dead owner is a separate
case. The exact control mechanism must be specified before cutover so a control
command cannot race the live owner with independent teardown.

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

Use one exclusion protocol for session acquisition, recovery, explicit cleanup,
and state-changing upgrade. Its stable coordination object must live outside the
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

### Crash-safe deletion

Proposal: claim a particular owned tree for deletion under slot exclusion, record
durable intent, and move that tree out of the active namespace before recursive
removal. Hold directory identities and use non-following relative operations so
later path replacement cannot redirect deletion.

The durable record belongs to the existing registry rather than a new cleanup
database. It must identify the deletion object and ownership facts independently
of the marker being deleted. Never infer permission from a familiar pathname.

Rename and registry commit are not one atomic transaction. Before implementation,
specify recovery for at least these interruption points:

| Observed condition | Required recovery behavior |
| --- | --- |
| Intent exists; original tree remains | Revalidate its identity under exclusion before continuing. |
| Tree moved; completion not recorded | Find the exact claimed deletion object and continue without touching a replacement tree. |
| Tree partly deleted, including marker | Continue from retained deletion authority, not a missing marker. |
| Claimed tree absent; intent incomplete | Complete the recorded operation only when its identity/history proves this case. |
| Unexpected tree or ambiguous evidence | Refuse mutation and retain diagnostic evidence. |

Specify filesystem durability and ordering alongside these cases. This table is
an obligation for the protocol, not a claim that rename alone makes it crash-safe.

**Proof:** two simultaneous sessions for one slot admit only one owner, with no
losing-session child or application-state mutation; sessions in different slots
can proceed independently. Hold finalization at controlled barriers and prove
that a competing run or explicit clean cannot mutate the slot. Kill the owner
while descendants remain and prove that lock availability alone never admits new
execution. Exercise recovery/run interleavings, interruption at every durable
transition, directory replacement during deletion, protected data survival,
automatic run-scoped deletion, and persistent PostgreSQL data surviving into a
new process in the next session. Ensure logs and registry history remain intact.

## 6. S4: remove service reuse identity; preserve state identity

With no standing services, a new session does not decide whether to borrow an old
process. Remove compatibility hashes and comparisons used solely for that decision,
and remove whole-manifest teardown as the prelude to reuse. Do not perfect the old
reuse hash and then delete it.

Retain identities with real remaining consumers: project/environment/slot
ownership, process start identity for safe recovery, raw manifest provenance for
evidence, endpoint ownership, and state compatibility. Name their purposes clearly.

**Invariant:** a changed manifest does not authorize deletion of persistent data.
An incompatible state epoch must refuse reuse of retained data until the existing
policy permits an explicit operation. This RFC adds no state migration machinery.
Interrupted run-scoped cleanup must finish before new slot execution, regardless
of whether the next manifest happens to have the same hash.

**Proof:** unrelated task edits do not erase retained data; a new session starts
new processes; epoch mismatch preserves protected data; recovery does not signal
an unrelated reused PID; manifest hashes remain accurate execution evidence.

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

Status: proposed specification following an independent architect audit of the
current schema and its callers. The accepted session model motivates these
removals; the exact records and control mechanisms below still require review.
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
| [state/upgrade.rs](runtime/crates/nixfied-runtime/src/state/upgrade.rs) | Remove manifest-hash-filtered process replacement and `ProcessFilter::ManifestHashNot`. Recover interrupted sessions before evaluating retained state, regardless of their manifest hashes. |

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
recovery path as the next `run`. The exact cancellation transport remains open;
do not let this specification introduce a daemon, a competing SQL writer, or a
bare persisted PID treated as unconditionally safe signaling authority.

`clean` takes the same slot authority, resolves interrupted process obligations,
then applies persistence/purge authorization and safe deletion. It has no lease
TTL or borrower-count gate. A live owner causes refusal rather than competing
cleanup. State upgrades follow the same recovery boundary and never use provenance
as permission to delete persistent data.

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
   termination, deletion intent, rename, and partial deletion. The successor
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
4. Remove obsolete reuse/replacement machinery and verify retained state identity.
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
identity based on selected outputs, and help scoped to generated apps are not
reopened by this list. These items remain to make implementation concrete:

- Define the exact foreground development session entrypoint without adding
  dynamic orchestration.
- Specify supported process cleanup and recovery-or-refusal mechanisms on Linux
  and macOS, including interruption during process registration.
- Define live-session cancellation by control commands without introducing a
  second finalization owner; dead-session recovery follows the accepted sequence.
- Specify the slot exclusion and filesystem/registry deletion protocol, including
  durability, identity, and every interrupted transition.
- Review S10's six-table target, finalizer-owned session completion, read-only
  observation, recovery records, and evidence layout before implementing the
  registry cutover. Those details are architect proposals, not yet accepted schema.
- Accept or revise S4 and S9; defer S7's remaining metadata/policy proposals to
  the API behavior audit after the main refactor. Review their explicit losses
  and required proofs before accepting them as commitments.

This draft contains design and verification obligations only. None of the proposed
runtime behavior has been implemented or validated by adding this document.
