# Nixfied Architecture & Design Rationale

This is the *why* behind Nixfied's design. It is condensed from the original
build spec (RFC v2.3), which has since been retired from the tree — its
decisions live on here and in git history.

- **README.md** — what Nixfied is and how to use it.
- **AGENTS.md** — the contract: invariants, boundaries, project map, checks.
- **This file** — the reasoning behind those invariants, and the v1 mistakes that
  motivated them.

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
- **No cross-version compatibility.** A model is valid only for the exact
  `runtimeAbi` / `toolchainId` that produced it. The `runtimeAbi` is *derived*: its
  suffix is a digest of the capability descriptor, hashed identically by Rust and
  Nix, so any contract change rotates it on both sides and old models are rejected.
  No migration layer; updating Nixfied means recompiling. This keeps the first
  implementation simple and honest, and the version integer bumps only on a real
  breaking change — never per
  milestone or docs rebuild.

## The task–service algebra (the composition rewrite)

The first external adoption (`PROBLEM_COMPOSITION.md`) exposed the original
sin of the authoring surface: it was the runtime's **wire format exposed
raw** — closures, invocations, operationIds, terminal tokens — so adopter concepts
with no runtime equivalent (a *command*, a *toolchain*, a *pipeline*, a
*verb*) escaped the model: below it into opaque shell dispatchers, above it
into the adopter's own flake. The accepted fix (`DESIGN_COMPOSITION.md`) is a
closed algebra of exactly **two semantic kinds** (KIND-2):

- **task** — bounded, composable execution: a **leaf** (one inline
  invocation + `requires` + exit policy) or a **composite** (a static
  named-step DAG over task references; STATIC-1 — no parameters,
  conditionals, retries, or loops in the contract).
- **service** — durable execution: orchestrated, probed, owned, cleaned —
  with **endpoints optional**, because durable is not listening: a queue
  consumer or indexer is owned, invocation-probed, contained, and cleaned
  while binding no socket and claiming no port (PORT-1 stays fully scoped to
  declared endpoints).

The connective tissue is the **invocation** (`tools + run + env + cwd +
timeout + stdin`) — the one way anything says "run this program". Invocations
are **anonymous and inline** (INVOKE-1): naming them for reuse would recreate
the named-invocation registry and its reuse/wiring entanglement; content reuse is a Nix
`let`, and the model carries the fully-applied copies.

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
runtime-owned PATH (assembled from the tool roots), nothing inherited.

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
- **Leases split three ways** — `run-scoped`, `until-idle`, `persistent-until-down`
  — because no daemon is guaranteed; reference counts derive from live borrower
  leases, never an independently mutated counter. `until-idle` services are
  torn down lazily on the next runtime invocation after the last borrower is
  gone or stale; `persistent-until-down` services stand until `down` releases
  them. (v1: lease/refcount semantics assumed a daemon that didn't exist.)

## Ports, state, containment

- **Ports: ownership-verified readiness.** Pure derivation and registry
  reservations are both insufficient — the registry coordinates Nixfied processes,
  not the OS. A service is ready only when the *expected owned process* is proven
  to hold the *expected endpoint*. (v1: pure port derivation was treated as
  sufficient.)
- **State: marker-gated, path-confined cleanup.** Every owned state root carries a
  `.nixfied-state.json` marker. Cleanup canonicalizes first; refuses paths outside
  the state base, symlink/traversal escapes, unmarked roots, marker mismatches,
  active leases/processes/reservations, and policy-protected persistent state; and
  is idempotent and crash-safe. `clean --purge` expresses deliberate destruction
  of protected/persistent state, but it relaxes only that policy gate.
- **Containment is runtime-owned.** Services run foreground under a runtime-owned
  process group; cancellation propagates to the whole group; a process counts as
  started only after a registry record exists. A supervisor whose children form
  their own groups uses `process-tree` containment. Daemonization/double-fork
  without a stable handoff is refused as `PROC_ESCAPE`. (v1: containment differed
  by platform with no single owner.)

## Secrets

The contract carries secret descriptors, never secret values. Descriptors are
runtime references such as `env-var` and confined `file` resolvers; resolved
values belong only in runtime memory and hermetic child environments. REDACT-1 is
scoped to runtime-owned persistent output (logs, summaries, registry payloads,
captured child output, and error JSON), which is scrubbed before write; it cannot
govern files or sockets a child chooses to write on its own.

## Output Control

`run` has three output projections selected by runtime-owned flags. `summary`
is the default: progress, concise pass/fail summaries, and pointers to the run
summary and log directory on stderr, with stdout empty. `json` (`--json`) is the
structured automation contract: run/task/node results, diagnostic `durationMs`,
and evidence paths on stdout, with human run-summary narration suppressed.
`both` (`--both`) emits both projections explicitly for diagnostics. Captured
child stdout/stderr stays in redacted log files; the runtime does not inline or
replay child bytes, and no `logs` command is part of the public surface.

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

## The framework gate

A system cannot fully certify itself, so the gate is layered and runs in order:
(1) `cargo test` is the trusted floor — it verifies the runtime's own primitives
without the runtime grading itself; (2) **`gate-runtime`** (`nix/gate-runtime/nixfied.nix`)
is a first-class nixfied model that exercises the runtime as adopters do — parallel
example runs with emitted-view diffs, concurrent slot isolation, runtime-layer negatives,
and a sequential state lifecycle matrix (adopt, in-place upgrade, epoch-bump clean,
tampered-marker refusal) — all expressed as nixfied tasks with no bespoke orchestration;
(3) **`gate-nix`** (`nix/gate-nix.nix`) covers the Nix layer in bash: `reject_composite`
(invalid composites must fail at evaluation), a `nix eval` duplicate-step assertion, and
the full `install`/`upgrade` adoption loop — these stay in bash because they invoke the
Nix build system directly, which violates bounded-execution semantics if expressed as
tasks; (4) `nix flake check` covers the structural gates. A thin coordinator
(`nix/gate.nix`) runs gate-runtime then gate-nix.

The interrupt-and-recover scenario — kill the runtime mid-flight; verify the next run
reconciles the orphaned service and adopts persisted state — lives in the white-box cargo
test floor (`tests/lifecycle.rs`). A bounded task cannot hold a registry connection or
control process timing across a kill; cargo can. This is consistent with the
already-stated principle that cancellation/GC/lifecycle invariants are white-box cargo
tests.

A NixOS-VM was considered and rejected: the gate's needs (a real Nix daemon, ports,
multi-process supervisors, nested `nix`) already exist in a normal shell, so hermeticity
comes from pinned inputs + nix-built binaries + throwaway repos/state, not a VM.

## Verification surfaces (framework vs adopter)

Two audiences, two surfaces, one rule. **Adopters** get the reserved control
apps (`run` / `ps` / `down` / `clean` / `model-check`) plus one app per task id they
export in `nixfied.surface.verbs` (`lib.projectApps`, wired by the `install`
scaffold); their verification *is* composition, because their tests are
tasks — a 0-service `check` (lint/test), an N-service `e2e` — composed into
the composites they name and run by the same runtime that runs their app,
with no separate harness. **The framework** verifies its own Rust/Nix with
plain `nix flake check` (a hermetic rustfmt/clippy/check derivation), a
`cargo` test floor, and the gate; it does *not* route its source checks
through nixfied tasks. The reason is the invariant that makes the whole
design work: a nixfied task can never invoke Nix (SEAM-1), so `nix build` /
`nix flake check` cannot be tasks; and the runtime must not be the instrument
that grades its own unit tests. So `.#gate` is framework-only — a system
proving its own runtime — while an adopter's acceptance proof is simply
another task they declare and export (`nix run .#release`, or
`.#run -- --task <id>` for anything unexported).

## Definitional boundaries

Some things are not roadmap items but **definitional non-goals** — the design
is what it is because it excludes them. Multi-host/remote execution, a required
daemon, central log aggregation, and UI/dashboards are outside a per-project,
per-slot, single-host authority; a manifest-sealed bundle envelope re-opens the
v1 artifact-sealing failure (SINGLE-MODEL-1); a dynamic runtime adapter protocol
re-opens v1 adapter complexity and would give the runtime domain awareness
(RUNTIME-GENERIC-1). The no-daemon assumption in particular is load-bearing:
the lease/liveness model (LIVE-1) is shaped around it. These are not deferred;
adding any of them is a deliberate redefinition of the product, not a backlog
pickup.
