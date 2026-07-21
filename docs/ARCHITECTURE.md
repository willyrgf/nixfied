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
  evaluate to `model.json`; invalid intent never produces an admitted model. Nix
  builds/realises closures and emits views. It is also the public extension API:
  adopters integrate by importing a module, not by vendoring internals or
  reshaping their repo.
- **Rust is the hidden, generic execution runtime.** It knows no domain
  (Postgres/Node/Python/nginx). It executes generic primitives and enforces typed
  lifecycle semantics. Mutable runtime state — process ownership, ports, leases,
  reconciliation, cleanup, logs, summaries — is Rust's responsibility.

> Nix is the authority for what the project is *allowed to be*. Rust is the
> authority for what it is *currently doing*.

Three properties, deliberately distinct:

- **Codified & discoverable** — the whole project is typed data in `model.json`; a
  human or agent learns what it can do by reading the model or its views.
- **Deterministic model & logical placement** — same typed input ⇒ same compiled
  model and logical placement. Host-absolute paths are *not* part of this; Rust
  materialises them at admission.
- **Owned execution** — runtime timing/scheduling is not deterministic, but every
  process is owned, tracked, attributable, OS-reconciled, and cleanable.

This split creates subtractive design pressure: one seam and one owner per
responsibility make it possible to remove concepts, branches, public types, and
change sites instead of coordinating more of them. Net LOC reduction is useful
evidence when it removes semantic machinery while keeping the typed boundaries,
fail-closed checks, and independent seam proofs explicit.

## Why one `model.json` seam

The single most important hardening choice. Each clause replaced a v1 failure
mode:

- **Many artifacts → one semantic model.** No required `manifest.json` /
  `schema.json` / `capabilities.json`. `schema`/`docs`/`capabilities` are
  *disposable views*. (v1: artifact sealing kept growing toward a mini package
  format.)
- **Manifest hardening → model-owned admission contract.** Source identity,
  target identity, generator/toolchain identity, closure metadata, layered service
  identity, and state policy are first-class `model.json` fields — so the model
  *is* the admission contract, not just an execution graph. The contract is also
  *typed*: illegal shapes (a malformed lifecycle, a non-loopback endpoint, a zero
  timeout) are unrepresentable rather than caught by a rule, and the
  cross-references are proven by lowering the model into the executor's input.
- **Self-hash → computed provenance hash.** The model embeds no self-hash
  (circular). The runtime computes `computedModelHash = sha256(raw bytes)` at
  admission and records it in registry/summaries/logs/errors.
- **Model origin → Nix store output.** Normal admission requires `model.json`
  under the Nix store: a local origin/trust + immutability policy, not a proof of
  compiler provenance. A non-store escape hatch exists only for framework
  tests/dev (`--allow-non-store-model`).
- **Replacement instead of compatibility.** Exact ABI/toolchain matching means
  adopters recompile when the contract changes. Translators and dual
  implementations would multiply parsers, branches, tests, and future change
  sites; Git preserves removed implementations. The capability digest records
  every contract change, while numeric versions change only for their separately
  defined semantics.

## The task–service algebra (the composition rewrite)

The first external adoption exposed the original sin of the authoring
surface: it was the runtime's **wire format exposed
raw** — closures, invocations, operationIds, terminal tokens — so adopter concepts
with no runtime equivalent (a *command*, a *toolchain*, a *pipeline*, a
*verb*) escaped the model: below it into opaque shell dispatchers, above it
into the adopter's own flake. The accepted fix is a closed algebra of exactly
**two semantic kinds** (KIND-2):

- **task** — bounded, composable execution: a **leaf** (one inline
  invocation + `requires` + exit policy) or a **composite** (a static
  named-step DAG over task references; STATIC-1 — no parameters,
  conditionals, retries, or loops in the contract).
- **service** — durable execution: orchestrated, probed, owned, cleaned —
  with **endpoints optional**, because durable is not listening: a queue
  consumer or indexer is owned, invocation-probed, contained, and cleaned
  while binding no socket and claiming no port (PORT-1 stays fully scoped to
  declared endpoints).

The connective tissue is the **invocation** (`tools + run + env + cwd + timeout
+ stdin`) — the one way anything says "run this program".
Invocations are **anonymous and inline** (INVOKE-1): naming them for reuse would
recreate the named-invocation registry and its reuse/wiring entanglement;
content reuse is a Nix `let`, and the model carries the fully-applied copies.
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
`nixfied.surface.verbs` (VERB-1: control verbs — `run`, `ps`, `down`,
`clean`, `model-check` — are framework-reserved; project verbs derive only from
adopter-exported task names). Environment **membership does not exist**:
running a task brings up exactly the services its leaves require —
`servicesRequired`, `operationBindings`, and operation ids are **derived**
from the graph (DERIVE-1), computed identically by the Nix compiler and the
runtime's lowering against one normative source
(`docs/DERIVATION_SPEC.md`), and compared fail-closed at admission.
Hand-declaration is reserved for *choices* (surface verbs) and *attestations*
(effects). Child environments are **hermetic**: declared env plus the
runtime-owned PATH assembled from the tool roots, nothing inherited.

## Correctness in four layers

1. **Model correctness (Nix).** Reject invalid names, broken references, cyclic
   task graphs, malformed state/placement, missing closures, invalid primitives, or
   bad source/target policy — before runtime is even possible.
2. **Closure correctness (Nix).** Reproducibly realise the store paths the runtime
   may execute.
3. **Admission correctness (Rust).** Host checks Nix cannot prove: store origin,
   exact ABI/toolchain, target support, source policy, already-realised closures,
   reference resolution (the model lowers into the executor's input only if every
   cross-reference exists), writable marker-owned state, registry acquisition, port
   ownership strategy, and stale-lease reconciliation.
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
- **Service identity is layered**, so harmless changes don't blur ownership:
  `serviceInstanceId = hash(serviceAddress, endpointIdentity, stateIdentity,
  runtimeCompatibilityHash, targetIdentity)`. Reuse requires an exact match across
  all layers. (v1: reuse blurred incompatible runtime configs.)
- **Placement is split by phase.** Nix bakes only the *logical* placement the
  model needs — the per-slot candidate port windows — into `model.json`; the
  directory layout (state root, registry, run/logs/artifacts) is a runtime-owned
  constant, and Rust materialises *host-absolute* placement at admission
  (`$NIXFIED_STATE_DIR` or a platform default), with run-scoped paths using the
  `runId` known only at runtime. (v1: placement drifted when both sides derived it;
  the layout templates were later pinned constants in the model, then removed.)

## Registry, liveness, leases

- **Per-slot SQLite WAL registry** owns shared mutable state transactionally, with
  a total per-slot event order (timestamps are diagnostic only). It is a *durable
  record, not a liveness oracle*. (v1: the registry was treated as liveness truth.)
- **Liveness is reconciled against the OS** before being reported — `ps` confirms
  process identity (surviving PID reuse) before saying `running`/`stale`/etc.
- **Registry-only evidence fails closed.** A live reservation lease without a
  process is `LEASE_CONFLICT`; a post-reconcile row with neither a valid lease
  nor a valid process is `REGISTRY_CORRUPT`.
- **Service state is derived.** The `services` row stores identity, lifetime,
  and state-root metadata; process readiness comes from the primary process and
  active ports, while standing and borrowing derive from owner/borrower leases.
  `ps` projects those facts without a separately persisted service status.
- **Leases split three ways** — `run-scoped`, `until-idle`, `persistent-until-down`
  — because no daemon is guaranteed; reference counts derive from live borrower
  leases, never an independently mutated counter. `until-idle` services are
  torn down lazily on the next runtime invocation after the last borrower is
  gone or stale; `persistent-until-down` services stand until `down` releases
  them. (v1: lease/refcount semantics assumed a daemon that didn't exist.)
- **Open leases are replacement authority.** Exact healthy reuse may admit
  another borrower while the owner and existing borrowers remain open. If the
  service is not exactly reusable, any open owner or borrower lease returns
  `LEASE_CONFLICT`; acquisition never mutates the lease or signals its process.
  Expired process-less reservations are reclaimed by ordinary reconciliation.

## Ports, state, containment

- **Ports: host-coordinated, ownership-verified readiness.** Pure derivation and
  per-state-root registry reservations are insufficient because TCP endpoints
  are host resources. A fixed per-euid endpoint lock serializes
  service-specific mutation across independent roots; Linux `SOCK_DIAG` and
  macOS `net.inet.tcp.pcblist_n` provide kernel listener truth. A service is
  ready only when its probe succeeds and every exact endpoint is held by the
  expected containment. Wildcards conflict but do not satisfy an exact
  declaration. Complete observation with no exact listener remains pending and
  ends as `READINESS_TIMEOUT`; incomplete ownership proof is
  `PORT_UNVERIFIABLE`. Locks end after the atomic ready commit; sockets remain
  steady-state ownership. The lock is transient coordination, never durable
  service identity, liveness evidence, or owner attribution; the registry
  remains the only durable runtime authority.
- **Lock scope begins after slot preparation.** Marker adoption, epoch handling,
  registry opening, and mandatory reconciliation remain run-wide. Endpoint
  locks begin when the pre-lock exact-reuse attempt does not succeed and cover
  the under-lock reuse check, local reservation, service prepare, spawn, and
  readiness.
- **Listener loss requires explicit teardown.** `ps` remains process liveness
  and never signals solely because an endpoint is missing. Endpoint acquisition
  also never signals a pre-existing process: missing or unprovable ownership is
  `PORT_UNVERIFIABLE`, an outside listener is `PORT_CONFLICT`, and an open lease
  is `LEASE_CONFLICT`. The recorded process and ownership evidence remain
  actionable to `down` and cleanup; after explicit `down`, a later run may start
  the replacement.
- **The coordination boundary is deliberately narrow.** Endpoint locks
  coordinate participating runtimes for the same effective user and relevant
  network scope, not arbitrary external binders. An unrelated process can still
  race between preflight and the child's bind; a surviving competing listener is
  observed, while an ambiguous bind failure fails closed. Eliminating that
  window would require socket activation and descriptor handoff, which widens
  the generic invocation and adapter contracts, or a lifetime lock or guardian,
  which adds a supervision protocol even though persistent services outlive the
  invoking runtime. Nixfied therefore does not claim atomic reservation against
  arbitrary host processes.
- **State: marker-gated, path-confined cleanup.** Every owned state root carries a
  `.nixfied-state.json` marker. Cleanup canonicalizes first; refuses paths outside
  the state base, target symlinks, traversal escapes, unmarked roots, marker
  mismatches, active leases/processes/reservations, and policy-protected
  persistent state; unlinks symlink entries inside the owned tree without
  following them; and is idempotent and crash-safe. `clean --purge` expresses
  deliberate destruction of protected/persistent state, but it relaxes only that
  policy gate.
- **Containment is runtime-owned.** Services run foreground under a runtime-owned
  process group; cancellation propagates to the whole group; a process counts as
  started only after a registry record exists. A supervisor whose children form
  their own groups uses `process-tree` containment. Daemonization/double-fork
  without a stable handoff is refused as `PROC_ESCAPE`, but only after an exact
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

`run` has three output projections selected by runtime-owned flags. `summary`
is the default: progress, concise pass/fail summaries, and pointers to the run
summary and log directory on stderr, with stdout empty. `json` (`--json`) is the
structured automation contract: run/task/node results, diagnostic `durationMs`,
and evidence paths on stdout, with human run-summary narration suppressed.
`both` (`--both`) emits both projections explicitly for diagnostics. Captured
child stdout/stderr stays in redacted log files; the runtime does not inline or
replay child bytes, and no `logs` command is part of the public surface.

Runtime failures use the same projection rule: default runtime execution prints a
human-readable error on stderr; `run --json` prints the structured `RuntimeError`;
and `run --both` prints human text followed by the JSON error as the final stderr
line. Registry/state refusals must point at the selected slot paths instead of
encouraging broad deletion: `REGISTRY_CORRUPT` is structural registry damage,
`STATE_UNOWNED` is project/environment/slot ownership mismatch, and
`RUNTIME_ABI_MISMATCH` is a runtime/toolchain contract mismatch.

## What v1 taught us (and the invariant each lesson produced)

| v1 failure | Resulting decision |
| --- | --- |
| Runtime semantics drifted into shell + Nix builders, no single owner | SEAM-1 / SHELL-1 / NIX-1; the two-binary split |
| Artifact sealing grew toward a package format | single model seam + computed provenance hash + model-owned admission |
| Live checkout state was implicit | first-class `codebases` |
| Registry treated as liveness truth | OS reconciliation (LIVE-1) |
| Pure port derivation treated as sufficient | ownership-verified readiness (PORT-1) |
| Placement drifted across Nix and runtime | logical placement in Nix, host materialisation in Rust |
| Service reuse blurred incompatible configs | layered service identity, exact match (SVC-ID-1) |
| Lease/refcount assumed a daemon | run/service/borrower lease split |
| Adapter complexity preceded proven lifecycle | generic primitives before concrete adapters (RUNTIME-GENERIC-1) |
| A single huge proof workspace became a second framework | tiered proofs; later, the thin self-hosted gate |
| Duplicate command families per lifecycle action | framework-owned public surfaces (SURFACE-1, since split) |
| The authoring surface was the wire format; the first adopter's commands/toolchain/verbs escaped into shell and its flake | the task–service algebra: vocabulary as names over a closed algebra, derived facts, the adopter-owned verb surface (KIND-2 / INVOKE-1 / STATIC-1 / DERIVE-1 / VERB-1) |
| The endpoint requirement conflated durable with listening; the non-listening worker shape was unrepresentable | endpoint-optional services with probe/placeholder/effects coherence; PORT-1 restated scoped to declared endpoints |

## Verification boundary

A system cannot fully certify itself, so framework verification is split across
independent assurance layers: white-box Cargo tests grade the runtime primitives;
hermetic Nix checks grade source, model compilation, and builds; an
adopter-shaped gate exercises the runtime against realised models; and hosted CI
adds platform-specific coverage plus a release build. These are logical layers,
not one universal execution order. The current entrypoints and their exact order
are documented in [`DEVELOPMENT.md`](DEVELOPMENT.md).

The runtime-shaped portion is a first-class Nixfied model, while compiler and
installer cases remain ordinary shell around Nix. The latter perform open-ended
Nix builds and external fetches, which do not fit the bounded role of those
framework tasks and obscure which layer is under test. SEAM-1 itself constrains
the runtime binary; its exact scope is defined in
[`CONTRACT.md`](CONTRACT.md). Precise process-kill, registry, and recovery
scenarios similarly belong in white-box Cargo tests rather than bounded leaves.

Adopters have a different verification surface. They receive the reserved
control apps plus one app per task exported in `nixfied.surface.verbs`; their
checks and acceptance proofs are ordinary tasks and composites run by the same
runtime as the rest of their project. The framework uses Cargo and Nix to grade
its own implementation before relying on its self-hosted gate.

A NixOS VM was considered and rejected: the gate already has the real Nix
daemon, ports, multi-process behavior, pinned inputs, Nix-built binaries, and
throwaway repositories/state it needs in a normal host shell.

## Definitional boundaries

The exhaustive normative list is in
[`CONTRACT.md`](CONTRACT.md#definitional-boundaries). These exclusions shape the
design rather than wait on a roadmap. Multi-host execution, a required daemon,
central log aggregation, and UI are outside a per-project, per-slot, single-host
authority. A manifest envelope re-opens the v1 artifact-sealing failure
(SINGLE-MODEL-1), while a dynamic runtime adapter protocol restores domain
awareness and v1 adapter complexity (RUNTIME-GENERIC-1). The no-daemon assumption
is especially load-bearing because it shapes the lease/liveness model (LIVE-1).
Adding one of these concepts deliberately redefines the product.
