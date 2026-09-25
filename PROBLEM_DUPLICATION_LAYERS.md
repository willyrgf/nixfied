# Duplication, layers, and architectural ownership

Status: architectural review and decision discussion, not an accepted implementation plan.
Written 2026-09-24 against commit `a889fa3`. Historical record: the accepted
solutions are in [RFC_REFACTOR_DEDUP_LAYERS.md](RFC_REFACTOR_DEDUP_LAYERS.md),
and [CONTRACT.md](docs/CONTRACT.md) describes current behavior. Paths below may
name files that the refactor deleted.

This document records the architecture review of Nixfied and expands the decisions
behind its findings. The original review used four independent architect reviews:
runtime ownership, compiler/manifest design, metadata infrastructure, and adopter
experience. Relevant implementation paths were inspected again when writing this
document. No production behavior is changed by this document.

[CONTRACT.md](docs/CONTRACT.md) remains normative. Several proposals below would
deliberately change it, especially durable service ownership, derived wire facts,
discovery, and compatibility identity. They require explicit contract decisions
and coordinated implementation; this review does not silently redefine them.

## Diagnosis

Nixfied has substantial accidental complexity, and some strong guarantees exceed
what the inspected implementation establishes. Much of the problem begins with
ownership and product scope. Refactoring functions or putting stronger types
around the same design cannot resolve an absent lifetime owner or an incomplete
concurrency protocol.

The useful core is small enough to state precisely:

> Realize declared tools, execute a local dependency graph, and retain sufficient
> ownership to stop its processes and safely manage its state.

Cross-invocation service reuse, automatic final-flake discovery, rich generated
references, and strict wire compatibility are additional product choices. Each
can be valuable. None becomes necessary merely because the current contract
requires it.

The recurring failure is promoting a representation into authority:

- A package's display name substitutes for its executable identity.
- Registry evidence substitutes for continuous process ownership.
- A completed filesystem check substitutes for exclusion during deletion.
- A manifest provenance hash decides service replacement.
- Documentation metadata decides whether native compiler bindings are available.
- Shared wire declarations decide private Rust memory representation.

These substitutions require compensating validation, coordination, and recovery.
The resulting code can be locally careful while its global ownership remains
wrong.

## Evidence and limits

The findings below distinguish directly inspected control flow from inferred
failure schedules and architectural preferences. The original review checked
Nix's first-entry-wins `listToAttrs` behavior independently. It did not execute a
full compiler package-collision reproduction, runtime escape reproduction, cleanup
race, or crash-injection experiment. No full runtime suite, release build, or
macOS verification was run for this review.

Links identify source files and the text names the relevant functions, avoiding
line references that become misleading after unrelated edits. Findings are tied
to the reviewed revision, not permanent assertions about future versions.

## Findings to preserve

| ID | Finding | Evidence and classification |
| --- | --- | --- |
| F1 | Different packages with the same name can collapse into one synthesized closure. | `toolClosureId` and `synthesizedClosures` in [derive.nix](nix/compiler/derive.nix): name-based IDs enter `listToAttrs`; only synthesized-versus-explicit collisions reject. Direct code finding; end-to-end reproduction remains required. |
| F2 | Normal durable-service teardown loses part of the startup containment contract. | `transfer_to_persistent` in [process.rs](runtime/crates/nixfied-runtime/src/service/process.rs) stops monitoring; `down_processes` in [control.rs](runtime/crates/nixfied-runtime/src/control.rs) normally terminates only the recorded group. A permitted child in another group can survive. Static failure schedule. |
| F3 | Cleanup checks do not hold exclusive authority through deletion. | `clean_marked_state` in [cleanup.rs](runtime/crates/nixfied-runtime/src/state/cleanup.rs) checks references, records intent, then deletes. Execution does not use that intent as an exclusion gate. Static concurrency finding. |
| F4 | Partial cleanup can delete the evidence needed for its own retry. | Recursive deletion removes the marker like any other entry; existing targets require a marker, while prior intent recovery is used for a missing target. Static crash schedule in the same cleanup module. |
| F5 | Cleanup confinement can change between checking and reopening a pathname. | `remove_dir_all_confined` canonicalizes a directory and then recursively opens its original path. Concurrent replacement can redirect traversal. Static race schedule, not an executed exploit. |
| F6 | Whole-manifest replacement preempts service-level reuse. | `prepare_slot_state` in [upgrade.rs](runtime/crates/nixfied-runtime/src/state/upgrade.rs) tears down other manifest hashes before service acquisition. Direct control-flow finding and design conflict. |
| F7 | Presentation validation is on the compilation dependency path. | [authoring.nix](nix/meta/authoring.nix), [publications.nix](nix/meta/publications.nix), and [meta/default.nix](nix/meta/default.nix) route native bindings and constructors through documentation-aware assemblies. Direct dependency finding. |
| F8 | Shared schema machinery controls private Rust representation. | [structure.nix](nix/meta/structure.nix) and [rust.nix](nix/meta/rust.nix) describe identifiers, visibility, storage, borrowed records, and lifetimes. Direct scope finding; whether that cost is justified is a design judgment. |
| F9 | The wire carries repeated facts and non-operational metadata. | [lower.rs](runtime/crates/nixfied-runtime/src/execution/lower.rs) checks carried derivations and discards descriptive references. [source.rs](runtime/crates/nixfied-runtime/src/admission/source.rs) treats dirty `warn` like `allow` and records a fingerprint policy without computing a fingerprint. Direct code findings. |
| F10 | Help discovery restricts module integration. | [project-apps.nix](nix/project-apps.nix) requires root-level `nixfied.nix`, `flake.nix`, and `flake.lock`; [GUIDE.md](docs/GUIDE.md) explains the help-source requirement. Direct public API finding. |

## Decision 1: What is the identity of an executable dependency?

The invariant is that the package selected by the author is the package used by
the invocation. Readable names are useful labels, but versions and overrides can
share a name while producing different store paths.

Currently, synthesis maps packages to `tool-${lib.getName package}` globally.
Consider two tasks selecting two builds of Python with the same package name.
Both invocations become references to the same synthesized ID; one closure wins
when the attrset is constructed. Subsequent validation sees a coherent graph
because the conflicting intent was already erased.

Two small alternatives exist. Derive the synthesized key from the selected output
path and any additional executable-selection information needed to distinguish
closures, retaining a readable label separately. Or keep names but reject unequal
closure identities that map to the same name. The second is simpler to introduce,
but prevents legitimate simultaneous use of multiple builds under that name.

Recommendation: use actual closure identity for synthesis. Do not introduce a
second package registry or an alias-resolution protocol to repair name collisions.
Explicit user-authored names can remain references, with uniqueness checked at
their owning boundary.

The compiler owns rejection before emitting the manifest. Proof must include two
same-name packages with distinct outputs, repeated use of the exact same package,
and explicit/synthesized collisions. Verify emitted executable and PATH selection,
not just distinct generated names. This correction need not sacrifice an intended
runtime guarantee, although changed generated IDs require contract impact review.

## Decision 2: Must services survive the invocation that created them?

This is the largest product decision. A task runner with temporary dependencies
and a durable local service manager have different ownership requirements.

### Session ownership

A live runtime owns the processes for the duration of a foreground session. Tasks
can share services within that session; when it ends, owned services stop. Durable
data can remain on disk even though the processes stop. Process lifetime and data
retention are separate decisions.

This removes much of the need for cross-invocation borrowing, detached output
handoff, and standing-service adoption. It loses warm services shared by unrelated
later invocations. It also does not automatically solve runtime crashes: if abrupt
owner death must terminate all descendants, an OS mechanism or another surviving
owner must enforce that behavior. A destructor cannot run after SIGKILL.

### Durable supervision

If services must outlive the initiating command, a durable owner must carry the
contract across that handoff. An OS service manager or a deliberately designed
supervisor could serve that role. The requirement is lifetime ownership, not a
particular implementation or platform API.

This retains cross-invocation reuse but adds a real supervision boundary: who
starts the owner, authenticates control, retains output handling, arbitrates
borrowers, survives failure, and finally declares the service gone? Platform
support must be demonstrated rather than hidden behind a common enum. A manager
that merely remembers the main PID would not solve descendant containment.

A new daemon or supervision protocol would change the current product boundary.
It is an alternative requiring design review, not an implementation authorized by
this document.

### Detached cooperative processes

The current approach can instead be retained with a narrower promise: supported
programs remain in specified groups, obey foreground conventions, and are
reconciled when another command runs. Observation detects some violations; it
does not establish continuous containment.

This is a coherent product only if the weaker guarantee is explicit. It must not
claim to detect every fast reparenting escape or promise immediate idle teardown
when no owner is running.

Recommendation: use session ownership as the baseline unless concrete adopter
workflows require durable reuse. If durable reuse is essential, fund it as a core
supervision feature. Do not make a polling loop responsible for guarantees that
require continuous ownership.

Proof must exercise normal teardown of a child in another process group, abrupt
owner death, borrower death, reuse during owner exit, and persistent output
handling. In particular, test ordinary `down`, not only recovery of a process
already marked as escaped. The immediate F2 defect remains actionable regardless
of which long-term model is chosen.

## Decision 3: What does containment actually promise?

Containment has at least three distinct meanings: signal a cooperative process
group, discover descendants and attempt cleanup, or enforce membership in an OS
resource boundary. Naming each one "containment" does not make them equivalent.

The monitor currently sleeps for 1 ms between traversals. Descendant traversal
scans the host process table for each parent. Its cost grows with both host process
count and the service tree, and background collection errors are discarded. A
single snapshot per traversal would reduce work, but would still miss relationships
created and destroyed between observations.

Recommendation: specify capabilities by the guarantee they establish. Admission
must reject a requested guarantee that the host implementation cannot supply.
An observed descendant list is evidence, not an exhaustive owned-resource handle.
Make loss of required observation an explicit failure; a shorter polling interval
cannot substitute for that policy.

Separate cooperative cancellation tests from adversarial containment tests. A
finite stress test can expose polling failures, but passing it cannot prove that
every interleaving is observed. The design argument must identify what prevents
escape, or explicitly limit the claim to cooperative programs.

## Decision 4: Who has authority to delete a state tree?

Safe cleanup needs three things simultaneously: permission to delete the intended
object, exclusion of new users while deleting it, and enough durable identity to
resume after interruption. A marker, a lease query, and a cleanup event each
address only part of this requirement.

An ordinary race is enough to expose the gap: cleanup observes zero users; a run
then acquires the slot and starts using it; cleanup deletes the tree. No malicious
process or corrupted database is needed.

The crash case is equally concrete: cleanup removes the marker, crashes before
removing the remaining files, and later refuses the existing unmarked target.
Deleting the marker last reduces that interval but leaves a crash window between
marker deletion and directory removal.

A replacement design needs one exclusion protocol covering acquisition, state
materialization, upgrade, and cleanup. Shared execution access and exclusive
deletion are one possible model. Durable services complicate it: a lock held only
by the command is insufficient after that command exits. Their surviving owner
must preserve exclusion, or admission must consult a durable state machine that
provides equivalent protection. This decision depends on Decision 2.

For crash recovery, consider claiming an owned tree as a deletion target and
renaming it out of the active namespace before recursive removal. Persist enough
identity to distinguish that tree from a newly created slot. Rename and SQLite
commit are not one transaction: the protocol must handle interruption before and
after each, specify durability requirements, and never infer permission merely
from a familiar pathname.

For confinement, traverse through held directory identities with non-following
operations rather than validating one pathname and reopening it later. Exclusion
among cooperating runtimes does not prevent another filesystem writer from
replacing a path. The supported threat model must state that distinction.

Recommendation: design the complete ownership protocol before modifying recursive
deletion. Keep policy gates and marker checks, but make them inputs to held
authority. Tests must interleave run/clean and upgrade/run at controlled barriers,
interrupt every durable transition, and replace directory entries during deletion.
Safety and successful retry are separate assertions.

## Decision 5: Is replacement scoped to a manifest or a service?

Provenance answers "which document caused this run?" Compatibility answers "can
this running service satisfy this request?" Ownership answers "who may stop or
delete it?" These facts should not substitute for one another.

The existing upgrade path stops processes from another manifest hash before
service reuse is attempted. Changing an unrelated task can therefore restart an
unchanged database or encounter a lease refusal. The layered service identity
does not protect that database from the earlier decision.

Whole-manifest replacement is a legitimate simpler policy. Under it, the slot is
the unit of configuration and a changed manifest requires an explicit coordinated
replacement. Service identity can then be reduced to what same-configuration
concurrency and recovery actually need. The price is unnecessary restarts and
less flexible concurrent development.

Selective reuse is a different policy. It preserves services across unrelated
edits, but must compute compatibility from the resolved execution contract. The
current identity function takes `ServiceSpec`, state policy, and target. Primary
executable paths are present in inline invocations; resolved supporting tool
directories are not. Source identity and prepare-task bodies also require an
explicit policy. Removing the broad teardown without addressing these omissions
would expose reuse decisions that broad invalidation currently masks.

Secrets and live source require particular care. A secret descriptor does not
prove its current value equals the value used by the running service. Recording
plain secret hashes can leak information and is not an automatic solution.
Likewise, a mutable source path is not a content identity. The product must decide
whether changes require explicit restart, immutable inputs, or another narrowly
specified mechanism. Do not promise automatic compatibility for unobserved facts.

Recommendation: first choose replacement scope. If reuse across edits is retained,
separate provenance from compatibility, compute identity after resolving relevant
dependencies, and require explicit replacement when compatibility cannot be
established. Hashing the entire manifest is conservative but defeats this choice.

Proof must show that unrelated edits preserve the chosen reuse behavior, executable
and supporting-tool changes invalidate it, active borrowers prevent unauthorized
replacement, and state epochs remain distinct from runtime configuration changes.

## Decision 6: Which graph facts belong on the wire?

Nix and Rust independently derive `servicesRequired` and `operationBindings` and
admission compares the carried values with runtime results. This is useful
per-document compiler disagreement detection. It is not an independent permission
boundary: the producer supplies both the graph and its claimed derived facts.

There are two coherent choices. Keep the redundant fields as deliberate checked
assertions, paying for duplicate representations, mismatch diagnostics, and
coordinated changes. Or serialize the graph inputs and let runtime admission
derive its execution facts, while checking producer/runtime agreement in
conformance tests.

Recommendation: prefer the second unless per-manifest disagreement detection has
a demonstrated operational value that outweighs the maintenance cost. Nix can
still derive facts for early diagnostics and documentation. Removing carried
copies does not require removing useful Nix-side validation or trusting arbitrary
JSON at runtime.

The loss must be stated accurately: conformance tests cover their cases; they do
not detect every disagreement on every future manifest. Conversely, agreement
between two implementations does not prove that either implements the intended
meaning. Independent behavioral expectations remain necessary in both designs.

Runtime admission owns graph references, cycles, required-service closure, and
capacity before spawn. Tests need accepted and rejected graphs, literal expected
plans, and executions that prove the intended ordering and service requirements.
Update DERIVE-1 and both sides atomically if the wire assertions are removed.

## Decision 7: How much should shared schema machinery generate?

A fact should have one semantic owner. That principle does not require one
representation for every consumer. A Nix authoring option, wire record, admitted
execution value, and borrowed output view have different jobs.

The current metadata layer extends from shared wire fields into private Rust
storage, visibility, borrowing, lifetimes, and boxed values. That transfers native
implementation decisions into a second language and its custom validator. A Rust
refactor must then pass through declaration syntax, generator behavior, routing,
generated output, and freshness checks.

One alternative is entirely native definitions with explicit parity tests. This
minimizes generator machinery but duplicates wire structure. Another is a small
shared schema for actual wire records and closed vocabularies, with handwritten
internal Rust types and conversions. The present broader generator avoids more
handwritten declarations but increases the number of concepts a maintainer must
understand to change them.

Recommendation: keep generation bounded to shared wire meaning. Let Rust own
private memory representation and let Nixpkgs own option evaluation. Keep ordinary
shared constructors where they express real repetition; do not replace the
current generator with a new general framework.

Explicit conversion is not necessarily harmful duplication. It can be the place
that states which untrusted fields become executable facts and which are rejected.
Exhaustive matches and private constructors can preserve that obligation without
teaching Nix how Rust should borrow a local output record.

Proof must retain raw-wire rejection, serialization behavior, native path failures,
redaction boundaries, and exhaustive conversion. Compare maintenance cost by
adding a real field and changing a private Rust representation, not by counting
generated lines removed.

## Decision 8: Can presentation fail independently of execution?

The current authoring bootstrap collects option documentation and reference
targets, then exposes providers through a checked publication facade. Raw
providers are needed to bootstrap the declarations before that facade exists.
Manifest constructors similarly depend on an assembly containing output metadata
and documentation topics.

This makes one shared source into one shared failure domain. A malformed reference
can prevent compilation even when it cannot affect the emitted execution contract.

Recommendation: native definitions should feed both compilation and presentation.
Compilation validates the facts it consumes. Reference construction validates
references. Whole-inventory audits remain mandatory checks, but ordinary consumers
should not force unrelated presentations to validate before obtaining their input.

This is a dependency change, not a proposal to tolerate broken documentation in a
release. Documentation failures should fail their build and CI. The tradeoff is
that a user may still compile a valid project from a revision with a documentation
defect.

Help has the same issue at the product boundary. Discovering every app merged into
the final flake currently requires source probing and a prescribed module path.
A catalog of generated apps can instead be constructed directly from their native
definitions, with explicitly supplied additional metadata if needed. Automatic
discovery of arbitrary later merges is the feature lost; module composition and
caller-independent help become simpler.

Proof should deliberately break a documentation reference and confirm that the
documentation check fails while valid manifest compilation still succeeds. Test
help from another directory, supported module forms, metadata propagation, and
reserved-name rejection. Preserve lazy option defaults and executable Nix string
context while changing the dependency direction.

## Decision 9: Which policies and labels deserve runtime semantics?

Every public field creates author expectations and future compatibility work.
Before retaining one, identify its observable effect and the owner that implements
it. Being serialized is not itself an implementation of a policy.

Service `stateRefs`/`logRefs` and task artifact/log/summary references are explicitly
discarded by lowering. Keep useful descriptions in Nix-side documentation or
remove them. This loses raw-manifest descriptive labels, not implemented state
ownership or artifact collection.

For source, the implemented distinction is essentially a confined live root
versus an immutable store root. Dirty `warn` does not warn, and the recorded
fingerprint policy does not compute a fingerprint. Either implement a precisely
scoped observation with honest limits or remove the unsupported distinction.
Even an actual admission-time fingerprint would not freeze a live workspace for
the remainder of execution.

Closure-level effects also need review. One executable can listen in one mode and
not another. A closure-wide listener bit can force duplicate declarations while
still providing no enforcement against undeclared listeners. The relevant owners
are endpoint declarations for managed addresses, invocation-level behavior if an
attestation remains useful, and an actual host boundary if network confinement is
promised. An attestation consistency check must be described as such.

Configurable success/failure terminal strings similarly need a demonstrated
consumer. Runtime outcomes already have meaning; allowing arbitrary labels adds
vocabulary without necessarily adding behavior. Prefer fixed outcomes unless
adopter-defined labels solve a concrete integration need.

Recommendation: remove unsupported knobs before adding machinery to justify their
existence. For retained policies, prove an observable effect and its rejection
phase. For removed descriptive fields, prove that execution behavior is preserved
while changing the wire and documentation deliberately.

## Decision 10: What does compatibility identity certify?

The ABI suffix hashes [capability.txt](runtime/crates/nixfied-manifest/capability.txt).
This reliably changes when the inventory bytes change. It does not automatically
change when implementation semantics change without a descriptor edit. Comments
also participate in the digest. The current contract correctly depends on authors
recording behavioral changes; the hash does not remove that human obligation.

An explicit protocol version makes the obligation visible. A generated structural
digest can additionally detect changes to the actual schema, but still cannot
prove semantic compatibility. Keeping the present descriptor is also defensible
if it is described as a reviewed compatibility declaration rather than an
automatic semantic fingerprint.

Recommendation: preserve exact mismatch rejection and choose the least costly
mechanism that accurately states what it checks. Do not maintain a version,
inventory digest, schema digest, and build digest unless each has a distinct
consumer and failure meaning. No choice eliminates behavioral conformance tests.

The contract should also distinguish intended CLI behavior from parser accidents.
Freezing malformed-argument quirks as exact ABI makes parser replacement expensive
without improving lifecycle safety. Choose desired behavior explicitly and change
it atomically under the existing no-backward-compatibility policy when warranted.

Proof must cover old/new admission mismatch, structural freshness where generated,
and literal behavior for semantic changes. A digest snapshot proves that bytes
changed, not that a runtime honors the fields it accepts.

## Decision order and delivery

These decisions interact, but they should not become one unreviewable rewrite.

1. Establish focused reproductions for package selection, ordinary process-tree
   teardown, cleanup concurrency, and partial cleanup recovery. Fix correctness
   defects without waiting for the entire architecture discussion.
2. Decide process lifetime and containment promises. This determines who can hold
   state exclusion and output ownership after a command exits.
3. Design cleanup authority and crash recovery around that owner. Specify all
   interruption states before adding more deletion checks.
4. Choose manifest-wide replacement or selective service reuse. If selective,
   complete compatibility inputs before removing broad invalidation.
5. Reverse presentation dependencies and bound code generation. Preserve wire
   behavior during this work where practical, so dependency changes are reviewable.
6. Remove redundant wire assertions and inert policy fields only through explicit
   contract changes, with their lost assurances and consumer impact recorded.

For each accepted decision, record the invariant, its owner, invalid states made
unrepresentable, the rejection boundary for remaining runtime facts, and independent
proof. Separate runtime guarantees retained from conveniences intentionally lost.
Do not measure success solely by fewer lines, fewer files, or more generated code.

The desired result is fewer competing authorities and fewer promises that depend
on reconstructing ownership after it has already been lost. A smaller contract
with demonstrable guarantees is stronger than a larger contract surrounded by
checks that cannot establish it.
