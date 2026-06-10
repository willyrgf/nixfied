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
test harnesses, CI jobs, and dev workflows — often polyglot, owned by different
tools. Their operational behavior is scattered across `flake.nix`, shell scripts,
package scripts, CI YAML, compose files, env files, and port conventions. Nothing
gives a project a clean, process-aware way to run multiple environments — or
multiple copies of one environment — while keeping services, workflows, state,
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
  target identity, generator/toolchain identity, closure metadata, runtime
  capabilities, layered service identity, state policy, and secret descriptors are
  first-class `model.json` fields — so the model *is* the admission contract, not
  just an execution graph.
- **Self-hash → computed provenance hash.** The model embeds no self-hash
  (circular). The runtime computes `computedModelHash = sha256(raw bytes)` at
  admission and records it in registry/summaries/logs/errors.
- **Model origin → Nix store output.** Normal admission requires `model.json`
  under the Nix store: a local origin/trust + immutability policy, not a proof of
  compiler provenance. A non-store escape hatch exists only for framework
  tests/dev (`--allow-non-store-model`).
- **No cross-version compatibility.** A model is valid only for the exact
  `runtimeAbi` / `toolchainId` that produced it. No migration layer; updating
  Nixfied means recompiling. This keeps the first implementation simple and
  honest, and the version integer bumps only on a real breaking change — never per
  milestone or docs rebuild.

## Correctness in four layers

1. **Model correctness (Nix).** Reject invalid names, broken references, cyclic
   workflows, malformed state/placement, missing closures, invalid primitives, or
   bad source/target policy — before runtime is even possible.
2. **Closure correctness (Nix).** Reproducibly realise the store paths the runtime
   may execute.
3. **Admission correctness (Rust).** Host checks Nix cannot prove: store origin,
   exact ABI/toolchain, target/capability support, source policy, already-realised
   closures, writable marker-owned state, registry acquisition, port ownership
   strategy, stale-lease reconciliation, secret policy.
4. **Execution correctness (Rust).** The impure graph: process groups, signals,
   readiness/health, tasks, workflow cancellation, registry events, summaries,
   cleanup, reconciliation.

This keeps Nix central to project evolution while preventing it from becoming a
live process supervisor.

## Identity & placement

Every runtime action is scoped by `projectId / environment / slot / runId`.

- **Source is explicit.** Every tree the runtime may observe is a declared
  `codebase` (logical root, source mode, dirty policy, fingerprint policy). Runtime
  operations observe source only through declared `codebaseId`s — never implicit
  cwd. (v1: live checkout state was implicit.)
- **Service identity is layered**, so harmless changes don't blur ownership:
  `serviceInstanceId = hash(serviceAddress, endpointIdentity, stateIdentity,
  runtimeCompatibilityHash, targetIdentity)`. Reuse requires an exact match across
  all layers. (v1: reuse blurred incompatible runtime configs.)
- **Placement is split by phase.** Nix bakes *logical* placement (template roots,
  relative layouts, candidate port windows) into the model; Rust materialises
  *host-absolute* placement at admission (`$NIXFIED_STATE_DIR` or a platform
  default); run-scoped paths use the `runId` known only at runtime. (v1: placement
  drifted when both sides derived it.)

## Registry, liveness, leases

- **Per-slot SQLite WAL registry** owns shared mutable state transactionally, with
  a total per-slot event order (timestamps are diagnostic only). It is a *durable
  record, not a liveness oracle*. (v1: the registry was treated as liveness truth.)
- **Liveness is reconciled against the OS** before being reported — `ps` confirms
  process identity (surviving PID reuse) before saying `running`/`stale`/etc.
- **Leases split three ways** — `run-scoped`, `until-idle`, `persistent-until-down`
  — because no daemon is guaranteed; reference counts derive from live borrower
  leases, never an independently mutated counter. (v1: lease/refcount semantics
  assumed a daemon that didn't exist.) Only `run-scoped` is implemented today.

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
  is idempotent and crash-safe.
- **Containment is capability-gated.** Services run foreground under a
  runtime-owned process group; cancellation propagates to the whole group; a
  process counts as started only after a registry record exists. A supervisor
  whose children form their own groups uses `process-tree` containment.
  Daemonization/double-fork without a stable handoff is refused as `PROC_ESCAPE`.
  (v1: containment differed by platform with no single owner.)

## Secrets

Full secrets management is deferred, but non-leakage is not. The model may carry
secret *references*, never values; persistent output (logs, summaries, registry,
errors) is redacted before write. The conservative current stance: only an empty
`secrets` section is accepted.

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
| A single huge proof workspace became a second framework | tiered proofs; later, self-hosted conformance |
| Duplicate command families per lifecycle action | model-derived public surfaces (SURFACE-1) |

## Self-hosted conformance (the gate)

A system cannot fully certify itself, so the gate is layered and runs in order:
(1) `cargo test` is the trusted floor — it verifies the runtime's own primitives
without the runtime grading itself; (2) the dogfood gate runs the framework's own
`conformance` workflow through the runtime under test, each check writing a
ground-truth verdict artifact; (3) `nix flake check` covers structural gates. The
repo's own `nixfied.nix` is therefore both the acceptance gate and the canonical
adopter example. A NixOS-VM was considered and rejected: the gate's needs (a real
Nix daemon, ports, multi-process supervisors, nested `nix`) already exist in a
normal shell, so hermeticity comes from pinned inputs + nix-built binaries +
throwaway repos/state, not a VM.

## Deferred by design

The first cut intentionally omits: full secrets/credentials management;
inter-service DAGs beyond workflow readiness gates; centralized log aggregation;
multi-host/remote execution; a required daemon; UI/dashboards; a manifest-sealed
bundle envelope; and a dynamic runtime adapter protocol. The last two are added
only if a concrete need (portable bundles, cache export/import, standalone
distribution; or generic primitives proving insufficient) appears. See AGENTS.md
for the current deferred list as it stands in the code.
