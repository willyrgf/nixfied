# RFC: simplify ownership and remove duplicated layers

Status: draft. Session-owned service processes, a single persistence policy for data retention,
one active session per slot, interrupted-session cleanup followed by a fresh
start, independent compilation/presentation consumers of native definitions,
structural type generation limited to shared wire meaning, runtime derivation
of execution graph facts without carried duplicate answers, and executable
dependency identity based on selected outputs and executables are accepted
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
remain open.

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

Build the help catalog from the same native app definitions that generate the
framework controls and exported verbs. Remove final-caller-flake probing as a
requirement for project app construction. If projects need custom entries, accept
explicit metadata through a small interface rather than reintroducing evaluation
of the caller's entire flake.

This should allow ordinary supported Nix module forms without requiring a literal
root-level `nixfied.nix` solely for help provenance. Installer scaffolding can keep
its familiar layout; compiler integration need not mandate it.

**Tradeoff:** help no longer automatically discovers arbitrary apps merged later
into the final flake. Generated help still corresponds to the definitions used to
build its catalog and must not misrepresent its scope.

**Proof:** caller-directory independence, non-root module placement and module
composition, reserved-name rejection, generated description changes, and explicit
custom metadata if that interface is included.

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

## 12. Delivery and verification

Use ordered coherent commits rather than one rewrite. Suggested sequence:

1. Reproduce and fix package selection independently.
2. Complete the session/containment/control and state-exclusion design, resolving
   the open decisions below before implementing dependent behavior.
3. Cut over session ownership, persistence finalization, recovery, and their wire
   surfaces together. Remove standing/borrower alternatives in the same cutover.
4. Remove obsolete reuse/replacement machinery and verify retained state identity.
   Changes inseparable from step 3 belong in that same coherent commit.
5. Reverse presentation dependencies and narrow internal type generation while
   preserving observable behavior where possible.
6. Remove redundant wire assertions and inert fields in scoped contract changes.
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

## 13. Open decisions

The accepted process/data separation, exclusive slot ownership, cleanup-then-
restart recovery behavior, independent compilation/presentation dependency
direction, shared-wire-only structural type generation, removal of carried
duplicate graph answers, persistence as the sole retention policy, and executable
identity based on selected outputs are not reopened by this list. These items
remain to make implementation concrete:

- Define the exact foreground development session entrypoint without adding
  dynamic orchestration.
- Specify supported process cleanup and recovery-or-refusal mechanisms on Linux
  and macOS, including interruption during process registration.
- Define live-session cancellation by control commands without introducing a
  second finalization owner; dead-session recovery follows the accepted sequence.
- Specify the slot exclusion and filesystem/registry deletion protocol, including
  durability, identity, and every interrupted transition.
- Accept or revise S4, S7's remaining metadata/policy proposals, and S8–S9
  after reviewing their explicit losses and required proofs. Those proposals are
  not user-approved commitments.

This draft contains design and verification obligations only. None of the proposed
runtime behavior has been implemented or validated by adding this document.
