# RFC v2 (v3 merge): Greenfield Nixfied Build Spec

Date: 2026-06-03
Status: Draft (build-ready)
Merges: `RFC_v2.md` and `RFC_v2cx.md` into one spec. Where the two diverged, the resolution is noted inline as **[fork]**.
Supersedes: all prior Nixfied implementation. This branch is a from-scratch rebuild; nothing from v1 is preserved except the lessons captured in [Constraints From v1](#constraints-from-v1).

## Decision Summary

Nixfied v2 is a greenfield rebuild. The existing framework, proof workspaces, shell sidecars, historical command structure, and repository layout are not compatibility constraints.

The load-bearing architecture decision:

- **Nix is the authority for what the project is allowed to be** — codification, static correctness, and reproducible runtime inputs (closures).
- **Rust is the authority for what the project is currently doing** — admitted execution, observed runtime state, process ownership, and cleanup.
- Nix compile/check phases produce immutable, versioned, content-hashed artifacts consumed by the Rust execution engine. The primary artifact is `model.json`.
- The Rust execution engine never evaluates Nix after **execution admission** begins.
- Shell may exist as a user convenience or an adapter implementation detail, never as the owner of graph, registry, summary, validation, process-lifecycle, or cleanup semantics.

This is a build spec, not a migration plan.

## Problem

Modern projects are not one program. They are small systems: frontend, API, workers, databases, queues, object storage, migrations, test harnesses, infra scripts, CI jobs, and local developer workflows, often written in different languages and owned by different tools.

The operational behavior of those systems is usually scattered across `flake.nix`, shell scripts, package-manager scripts, CI YAML, Docker compose files, service wrappers, env files, port conventions, and local tribal knowledge.

The deeper problem is that projects have no clean, process-aware way to run multiple environments, or multiple copies of the same environment, while keeping services, workflows, state, logs, ports, artifacts, and cleanup under one authority.

Without first-class environment, slot, state, placement, and registry concepts:

- `dev`, `test`, `ci`, preview, and prod-like runs collide.
- Two copies of the same environment cannot run predictably.
- Ports and mutable state leak between runs.
- Services are started by one script and stopped by another, if they are stopped at all.
- Workflows cannot reliably know which service instance they own.
- CI failures leave weak evidence about what ran, what was canceled, and what survived.
- Polyglot codebases need too much glue to coordinate simple local and CI execution.

Nixfied v2 exists to solve that problem.

## Product Statement

Nixfied v2 is a deterministic project model, correctness, and placement system with an owned Rust execution runtime for project-shaped systems.

It codifies project intent in Nix, evaluates that intent to typed correctness, compiles it into canonical artifacts, then runs those artifacts through one Rust runtime that can start, inspect, record, reconcile, cancel, and clean up the execution graph across environments, slots, services, workflows, and codebases.

Three properties define the product, and they are not the same:

- **Codified & discoverable.** The entire project — every capability, service, task, workflow, environment, and its constraints — is expressed as typed data. A human or agent can discover what the project can do and how to use it by reading the compiled model, without studying the source or assuming conventions.
- **Deterministic model & placement.** Given the same typed input, the compiled model, its hash, the derived identities, and the derived placement (paths, state roots, registry keys, candidate port windows) are byte-for-byte reproducible across machines.
- **Owned execution.** Runtime behavior (timing, readiness, scheduling, external services) is *not* deterministic. The runtime instead guarantees that every process is owned, tracked, attributable, reconciled against OS reality, and cleanable.

Nix is the authority for what the project is allowed to be. Rust is the authority for what the project is currently doing.

Nixfied is not primarily a task runner, service template collection, CI wrapper, Docker replacement, or Nix scaffold. Those may exist as surfaces or adapters, but the product is the model/runtime contract.

## The Load-Bearing Decision: Two Tools, One Seam

The split is defined by the **verb**, not by timing. "Nix at runtime" conflates three different operations; we separate them:

| Verb | Example | Owner |
|------|---------|-------|
| **Evaluate** | flake → derivations, type-checks, module assertions | **Nix**, at authoring surfaces (`compile`, `validate --deep`) only |
| **Build / realise** | the postgres binary, an adapter's `start` closure | **Nix** builds it; the result is a store path pinned in the model and *used* at runtime |
| **Enforce** | "is this runtime port override valid?" | **Rust**, against rules Nix authored and compiled into a schema |

1. **Nix is the source of truth and the correctness gate.** Typed Nix modules, with no impure access, *evaluate* into the compiled output. Nix also *builds* the closures the runtime needs — dependency binaries, adapter operations, helper scripts — and pins them into the model as store paths. Nix never starts a process, never touches mutable state, never observes the running system.

2. **Rust is the runtime, and the only runtime.** The execution engine reads the compiled output and owns everything impure: admission checks, process supervision, process-group ownership, signal propagation, readiness/health polling, the registry, state placement, cancellation, reconciliation, and cleanup. It freely *uses* and *realises* Nix's outputs but never *evaluates*.

These two halves communicate through one compiled output and one rule:

> **Invariant SEAM-1 (no evaluation after admission).** Nix *evaluation* is confined to the explicit `compile` and `validate --deep` surfaces. Once [execution admission](#correctness-layers) begins, nothing — `run`, `up`, `down`, `ps`, `svc`, reconciliation, GC — triggers evaluation. The engine reads the compiled output, uses pinned store paths, and enforces the compiled schema.

**[fork — one binary vs two]** SEAM-1 is enforced *structurally*, not by convention, by splitting the binaries (adopted from `RFC_v2cx`):

- **`nixfied`** — the ergonomic, integrated CLI. May run an explicit Nix `compile`/`check` phase, then delegate to the engine. (`nfd` is an accepted short alias.)
- **`nixfied-runtime`** — the pure execution engine. Consumes compiled artifacts by path and **cannot** evaluate Nix. This is where SEAM-1 lives as a property of the binary, not a promise.

### Correctness is preserved by construction, not by runtime evaluation

Confining evaluation to authoring time does **not** weaken Nix's correctness guarantee — it strengthens it:

> **The runtime can only ever execute a model that is the output of a successful Nix evaluation.** A config that fails a type-check or assertion never produces a compiled output, so it can never run. Nix's correctness gate therefore covers 100% of runs — as a precondition of existence, not a per-run check.

For an agent or human iterating programmatically, this is the better contract: correctness feedback arrives at `compile` time as a typed error, never as a mid-run crash, and the surface they read to discover capabilities is stable, typed JSON — not a Nix expression they must evaluate and hold in their head.

### Why Rust

- Real process-group control, signals, `pidfd`, async I/O, and timeouts are first-class.
- A single static binary is trivial to ship inside a Nix derivation and to exec from a thin flake app.
- Strong typing mirrors the "typed model" thesis end-to-end: the compiled output deserializes into Rust types, and a malformed model fails loudly at the boundary.
- Crash-safety primitives (atomic file writes, advisory locks, fsync) are available without ceremony.

## Core Concepts

- **Model:** the compiled contract for what exists and how it can run.
- **Model artifact:** the immutable, hashed `model.json` produced by Nix and consumed by the engine.
- **Closure:** a reproducible executable, adapter operation, helper, schema, or generated script produced by Nix and referenced by the model as a store path.
- **Environment:** a named mode of operation, such as `dev`, `test`, `ci`, preview, or prod-like.
- **Slot:** a deterministic parallel instance of an environment, identified by an integer index.
- **Run:** one runtime invocation with a unique run identity.
- **Placement:** derived paths, ports, state roots, log roots, artifact roots, and registry keys for a `(project, env, slot, run)`.
- **State:** all mutable roots derived from project, environment, slot, service, workflow, and run identity. Owned and cleaned by the runtime.
- **Registry:** a durable, reconciled projection of runtime state and history — *not* the source of truth for liveness. The OS owns liveness.
- **Service:** a long-lived process with lifecycle, readiness, health, logs, state, and ownership.
- **Workflow:** a dependency graph of tasks and service phases with artifacts, summaries, cancellation, and cleanup.
- **Adapter:** language-, service-, or tool-specific implementation behind a stable, typed model/runtime contract.

## Architecture & Data Flow

```text
typed Nix modules
      │
      ▼
Nix correctness + compiler passes      (pure: resolve → validate → derive → emit)
      │                                 + build/realise closures → store paths
      ▼
compiled output  ── the ONLY seam (immutable, content-hashed, versioned)
      ├─ model.json        typed model (schemaVersion + modelHash)
      ├─ schema.json       runtime-input validation rules the engine enforces
      ├─ capability catalog generated, agent-readable
      └─ closures          pinned dep binaries, adapter ops, helper scripts
      │
      ▼
nixfied-runtime  (Rust engine, the single execution authority)
      ├─ model loader + schema enforce
      ├─ admission (host checks, locks, reservations)
      ├─ orchestrator / workflow runner
      ├─ service dispatcher
      ├─ readiness + health probes
      ├─ process-group owner + signal propagation
      └─ registry reconciler
      │                                   ▲ uses store paths; may `nix build <drv>`
      ▼                                   │ but never evaluates (SEAM-1)
registry, state roots, logs, artifacts, summaries
```

The flake apps and the ergonomic `nixfied` CLI are thin: they resolve the cached compiled output and `exec` the engine. They contain no execution semantics.

### Boundary Rules

- Nix validates typed project intent and emits `model.json`, schemas, capability catalogs, and reproducible store paths for runtime commands and adapter operations.
- The engine reads compiled artifacts and performs all impure execution.
- The engine never evaluates Nix, imports Nix modules, shells to `nix eval`, or mutates the model artifact after admission begins.
- The ergonomic CLI may run an explicit compile/check phase before execution, but the resulting model **path and hash** must be recorded as the input to the admitted run.
- Adapter code may use shell internally, but shell output is never a semantic sidecar. Semantic state returns through typed operation results or runtime-owned observation.

## Correctness Layers

Nixfied v2 separates desired-state correctness from observed-runtime correctness across four layers. **[adopted from RFC_v2cx — the cleanest decomposition of the seam.]**

1. **Model correctness (Nix).** Reject invalid names, broken references, incompatible adapter declarations, invalid workflow edges, missing fields, and malformed state/placement policy before runtime is possible.
2. **Closure correctness (Nix).** Reproducibly construct the commands the runtime may execute — runtime/task/adapter closures, helpers, generated scripts, schemas, docs. The engine receives executable paths from the model instead of discovering tooling heuristically.
3. **Admission correctness (Rust).** Host-specific checks Nix cannot prove: port availability, writable state roots, registry lock acquisition, stale-lease reconciliation, executable availability on this host, ownership conflicts, env-specific preconditions. **Admission is the named moment after a model is selected and before any long-lived service starts** — and the moment SEAM-1 begins.
4. **Execution correctness (Rust).** The impure graph: process groups, signals, readiness/health, task execution, workflow cancellation, registry events, summaries, cleanup, reconciliation.

This keeps Nix central to project evolution and agent-readable codification while preventing Nix evaluation from becoming a live process supervisor.

## The Compiled Output (the seam)

One immutable, content-addressed directory — the entire contract between Nix and Rust.

### `model.json`

Versioned, serde-typed, owned by the shared `nixfied-model` crate. Required top-level fields:

- `schemaVersion` — integer, initially `1`. The engine refuses an unknown major version.
- `modelHash` — hash of the canonical model excluding `modelHash`. **[adopted from RFC_v2cx]** The engine validates it before execution; every admitted run records the path and hash it used. Canonicalization must be stable across machines for the same typed input.
- `runtime` — minimum compatible runtime version and feature requirements. Gives bidirectional version negotiation (model ⇄ engine), not just one-directional.
- `project` — stable project metadata and `projectId`.
- `environments`, `services`, `workflows`, `adapters`.
- `placement` — placement policy plus derived, run-independent roots and candidate port windows (see [Identity & Placement](#identity--placement)).
- `state` — state policy and root declarations.
- `closures` — store paths for runtime-dispatched tasks, adapters, and helper operations.

### `schema.json`

The validation rules for **runtime inputs** (slot index ranges, allowed env names, port-override bounds, collision policy, …). These rules are *authored in Nix* and *enforced in Rust*; the engine does not re-derive them.

### Pinned closures

The realised dependency binaries, adapter operations, and helper scripts the engine executes, referenced by store path. Evaluated and (optionally) built at compile time; the engine uses them and may lazily `nix build <drv>` a pinned derivation, but never evaluates.

> Model-artifact rules: `model.json` is immutable for a run; the engine receives the model path explicitly (e.g. `nixfied-runtime run --model ./result/model.json --env dev --slot 0`); the ergonomic wrapper may produce the model first (e.g. `nixfied up dev`) but the admitted run is still tied to one concrete model path and hash.

## The Iteration Loop (the deterministic correctness gate)

Evolving a project is one loop, and that loop *is* the correctness gate:

```text
edit typed Nix  →  nixfied compile  (Nix eval: type-checks + assertions)  →  new compiled output  →  run
                          │
                          └─ on error: typed compile-time failure, nothing runs
```

This loop must be **fast and first-class**, not hidden, because it is how humans and agents iterate safely:

- `nixfied compile` is the only thing that triggers evaluation on the common path; the dev shell offers a watch mode that recompiles on change.
- Compile failures are typed and actionable, surfaced before any process starts.
- `nixfied validate --deep` is an explicit, opt-in surface that *may* invoke Nix to re-check live config against the live filesystem (e.g. referenced source dirs exist). It is never part of `run`/`ps`/reconcile, so it does not violate SEAM-1.
- Shallow `nixfied validate` (no Nix) checks the already-compiled output and the live filesystem using `schema.json` — the fast, hot-path-safe check.

## Identity & Placement

Every runtime action is scoped by explicit identity:

```text
projectId / environment / slot / runId
```

- `projectId` — stable, explicit identifier from the model.
- `environment` — model-defined environment name.
- `slot` — user-selected deterministic parallel-instance index (`0..N`, default `0`).
- `runId` — runtime-created unique ID per invocation. **[fork — run ID format]** Use a **ULID**: lexicographically sortable by creation time, encodes the timestamp, and is contention-safe — superseding `RFC_v2cx`'s `YYYYMMDD-HHMMSS-<suffix>` while keeping time-ordering and readability.

### Placement derivation **[fork — resolved]**

`RFC_v2` derived placement in both Nix and Rust (guarded by a parity test); `RFC_v2cx` derived it only in Rust from model-supplied inputs. Resolution that removes the drift risk *and* keeps discoverability:

- **Pure, run-independent placement is derived by Nix and baked into `model.json`** (`placement` field): state roots, registry keys, log/artifact roots per `(project, env, slot)`, and candidate port windows. Discoverable directly from the artifact; no Rust derivation needed.
- **Run-scoped paths are composed at runtime** by joining `runId` onto the baked roots (`run_dir = run_root/<runId>/…`), since `runId` does not exist at compile time.
- **Port binding is runtime** (see below).

This means derivation logic exists in exactly one place per phase — pure parts in Nix (baked), impure composition in Rust — so there is no dual implementation to keep in sync.

```text
state_root    = <baked in model.json for (project, env, slot)>
registry_dir  = state_root/registry/
run_dir       = state_root/runs/<runId>/            # composed at runtime
artifacts_dir = run_dir/artifacts/
logs_dir      = run_dir/logs/
port_window   = <baked candidate window for (project, env, slot)>
```

### Port reservation (runtime)

Pure derivation is insufficient for ports because the host runs other processes.

> **Invariant PORT-1: No service starts without a reserved, verified port.**

1. Take the baked candidate window for `(project, env, slot)`.
2. Under the slot registry lock, pick the next free port in the window.
3. Verify by binding; on conflict, follow the model's **collision policy** — `fail`, `probe-in-range`, or `request-override` **[adopted from RFC_v2cx]** — and record the collision.
4. Reserve the port in the registry against `runId` before the service starts. Reservations record project, env, slot, run, service, logical port name, concrete port, owner process identity, and lease metadata.
5. Release on normal stop, cleanup, or reconciliation after proving the owner is gone.

## The Registry (reconciled projection)

The registry is a durable record, not a liveness oracle. The OS owns liveness.

- **Storage:** an append-only NDJSON event stream per run, plus a derived per-slot snapshot for fast reads.
- **Process identity:** every record carries enough to survive PID reuse — `pid`, process-group id, start-time (or platform-equivalent), optional `pidfd`/stable handle, command metadata, and parent `runId`. Never `pid` alone.
- **Reconciliation:** `ps` does not trust the log; it reconciles each record against the OS before reporting `running`, `stopped`, `stale`, `canceled`, or `orphaned`.
- **Leases & GC:** every run holds a lease. A crashed runtime leaves its services parented to the process group; `clean`/GC (and an opportunistic sweep on each invocation) detects expired leases, reaps the owned process groups, releases ports, and writes terminal events. This is the direct answer to "started by one script, stopped by another, if at all."

### Concurrency model

Parallel slots are a first-class product feature, not an implementation detail.

> **Invariant REG-1: One writer per slot registry for mutating shared state.** Reservation, lease acquisition, and snapshot updates take an advisory lock (`flock`) on the registry. Per-run event streams are append-only; the snapshot is rebuilt and written via temp-file + atomic rename. Slot roots and run roots are disjoint by construction, so concurrent slots cannot corrupt shared state. Readers tolerate partially completed runs; cleanup is idempotent; stale leases are recoverable. The design must not assume a remote database, daemon, or multi-host coordinator.

## Services and Workflows

Services are long-lived processes; workflows are bounded execution graphs.

**Service lifecycle phases** (also the adapter operation-class enum) **[fork — phase set]** — use `RFC_v2cx`'s universal six; `migrate`/`reset` from `RFC_v2` are adapter-specific extensions, not core classes:

```
Prepare · Start · Ready · Health · Stop · Clean
```

**Workflow node classes** **[adopted from RFC_v2cx]**: task · service requirement · readiness gate · artifact collection · cleanup action.

A workflow run must know which services it requires; whether each is owned, reused, or forbidden; readiness criteria before dependent nodes run; cancellation behavior; cleanup policy; and artifact/summary locations.

> **Invariant PROC-3: No workflow node may depend on an untracked service process.**

## Adapter Strategy

Core stays adapter-free. Core owns only the typed adapter contract:

- service model shape
- lifecycle phases and the **closed enum of operation classes** above
- operation input schema and operation result schema
- registry and ownership expectations
- validation rules
- generated docs/schema from typed metadata (operation classes, env vars, ports, lifecycle support, readiness/health behavior, state roots, examples)

Concrete operation bindings compile to executable closure paths and arguments, but the semantic operation class is always typed. String dispatch such as `svc::<service>::<op>` may exist only as a generated compatibility ABI, never as the primary model.

Concrete adapters live under `adapters/` and are opt-in. A minimal Nixfied project must not evaluate postgres, nginx, minio, reth, helios, or any concrete adapter by default.

Postgres is the canonical reference adapter — it exercises ports, state, lifecycle, readiness, health, workflows, persistence, and cleanup. Others stay smaller until the core contract is stable.

## Public Surfaces

Derived from the execution model, not a command wishlist. Two command layers; **[fork — naming]** adopt `RFC_v2cx`'s friendlier verbs, mapped to single responsibilities (final names may change, but no duplicate families for the same action):

**`nixfied` (ergonomic, may compile):** `compile`, `validate [--deep]`, `model`/`introspect`, `up`, `down`, `run`, `ps`, `logs`, `clean`, `install`, `upgrade`.

**`nixfied-runtime` (pure engine, never evaluates):** `model` (inspect compiled model), `check` (model + host admission assumptions), `run` (workflow/task), `up` (start required services), `down` (stop owned services), `ps` (reconcile + list observed state), `logs`, `clean` (reconcile + reap stale state/leases/processes).

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What compiler pass validates it?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those five, it is not core. This is the contributor rule, not a one-time review.

## Runtime Invariants (the laws the codebase must hold)

- **SEAM-1:** No Nix evaluation after admission; evaluation is confined to `compile` and `validate --deep`. The engine uses/realises pinned closures but never evaluates. Enforced structurally by the `nixfied-runtime` binary.
- **MODEL-1:** The runtime never executes without a validated `model.json`; every admitted run records the exact model path and `modelHash` it used.
- **PORT-1:** No service starts without a reserved, verified port.
- **PLACE-1:** State, logs, artifacts, and summaries are placed under model-derived roots; pure placement is baked by Nix, run-scoped paths composed at runtime, derivation defined once per phase.
- **REG-1:** One writer per slot registry for shared mutations; per-run streams append-only; snapshots written via atomic rename.
- **PROC-1:** Every spawned process belongs to a runtime-owned process group; there are no untracked children.
- **PROC-2:** Cancellation propagates to the whole process group; cleanup is idempotent and safe to re-run after a crash.
- **PROC-3:** Every long-lived process has a registry record before it is considered started; no node depends on an untracked process.
- **LIVE-1:** Liveness is always reconciled against the OS before being reported; the registry is never trusted as a liveness oracle.
- **NIX-1:** Nix cannot own live process supervision, liveness reporting, cancellation, registry mutation, or cleanup; shell cannot own graph, registry, summary, validation, or cleanup semantics.

These exist so the rebuild does not silently regrow the v1 failure mode.

## Milestones

### Milestone 0 — Walking skeleton (thin slice through every layer)

Smallest end-to-end vertical slice; exercises the whole spine and nothing more:

- one typed Nix project module → one `compile` path producing `model.json` (`schemaVersion` + `modelHash`) and `schema.json`
- `nixfied-runtime` loads and validates the model (hash + schema)
- one env (`dev`), one slot (`0`), one task, one service with `Start`/`Ready`/`Stop`
- one registry event stream, one port reservation, one state root, one log root, one summary file
- `ps` reconciles and reports observed state; `model`/`introspect` reads the model

Complete when a downstream-shaped minimal example can: build `model.json`; start a service; wait for readiness; run a dependent task; write registry events and a summary; stop the owned service; record the admitted model path and hash; and report no live owned processes after reconciliation. No adapters, parallel slots, workflows, or install yet.

### Subsequent milestones

1. **Slot isolation** — two concurrent slots of the same env: disjoint ports, state, logs, registry streams; independent reconciliation.
2. **Reference adapter (Postgres)** — start → readiness → client workflow → artifacts → clean stop, with state preserved/deleted per policy.
3. **Workflow graphs** — dependency-aware workflows with bounded tasks, service requirements, readiness gates, cancellation, artifacts, summaries; cancellation propagates and leaves explanatory evidence.
4. **Cancellation & GC hardening** — signal propagation, leases, `kill -9` survival and reconciliation.
5. **Installable downstream wrapper** — adoption/upgrade without vendoring internals or losing project-owned config; `schemaVersion`/`runtime` gating.
6. **Polyglot example** — `examples/polyglot-stack`.

## Proof Strategy (tiered, to avoid a proof monolith)

Layered so no single workspace becomes the only evidence:

- Nix correctness/compiler proofs — model validation, closures, canonical JSON, stable `modelHash`.
- Runtime unit proofs — placement composition, port reservation, registry events, process identity, reconciliation, GC.
- Adapter-contract proofs without concrete services; adapter-specific proofs in isolation.
- Downstream-shaped examples exercising public APIs only, including the `kill -9` reconciliation scenario.

Initial examples (downstream-shaped workspaces with their own `flake.nix`, framework import, `README.md`, operating notes — not framework-owned fixtures):

- `examples/minimal`
- `examples/postgres-api`
- `examples/web-nginx`
- `examples/object-storage`
- `examples/polyglot-stack`

> Risk noted from v1: a single giant proof workspace becomes a second framework snapshot. The tiered split is deliberate; examples prove integration, not internals.

## Definition of Done (per concept)

- **Model:** a project compiles typed intent into canonical `model.json`; the engine rejects invalid, unsupported, or tampered (`modelHash` mismatch) artifacts before execution.
- **Environment:** `dev`, `test`, `ci` express different services, workflows, state and cleanup policy through the same typed model, with disjoint state roots and no cross-env leakage.
- **Slot:** two slots of the same env run concurrently with disjoint placement, ports, state, logs, artifacts, registry streams, and summaries; `kill -9` of one leaves the other intact and reconciling within one `ps`.
- **State:** roots are derived, inspectable, policy-controlled, and safe to clean without guessing which process owns them.
- **Registry:** the runtime reports live/stopped/stale/canceled/orphaned via reconciliation, not by trusting stale files; `clean` reaps orphans and releases their ports.
- **Service:** start → ready → health → stop owned end-to-end, with a reserved port and a process group that fully tears down, and reconciliation after abnormal exit.
- **Workflow:** expresses service requirements, task dependencies, cancellation, artifact collection, and cleanup policy, then produces a durable summary; cancellation mid-run leaves no orphans.
- **Adapter:** implements typed operation classes behind the stable contract without becoming framework core; the compiler rejects an adapter missing a declared class.

## Repository Layout (greenfield)

```
flake.nix                  # apps, devShell, packages, compiled-output builder
nix/
  modules/                 # typed module options (core, env, slot, service, task, workflow, state)
  compiler/                # pure passes: resolve → validate → derive → emit + build closures
  lib/                     # pure helpers; pure placement derivation baked into model.json
runtime/                   # Rust workspace
  Cargo.toml
  crates/
    nixfied-model/         # serde types for model.json + schemas; the shared contract
    nixfied-runtime/       # pure execution engine: admission, supervisor, registry, reconciliation
    nixfied-cli/           # ergonomic `nixfied` front-end (may compile, then delegate)
adapters/                  # opt-in service adapters (postgres first); NOT evaluated by default
examples/                  # downstream-shaped proof workspaces
docs/
```

## Non-Goals

- No compatibility with the current repository layout; no migration layer for v1 commands.
- No broad downstream project template as framework core.
- No required service adapter for a minimal project.
- No daemon requirement for the initial runtime; no remote or multi-host execution.
- No duplicate command families for the same lifecycle action.
- No shell-owned semantic sidecars for graph, registry, summary, validation, or cleanup behavior.
- No live Nix evaluation during admitted execution, supervision, cancellation, registry mutation, or cleanup.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Deferred (explicit non-goals *for now*, to keep them from leaking back in)

Real and intentionally out of v2's first cut; listed so they are not designed in by accident:

- **Secrets/credentials management** — beyond modeled environment passthrough to runtime-owned commands; no sourcing/redaction model yet.
- **Inter-service dependency DAG** — services declare readiness; cross-service ordering beyond workflow phases is deferred.
- **Log aggregation/retention** — logs are runtime-owned files per run; centralized aggregation and modeled-log-artifacts are later.
- **Multi-host / remote execution** — registry and state are local-disk; distributed execution would require revisiting REG-1 and LIVE-1.

## Open Questions

1. Should `model.json` be produced as a Nix derivation output, a CLI output, or both?
2. Dependency closures — built eagerly at `compile` (slower compile, zero runtime build) or pinned as derivations and `nix build`-realised lazily on first use (faster compile, first-use latency)?
3. Registry storage — start as NDJSON event streams, a file-lock protocol over JSON, or SQLite?
4. Minimum cross-platform process-identity strategy for Linux and macOS.
5. `schema.json` representation — JSON Schema, or a bespoke typed constraint format that both Nix emits and Rust enforces without drift?
6. Compile-latency target and watch-mode incrementality for ergonomic agent iteration; where to cache.
7. Final closed set of adapter operation classes beyond the core six.
8. How much service-dependency-DAG behavior belongs in Milestone 0 vs Milestone 3.
9. `nixfied`/`nixfied-runtime` distribution — pinned in the flake only, or also standalone release artifacts?

## Constraints From v1

Captured so the rebuild does not repeat history:

- Runtime semantics drifted into shell helpers and Nix builders, with no single owner. Hence SEAM-1, NIX-1, and the two-binary split.
- The registry was treated as the source of truth for liveness, which drifts from reality. Hence LIVE-1 and reconciliation.
- A single large proof workspace becomes a second framework snapshot. Hence the tiered proof strategy.
- Duplicate command families accreted for the same lifecycle action. Hence the model-derived public surface and the 5-question gate.

## Success Criteria

A downstream project can define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, runtime closures, and state policy in one typed Nix model; evaluate it for static correctness; compile it deterministically into a versioned, hashed `model.json`; then execute that admitted artifact through one Rust runtime with isolated placement, managed lifecycle, process-aware reconciliation, clean cancellation, crash-safe GC, durable registry history, and reproducible summaries. The acceptance checks in [Definition of Done](#definition-of-done-per-concept) are the measurable form of this statement.
