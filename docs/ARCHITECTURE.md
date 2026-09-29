# Nixfied Architecture & Design Rationale

This is the *why* behind Nixfied's design. It is condensed from the original
build spec (RFC v2.3), which has since been retired from the tree — its
decisions live on here and in git history.

- **[`README.md`](../README.md)** — what Nixfied is and how to use it.
- **[`CONTRACT.md`](CONTRACT.md)** — the normative invariants and boundaries.
- **[`DEVELOPMENT.md`](DEVELOPMENT.md)** — repository layout, checks, and test
  placement.
- **This file** — the reasoning behind the contract and the v1 mistakes that
  motivated it.

## The problem

Modern projects are small systems: APIs, workers, databases, queues, migrations,
test harnesses, CI jobs, and dev tasks — often polyglot, owned by different
tools. Their operational behavior is scattered across `flake.nix`, shell scripts,
package scripts, CI YAML, compose files, env files, and port conventions. Nothing
gives a project a clean, process-aware way to run multiple environments — or
multiple copies of one environment — while keeping services, tasks, state,
logs, ports, artifacts, and cleanup under a single authority. Without first-class
environment/slot/state/placement/registry/source concepts, `dev`/`test`/`ci`
runs collide, ports and state leak, services started by one script are stopped by
another (if at all), and CI leaves weak evidence of what ran or survived.

## The load-bearing decision: two tools, one seam

The split is defined by the verb, not by timing:

- **Nix is the integration *and* correctness layer.** It is programmable, typed,
  reproducible, and already where people describe environments. Typed modules
  evaluate to `manifest.json`; invalid intent never produces an admitted manifest. Nix
  builds/realises closures and emits the disposable docs view. It is also the
  public extension API:
  adopters integrate by importing a module, not by vendoring internals or
  reshaping their repo.
- **Rust is the hidden, generic execution runtime.** It knows no domain
  (Postgres/Node/Python/nginx). It executes generic primitives and enforces typed
  lifecycle semantics. Mutable runtime state — process ownership, ports,
  reconciliation, cleanup, logs, summaries — is Rust's responsibility.

> Nix is the authority for what the project is *allowed to be*. Rust is the
> authority for what it is *currently doing*.

Three properties, deliberately distinct:

- **Codified & discoverable** — the whole project is typed data in `manifest.json`; a
  human or agent learns its structure from the manifest or generated docs view.
- **Deterministic manifest & logical placement** — same typed input ⇒ same compiled
  manifest and logical placement. Host-absolute paths are *not* part of this; Rust
  materialises them at admission.
- **Owned execution** — runtime timing/scheduling is not deterministic, but every
  process is owned, tracked, attributable, OS-reconciled, and cleanable.

This split creates subtractive design pressure: one seam and one owner per
responsibility make it possible to remove concepts, branches, public types, and
change sites instead of coordinating more of them. Net LOC reduction is useful
evidence when it removes semantic machinery while keeping the typed boundaries,
fail-closed checks, and independent seam proofs explicit.

## Why one `manifest.json` seam

The single most important hardening choice. Each clause replaced a v1 failure
mode:

- **Many artifacts → one semantic manifest.** No required sidecar,
  `schema.json`, or `capabilities.json`. The manifest package contains only
  `manifest.json` plus the disposable, manifest-derived `views/docs.md` human
  reference. (v1: artifact sealing kept growing toward a mini package format.)
- **Separate admission metadata → manifest-owned admission contract.** Source identity,
  target identity, generator/toolchain identity, closure metadata, declared service
  identity, and state policy are first-class `manifest.json` fields — so the manifest
  *is* the admission contract, not just an execution graph. The contract is also
  *typed*: illegal shapes (a malformed lifecycle, a non-loopback endpoint, a zero
  timeout) are unrepresentable rather than caught by a rule, and the
  cross-references are proven by lowering the manifest into the executor's input.
- **Self-hash → computed provenance hash.** The manifest embeds no self-hash
  (circular). The runtime computes `computedManifestHash = sha256(raw bytes)` at
  admission and records it in registry/summaries/logs/errors.
- **Manifest origin → Nix store output.** Normal admission requires `manifest.json`
  under the Nix store: a local origin/trust + immutability policy, not a proof of
  compiler provenance. A non-store escape hatch exists only for framework
  tests/dev (`--allow-non-store-manifest`).
- **Replacement instead of compatibility.** Exact ABI/toolchain matching means
  adopters recompile when the contract changes. Translators and dual
  implementations would multiply parsers, branches, tests, and future change
  sites; Git preserves removed implementations. The capability digest records
  every manifest/runtime contract change, while numeric versions change only for
  their separately defined semantics.

The Rust admission APIs own their input document. `admit_run` and `admit_control`
read and hash the manifest, reject its origin before parsing, and construct private
admission values only after the applicable checks and lowering succeed.
`ControlAdmission` owns immutable document, provenance, and execution facts;
`RunAdmission` additionally owns required source and resolved secrets. Child
execution requires the run type; recovery consumes the common control type and
does not resolve source or secret values. Neither exposes mutable proof fields.
The manifest crate's `ValidatedManifest` is the structural prerequisite for
lowering; it does not claim host admission.

Lowering builds private candidate storage and moves it into the admitted execution
manifest after relational proof. One Rust planner supplies cycle checks, occurrence
flattening, combined service edges, and required-service closure. Admission derives
each task once and checks endpoint demand against every slot. A selected plan borrows
definitions and contains each service's prepare occurrences; slot binding allocates
ports before placement or registry effects. Invocation lowering selects each closure
once and parses scoped templates. Execution renders values opaquely, and prepared
probe commands retain concrete arguments and environment across attempts while
rechecking filesystem confinement each time.

## Shared contracts and the static reference

Adopters use ordinary Nix modules and the public library. The private declaration
machinery has four responsibilities:

| Mechanism | Shared facts | Native owner retained |
| --- | --- | --- |
| Option metadata | Paths, types, defaults and explanations | Nixpkgs evaluation, merging and transformations |
| Serialized structures | Manifest, result and error fields; inventory-linked vocabularies | Native scalar types, relational validation, derivation and runtime behavior |
| Command syntax | Tokens, value domains, literal defaults and help | Native parsers, encoding, precedence, contextual defaults and effects |
| Publications | Exported names, descriptions and lazy bindings | Native functions, modules, apps and packages |

Manifest records and closed wire vocabularies generate boundary types.
Output records share wire field descriptions and coverage checks with the
reference, while Rust owns their definitions and serialization views. Borrowing,
lifetimes, visibility and boxed storage of runtime records are native Rust
choices; they do not require schema edits. `NativeDomain` binds scalar wire
meaning to existing validation and serialization, not whole private records.

Native structs remain in their owning Rust modules alongside conversion,
redaction, formatting and write logic. Borrowed views project existing evidence
without creating another mutable data graph or converting through JSON.
Raw-wire and behavioral tests independently check those serialization boundaries;
generation is not a substitute for native validation or output tests.

Native bindings and presentation independently consume the same definitions.
Module evaluation receives its arguments directly from their native providers;
publication reference validation is not a compilation dependency. Manifest
constructors validate only manifest structure, without unrelated runtime output
storage or documentation topics. Presentation checks metadata and references
when constructing the reference. Release checks require both consumers and
whole-inventory coverage; native projections reject duplicate binding names.
Configured values, defaults and executable string context retain their native
owners and laziness.

The reference is built from those facts and existing authored documents at the
supplying framework revision. Its lookup index is disposable presentation data.
Topics combine focused explanation with concise entries from the selected
definitions. Checked references and topic membership supply navigation in both
directions from one association; they do not infer execution prerequisites or
native effects. Unknown targets and invalid selectors reject during reference
construction. `nix run .#docs -- topic runtime` demonstrates the runtime view;
exact `api` and `option` queries provide the full entries it references.
Only serialized presentation data loses Nix string context; executable values
keep their dependencies. Reference queries need no admitted manifest or runtime.
Structural agreement and reference freshness do not prove behavioral parity:
native guarantees still require independent admission, output and OS evidence.

The supported field forms, generation workflow and proof owners are documented
in [DEVELOPMENT.md](DEVELOPMENT.md#adopter-api-maintenance). New declaration
families or behavioral machinery require a contract-consistent design review.

## The task–service algebra (the composition rewrite)

The first external adoption exposed the original sin of the authoring
surface: it was the runtime's **wire format exposed
raw** — closures, invocations, operationIds, terminal tokens — so adopter concepts
with no runtime equivalent (a *command*, a *toolchain*, a *pipeline*, a
*verb*) escaped the manifest: below it into opaque shell dispatchers, above it
into the adopter's own flake. The accepted fix is a closed algebra of exactly
**two semantic kinds** (KIND-2):

- **task** — composable execution with an optional deadline: a **leaf** (one inline
  invocation + `requires` + exit policy) or a **composite** (a static
  named-step DAG over task references; STATIC-1 — no parameters,
  conditionals, retries, or loops in the contract).
- **service** — durable execution: orchestrated, probed, owned, cleaned —
  with **endpoints optional**, because durable is not listening: a queue
  consumer or indexer is owned, invocation-probed, contained, and cleaned
  while binding no socket and claiming no port (PORT-1 stays fully scoped to
  declared endpoints).

The connective tissue is the **invocation** — the one way anything says
"run this program". Its fields come from the structural definition rendered by
`nix run .#docs -- api record primitive/Invocation`.
Invocations are **anonymous and inline** (INVOKE-1): naming them for reuse would
recreate the named-invocation registry and its reuse/wiring entanglement;
content reuse is a Nix `let`, and the manifest carries the fully-applied copies.
Cache is not a third semantic kind or a runtime resource. Tool acceleration is
child/tool/project-owned and may use ordinary declared invocation environment
or arguments; Nixfied does not identify, place, create, lock, report, retain, or
selectively clean those artifacts. A cache-specific placement layer would imply
ownership Nixfied does not have: an honest Nixfied-owned cache manager also
needs writer arbitration, leases, accounting, retention and eviction policy,
deletion recovery, inspection, and operator controls. None is required for
runtime execution correctness, while ordinary invocation environment and
arguments already let each project configure its own tools without splitting
lifecycle responsibility.

Adopter vocabulary enters the contract as **names over this algebra, never as
schema**: `check` is not a concept nixfied knows, it is a composite an
adopter named, exported to the flake surface through
`nixfied.surface.verbs`. That attrset combines the explicit export choice with
the presentation metadata for each generated app: its key is the task id and
its value is the exact nonempty user-facing description. Exported verbs share
a namespace with framework apps reserved by VERB-1 and derive only from
adopter-exported task names. Generated help is a Nix-only catalog built from the same app definitions.
It does not inspect the caller's final flake or require a particular module
layout. Custom apps and later description overrides remain outside its scope.
It adds nothing to the manifest algebra or runtime ABI. Environment
**membership does not exist**: running a task brings up exactly the services
its leaves require —
runtime admission derives its execution graph from declared inputs under
`docs/DERIVATION_SPEC.md`. Nix independently checks graph feasibility and
binding restrictions, but neither derived service sets nor derived binding
sets cross the manifest seam. Independent vectors and runtime behavior tests
check conformance rather than comparing two carried answers at admission.
Hand-declaration is reserved for *choices* (surface verbs) and *attestations*
(effects). Child environments are **hermetic**: declared env plus the
runtime-owned PATH assembled from the tool roots, nothing inherited.

## Correctness in four layers

1. **Manifest correctness (Nix).** Reject invalid names, broken references, cyclic
   task graphs, malformed state/placement, missing closures, invalid primitives, or
   bad source/target policy — before runtime is even possible.
2. **Closure correctness (Nix).** Reproducibly realise the store paths the runtime
   may execute.
3. **Admission correctness (Rust).** Host checks Nix cannot prove: store origin,
   exact ABI/toolchain, target support, source policy, already-realised closures,
   reference resolution (the manifest lowers into the executor's input only if every
   cross-reference exists), writable marker-owned state, registry acquisition, port
   ownership strategy, and interrupted-session recovery.
4. **Execution correctness (Rust).** The impure graph: process groups, signals,
   readiness/health, task/composite cancellation, registry events, summaries,
   cleanup, reconciliation.

This keeps Nix central to project evolution while preventing it from becoming a
live process supervisor.

## Identity & placement

Every runtime action is scoped by `projectId / environment / slot / runId`.

- **Source is explicit.** Every tree the runtime may observe is a declared
  `codebase` (logical root, source mode, dirty policy, fingerprint policy). Runtime
  operations observe source only through declared `codebaseId`s. A live
  workspace resolves from the invocation root; immutable `snapshot` /
  `flake-input` modes resolve from the Nix store root carried in
  `sourceIdentity`, so `dirtyPolicy = reject` is provable for those modes.
  (v1: live checkout state was implicit.)
- **Service attribution belongs to process evidence.** The declared service label
  is stored with the session's process. Endpoint attribution requires process
  evidence and OS ownership proof. A service reference is the run ID plus the
  declared name.
  Startup requires predecessor process settlement, including endpoint-less
  workloads. Cleanup lifecycle events carry the declaration without an instance.
- **Placement is split by phase.** Nix bakes only the *logical* placement the
  manifest needs — the per-slot candidate port windows — into `manifest.json`; the
  directory layout (state root, registry, run/logs/artifacts) is a runtime-owned
  constant, and Rust materialises *host-absolute* placement at admission
  (`$NIXFIED_STATE_DIR` or a platform default), with run-scoped paths using the
  `runId` known only at runtime. (v1: placement drifted when both sides derived it;
  the layout templates were later pinned constants in the manifest, then removed.)

## Registry and liveness

The normative registry, session, process, and cleanup rules are in
[Registry, state, and processes](CONTRACT.md#registry-state-and-processes).
This section records why they take that shape and which runtime component owns
each rule.

- **Per-slot SQLite WAL registry** is a *durable record, not a liveness oracle*
  (v1: the registry was treated as liveness truth). Timestamps are diagnostic
  only; the per-slot event order is the order.
  Existing version, complete SQLite schema objects, and placed-slot identity
  validate in a read snapshot before journal conversion. The same authored DDL
  creates the registry and defines its exact schema; added triggers, indexes,
  columns, or weakened constraints reject without repair. Writable connections
  explicitly require and verify WAL, `synchronous=FULL`, and foreign-key checks;
  processes reference their run, endpoint rows their owning process, and events
  their run and process.
  Darwin additionally requests and verifies `fullfsync` and
  `checkpoint_fullfsync`. These settings do not by themselves establish filesystem
  publication ordering or a tested host-power-loss guarantee.
- **Liveness is observed against the OS** because PIDs are reused and records
  outlive processes; `ps` confirms process identity before it reports a status
  and writes nothing.
- **Registry readers own stored representation.** Endpoint rows are immutable:
  one per owning process and endpoint id, with the raw address spelling for exact
  comparison. Ready activation compares the complete recorded set with the
  verified set within its transaction; no path rewrites endpoint rows.
  Shared actionable-process SQL preserves each caller's multiplicity and conflict
  rules. Observation returns typed facts; `ps` alone projects public strings.
- **Transitions borrow registry context.** Connection, immutable identity, and
  redactor are borrowed together after revalidating the held slot guard. Each
  transition visibly selects its transaction
  mode and commits mutations with redacted events through one borrowed event record.
- **Endpoint evidence belongs to a process.** An endpoint claim is only as
  trustworthy as the process obligation behind it. Recording endpoint rows with
  their owning process means no claim can outlive, or exist without, a process
  that recovery must settle.
- **Session completion has one owner.** Workload transitions write only local
  evidence, so no task or service path can race the session over its aggregate
  outcome. `SessionProgress` distinguishes execution, finalizing a known outcome,
  and finalized execution; checked decoding and SQL constraints reject completion
  without an outcome.
- **Process lifetime belongs to the session.** Starting handles own
  startup guards and child resources; committed readiness consumes a starting
  handle into a ready owner. Failure settlement consumes that ownership, and
  session finalization stops every remaining service and settles its capture.
- **Direct-child reaping has one implementation.** A private child owner holds
  either the unreaped OS child or its immutable PID and exit status. Observation
  consumes the OS handle on exit; later observations return retained evidence,
  and direct kill cannot target the reaped alternative. Service and invocation
  cleanup share this owner while retaining separate containment and capture
  obligations. Ready-service and probe checkpoints observe this same child owner,
  retaining its exit status before teardown; service metadata has no separate
  liveness implementation.
- **Descendant evidence stays on the execution thread.** Checkpoints refresh it
  directly, so there is no shared monitor mutex or monitor thread whose failure
  could go unobserved.
- **Slot mutation has one owner.** A writable registry owns a non-cloneable
  slot guard through SQLite closure. Acquisition validates the private registry
  ancestry and a stable, non-followed lock file before admitting a writer.
  Workload spawning borrows that guard and consumes its command. The child closes
  the inherited ownership descriptor before exec, without unlocking the parent's
  shared lock; close failure refuses exec. Close-on-exec remains defense in depth.
  Task, prepare, service, and exec-probe spawning all use this boundary.

## Ports, state, containment

The normative endpoint rules are in
[Source, identity, and endpoints](CONTRACT.md#source-identity-and-endpoints);
state, deletion, and containment rules are in
[Registry, state, and processes](CONTRACT.md#registry-state-and-processes).

- **Ports: host-coordinated, positively observed readiness.** A per-euid
  endpoint lock serializes startup mutation across state roots. Exact-bind
  preflight checks availability. The endpoint owner observes sockets held by
  freshly enumerated, identity-checked containment members: managed FD inodes
  plus Linux SOCK_DIAG records, or full SDK-decoded macOS socket FD records.
  It retains positive evidence even when unrelated inspection is incomplete.
  It never needs negative host-wide inventory. A complete round owns initial
  witnesses, successful probe outcomes and matching final witnesses. Only its
  private completed value reaches the registry's atomic endpoint/ready/lifecycle
  transaction. Health repeats the same check without rewriting readiness.
  Missing witnesses retry; unresolvable uncertainty refuses. Locks end after
  ready commit and are not durable identity, liveness or responder attribution.
- **Coordination objects are opened through held directories.** The private
  runtime filesystem boundary owns non-following component access, private
  owner/mode checks, atomic close-on-exec, and opened-object/entry comparison.
  Lock files are regular, singly linked objects; opening an existing object
  never truncates or repairs it. Endpoint acquisition rechecks the entry after
  locking. These checks detect observed substitution; they do not protect
  managed ancestry against future same-user interference. Endpoint selection
  and lock lifetime remain with the endpoint owner.
- **Probe execution uses the common captured-child owner.** Application
  invocations retain liveness, cancellation, containment and capture checks
  while running. Endpoint-less services use the same round boundary with one
  scalar probe. Native socket creation serves bind preflight; Linux creates
  close-on-exec sockets atomically and macOS coordinates flag setup with every
  workload spawn. Coordination poisoning refuses effects.
- **Application data compatibility belongs to the application.** Only the
  application knows its data format, so a framework compatibility check could
  only delete or refuse data without real evidence. The marker records
  ownership, retention, data generation, and provenance. State preparation has
  no deletion branch; application startup owns format checks and migrations.
- **Slot ownership precedes recovery and state preparation.** Endpoint locks
  cover preflight, service prepare, spawn, and readiness after slot preparation.
- **Listener loss requires explicit teardown.** A missing listener is an
  observation, not proof of who owns the process, so neither `ps` nor endpoint
  acquisition signals on it: missing or unprovable ownership is
  `PORT_UNVERIFIABLE` and an outside listener is `PORT_CONFLICT`. The recorded
  obligations remain for the next exclusive owner's recovery.
- **The coordination boundary is deliberately narrow.** Endpoint locks
  coordinate participating runtimes for the same effective user and relevant
  network scope, not arbitrary external binders. An unrelated process can still
  race between preflight and the child's bind; a surviving competing listener is
  observed, while an ambiguous bind failure fails closed. Eliminating that
  window would require socket activation and descriptor handoff, which widens
  the generic invocation and adapter contracts, or a lifetime lock or guardian,
  which adds another supervision protocol. Nixfied therefore does not claim
  atomic reservation against arbitrary host processes.
- **State: one retention policy and marker-last deletion.** One `persistence`
  policy keeps retention a single fact that the marker, not a later manifest,
  carries. The marker stays until every payload entry is gone, so an interrupted
  deletion always leaves an attributable, resumable tree; one pending intent per
  slot means recovery resumes the recorded operation instead of guessing. User
  wording is in the [guide](GUIDE.md#services-slots-and-state).
- **Evidence outlives application data.** Disjoint `data/` and `registry/`
  namespaces let cleanup delete application state without any path that reaches
  registry or run evidence, even if an ownership marker is copied there.
- **Containment is runtime-owned.** A supervisor whose children form their own
  groups uses `process-tree` containment. Daemonization/double-fork without a
  stable handoff is refused as `PROC_ESCAPE`, but only after an exact
  process/run/event transaction durably records the escape while retaining open
  ports; registry failure takes precedence. (v1: containment differed by platform
  with no single owner.)

## Secrets

The contract carries secret descriptors, never secret values. Descriptors are
runtime references such as `env-var` and confined `file` resolvers; resolved
values belong only in runtime memory and hermetic child environments. REDACT-1 is
scoped to runtime-owned persistent output (logs, summaries, registry payloads,
captured child output, and error JSON), which is scrubbed before write; it cannot
govern files, cache contents, or sockets a child chooses to write on its own.

## Output Control

The output modes, capture, presenter, finalization, and error-precedence rules
are in [Output and failure contract](CONTRACT.md#output-and-failure-contract);
the launch ordering is in [Internal workload gate](CONTRACT.md#internal-workload-gate).

Task, preparation, and probe completion share one captured-child owner, and
services use the same owned capture workers, so containment, reaping, and
capture ordering have one implementation. Capture
workers own the evidence files and children receive only pipe writers; capture
completion is therefore checked, never inferred from a leader's exit.
`ObservedWithoutEvidence` preserves an observed task result when settlement
cannot produce completed evidence.

Presentation is separate from the session because a slow or stalled caller
stream must never delay supervision, teardown, retention, or slot release.
Evidence files are the data path; the command-owned `__presenter` only reads
them, so a stalled reader can block only the presenter.

The run session is the single finalization owner and runs every remaining
cleanup stage even when an earlier stage fails. One node runner executes prepare
and root occurrences, appending completed task records directly to the session's
canonical evidence vector. Root and selected projections retain private indices;
finalization derives owned output records once. Typed evidence crosses registry,
summary, and error boundaries without being reconstructed from serialized JSON.

Runtime failures use the same projection rule: default runtime execution prints a
human-readable error on stderr; `run --output json` prints the structured
`RuntimeError`; and `run --output both` prints human text followed by the JSON
error as the final stderr line. Registry/state refusals must point at the selected slot paths instead of
encouraging broad deletion: `REGISTRY_CORRUPT` is structural registry damage,
`STATE_UNOWNED` is project/environment/slot ownership mismatch, and
`RUNTIME_ABI_MISMATCH` is a runtime/toolchain contract mismatch.

## Verification boundary

A system cannot fully certify itself, so framework verification is split across
independent assurance layers: white-box Cargo tests grade the runtime primitives;
hermetic Nix checks grade source, manifest compilation, and builds; an
adopter-shaped gate exercises the runtime against realised manifests; and hosted CI
adds platform-specific coverage plus a release build. These are logical layers,
not one universal execution order. The current entrypoints and their exact order
are documented in [`DEVELOPMENT.md`](DEVELOPMENT.md).

The runtime-shaped portion is a first-class Nixfied manifest, while compiler and
installer cases remain ordinary shell around Nix. The latter perform open-ended
Nix builds and external fetches, which do not fit the bounded role of those
framework tasks and obscure which layer is under test. SEAM-1 itself constrains
the runtime binary; its exact scope is defined in
[`CONTRACT.md`](CONTRACT.md). Precise process-kill, registry, and recovery
scenarios similarly belong in white-box Cargo tests rather than bounded leaves.

Adopters have a different verification surface. They receive framework
discovery/control apps plus one app per task exported in
`nixfied.surface.verbs`; their checks and acceptance proofs are ordinary tasks
and composites run by the same runtime as the rest of their project. The
framework uses Cargo and Nix to grade its own implementation before relying on
its self-hosted gate.

A NixOS VM was considered and rejected: the gate already has the real Nix
daemon, ports, multi-process behavior, pinned inputs, Nix-built binaries, and
throwaway repositories/state it needs in a normal host shell.

## Definitional boundaries

The exhaustive normative list is in
[`CONTRACT.md`](CONTRACT.md#definitional-boundaries). These exclusions shape the
design rather than wait on a roadmap. Multi-host execution, a required daemon,
central log aggregation, and UI are outside a per-project, per-slot, single-host
authority. A manifest envelope re-opens the v1 artifact-sealing failure
(SINGLE-MANIFEST-1), while a dynamic runtime adapter protocol restores domain
awareness and v1 adapter complexity (RUNTIME-GENERIC-1). The no-daemon assumption
is especially load-bearing because it shapes the ownership/liveness model (LIVE-1).
Adding one of these concepts deliberately redefines the product.
