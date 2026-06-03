# RFC v2: Problem-First Nixfied Rebuild

Date: 2026-06-03
Status: Draft (build-ready)
Supersedes: all prior Nixfied implementation. This branch is a from-scratch rebuild; nothing from v1 is preserved except the lessons captured in [Constraints From v1](#constraints-from-v1).

## Problem

Modern projects are not one program. They are small systems: frontend, API, workers, databases, queues, object storage, migrations, test harnesses, infra scripts, and CI jobs, often written in different languages and owned by different tools.

The operational behavior of those systems is usually scattered across `flake.nix`, shell scripts, package-manager scripts, CI YAML, Docker compose files, service wrappers, env files, port conventions, and local tribal knowledge. That is only half the problem.

The deeper problem is that projects have no clean, reproducible way to run multiple environments, or multiple copies of the same environment, while keeping services, workflows, state, logs, ports, artifacts, and cleanup under one process-aware authority.

Without first-class environment, slot, state, and registry concepts:

- `dev`, `test`, `ci`, preview, and prod-like runs collide.
- Two copies of the same environment cannot run predictably.
- Ports and mutable state leak between runs.
- Services are started by one script and stopped by another, if they are stopped at all.
- Workflows cannot reliably know which service instance they own.
- CI failures leave weak evidence about what ran, what was canceled, and what survived.
- Polyglot codebases need too much glue to coordinate simple local and CI execution.

Nixfied v2 exists to solve that problem.

## Product Statement

Nixfied v2 is a **runtime coordinator for project-shaped systems with a deterministic model and owned execution.**

It compiles project intent into a typed, immutable model, then runs that model through one runtime that can start, inspect, record, reconcile, and clean up the full execution graph across environments, slots, services, workflows, and codebases.

Three properties define the product, and they are not the same:

- **Codified & discoverable.** The entire project — every capability, service, task, workflow, environment, and its constraints — is expressed as typed data. A human or agent can discover what the project can do and how to use it by reading the compiled model, without studying the source or assuming conventions.
- **Deterministic model & placement.** Given the same inputs, the compiled model, the derived identities, and the derived placement (ports, state roots, registry paths, artifact paths) are byte-for-byte reproducible.
- **Owned execution.** Runtime behavior (timing, readiness, process scheduling, external services) is *not* deterministic. What the runtime guarantees instead is that every process is owned, tracked, attributable, reconciled against OS reality, and cleanable.

Nixfied is not primarily a task runner, service template collection, CI wrapper, or Nix scaffold. Those may exist as surfaces or adapters, but the product is the model + owned-execution runtime.

## The Load-Bearing Decision: Two Tools, One Seam

Nixfied is split across exactly two substrates, chosen for what each is good at. The split is defined by the **verb**, not by timing. "Nix at runtime" conflates three different operations; we separate them:

| Verb | Example | Owner |
|------|---------|-------|
| **Evaluate** | flake → derivations, type-checks, module assertions | **Nix**, at authoring surfaces (`compile`, `validate --deep`) only |
| **Build / realise** | the postgres binary, an adapter's `start` script | **Nix** builds it; the result is a store path pinned in the compiled output and *used* at runtime |
| **Enforce** | "is this runtime port override valid?" | **Rust**, against rules Nix authored and compiled into a schema |

1. **Nix is the source of truth and the correctness gate.** Typed Nix modules, with no impure access, *evaluate* into a compiled output (see [The Compiled Output](#the-compiled-output-the-seam)). Nix also *builds* the artifacts the runtime needs — dependency binaries and adapter scripts — and pins them into that output as store paths. Nix never starts a process, never touches mutable state, never observes the running system.

2. **Rust is the runtime, and the only runtime.** A single statically-linked binary (working name `nfd`) reads the compiled output and owns everything impure: process supervision, process-group ownership, signal propagation, readiness/health polling, the registry, state placement, cancellation, reconciliation, and cleanup. The runtime freely *uses* Nix's outputs (store paths, scripts, the validation schema) and may *realise* a pinned derivation, but it never *evaluates*.

These two halves communicate through one compiled output and one rule:

> **Invariant SEAM-1 (the run hot path never evaluates Nix).** Nix *evaluation* is confined to the explicit `compile` and `validate --deep` surfaces. `run-task`, `run-workflow`, `runs`, `svc`, reconciliation, and GC never trigger evaluation — they read the compiled output, use pinned store paths, and enforce the compiled schema.

This is the decision that prevents the central failure mode of the previous design — process and lifecycle semantics leaking into shell helpers and Nix builders, with no single owner. Process supervision is a job for a typed, long-lived program. Configuration and correctness are a job for Nix. We do not blur the line, and we do not put a multi-hundred-millisecond evaluation on the hot path of a runtime that must spin slots and reconcile frequently.

### Correctness is preserved by construction, not by runtime evaluation

Confining evaluation to authoring time does **not** weaken Nix's correctness guarantee. The opposite:

> **The runtime can only ever execute a model that is the output of a successful Nix evaluation.** A config that fails a type-check or assertion never produces a compiled output, so it can never run. Nix's correctness gate therefore covers 100% of runs — as a precondition of existence, not as a per-run check.

For an agent or human iterating programmatically, this is the better contract: correctness feedback arrives at `compile` time as a typed error, never as a mid-run crash, and the surface they read to discover capabilities is stable, typed JSON — not a Nix expression they must evaluate and hold in their head.

### Why Rust

- Real process-group control, signals, `pidfd`, async I/O, and timeouts are first-class.
- A single static binary is trivial to ship inside a Nix derivation and to exec from a thin flake app.
- Strong typing mirrors the "typed model" thesis end-to-end: the compiled output deserializes into Rust types, and a malformed model fails loudly at the boundary.
- Crash-safety primitives (atomic file writes, advisory locks, fsync) are available without ceremony.

## Architecture & Data Flow

```
  project intent (typed Nix modules)
            │
            ▼
   ┌─────────────────┐
   │  Nix compiler   │   pure; passes resolve → validate → derive → emit
   │  (nix/compiler) │   + build/realise deps & adapter scripts → store paths
   └────────┬────────┘
            │ emits (cached, content-addressed)
            ▼
   compiled output  ── the ONLY seam (immutable, versioned)
     ├─ model.json        typed model (schemaVersion)
     ├─ schema.json       validation rules the runtime enforces
     └─ store paths       pinned dep binaries + adapter scripts
            │
            ▼
   ┌─────────────────────────────────────────────────────────┐
   │  nfd  (Rust runtime, the single authority)               │
   │                                                          │
   │   loader+enforce ──► identity & placement ──► orchestrator│
   │                                   │             │        │
   │                                   ▼             ▼        │
   │                              registry ◄──► supervisor    │
   │                             (projection)   (processes,   │
   │                                   │         groups,      │
   │                                   ▼         signals)     │
   │                          state root on disk              │
   └─────────────────────────────────────────────────────────┘
            │                              ▲
            ▼                              │ uses store paths;
   thin flake apps                         │ may `nix build <drv>`
   (`nix run .#<surface>` → exec nfd …)    │ but never evaluates
```

The flake apps are thin: they resolve the cached compiled output and `exec` the `nfd` binary. They contain no semantics. The runtime *uses* and *realises* the pinned store paths but never *evaluates* (SEAM-1).

## Repository Layout (greenfield)

```
flake.nix                  # apps, devShell, the nfd package, compiled-output builder
nix/
  modules/                 # typed module options (core, env, slot, service, task, workflow, state)
  compiler/                # pure passes: resolve → validate → derive → emit model.json
  lib/                     # pure helpers (identity derivation lives here AND in Rust; see PARITY-1)
runtime/                   # Rust workspace
  Cargo.toml
  crates/
    nfd-model/             # serde types for model.json + schemaVersion; the contract
    nfd-runtime/           # supervisor, registry, state, reconciliation
    nfd-cli/               # argument surfaces; thin over nfd-runtime
adapters/                  # opt-in service adapters (postgres first); NOT evaluated by default
examples/                  # downstream-shaped proof workspaces
docs/
```

## Core Concepts

- **Model:** the compiled, immutable contract for what exists and how it can run. Serialized as `model.json`, carries `schemaVersion`.
- **Environment:** the named mode of operation, such as `dev`, `test`, `ci`, preview, or prod-like.
- **Slot:** a deterministic parallel instance of an environment, identified by an integer index. Two slots of the same env must run concurrently without collision.
- **State:** all mutable roots derived from `(project, env, slot, run)` identity. Owned and cleaned by the runtime.
- **Registry:** a durable, **reconciled projection** of intent, history, and ownership — *not* the source of truth for liveness. The OS owns liveness; the registry records what the runtime intended and reconciles it against reality.
- **Service:** a long-lived process with lifecycle, readiness, health, logs, state, and ownership.
- **Workflow:** a dependency graph of tasks and service phases with artifacts, summaries, cancellation, and cleanup.
- **Adapter:** language-, service-, or tool-specific implementation behind a stable, typed model/runtime contract.

## Identity & Placement

Identity and placement are derived deterministically; *binding* a port is reserved and verified at runtime. Pure derivation alone is unsafe because the host runs other processes.

### Identity

```
run_id   = ULID (sortable, unique, generated by the runtime per run)
slot      = integer index, 0..N, supplied explicitly (default 0)
scope     = "<project>/<env>/<slot>"
```

`project` and `env` are model-defined. `slot` is an invocation input. `run_id` is minted by the runtime.

### Placement (deterministic derivation)

```
state_root     = $NIXFIED_STATE_DIR/<project>/<env>/<slot>/
registry_dir   = state_root/registry/
run_dir        = state_root/runs/<run_id>/
artifacts_dir  = run_dir/artifacts/
logs_dir       = run_dir/logs/
port_window    = port_base + (stable_hash(project, env) % window_count) * window_size
                              + slot * slot_stride
```

`port_base`, `window_size`, `slot_stride`, and `window_count` are model-level state-policy fields. The derivation yields a *candidate window*, not a guaranteed-free port.

### Port reservation (runtime, not derivation)

> **Invariant PORT-1: No service is started without a reserved, verified port.**

1. Compute the candidate window for `(project, env, slot)`.
2. Under the slot registry lock, pick the next free port in the window.
3. Verify by binding; if taken, advance and record the collision.
4. Reserve the port in the registry against `run_id` before the service starts.
5. Release on service stop or run GC.

> **Invariant PARITY-1: Placement derivation has one definition.** The Rust runtime is authoritative. The Nix `lib/` may reproduce derivation for introspection, but a parity test asserts the two agree. Disagreement is a build failure.

## The Registry (reconciled projection)

The registry is a durable record, not a liveness oracle.

- **Storage:** an append-only NDJSON event log per slot (`registry_dir/events.ndjson`), plus a derived snapshot for fast reads.
- **Process identity:** every tracked process is recorded as `(pid, start_time)` (or `pidfd` where available), never `pid` alone — PIDs are reused, and a stale `pid` will otherwise report a stranger's process as alive.
- **Reconciliation:** `nfd runs` does not trust the log; it reconciles each recorded process against the OS (`pid` + `start_time` match) before reporting `running`, `exited`, or `orphaned`.
- **Leases & GC:** every run holds a lease. A crashed runtime leaves its services parented to the process group; `nfd gc` (and an opportunistic sweep on each invocation) detects expired leases, reaps the owned process groups, releases ports, and writes terminal events. This is the direct answer to "started by one script, stopped by another, if at all."

### Concurrency model

> **Invariant REG-1: One writer per slot registry at a time for mutating operations.** Reservation, lease acquisition, and snapshot updates take an advisory lock (`flock`) on `registry_dir/lock`. Event appends are atomic (single write, `O_APPEND`); the snapshot is rebuilt from the log and written via temp-file + atomic rename. Parallel slots never share a registry, so they never contend.

## The Compiled Output (the seam)

The seam is not a single file but one immutable, content-addressed directory — the entire contract between Nix and Rust. It contains:

- **`model.json`** — the versioned, serde-typed model owned by `nfd-model`.
  - Top-level `schemaVersion` (integer). The runtime refuses a model whose major version it does not understand.
  - Sections: `project`, `environments`, `slotPolicy`, `statePolicy`, `services`, `tasks`, `workflows`, `adapters`, `surfaces`.
  - Every service declares its adapter and its supported **operation classes** (see Adapter Strategy). Operation classes are a closed enum, validated at compile time — not free-form strings.
- **`schema.json`** — the validation rules for runtime inputs (slot index ranges, allowed env names, port-override bounds, …). These rules are *authored in Nix* and *enforced in Rust*; the runtime does not re-derive them.
- **Pinned store paths** — the realised dependency binaries and adapter scripts the runtime executes, referenced from `model.json` by store path. They are evaluated and (optionally) built at compile time; the runtime uses them and may lazily `nix build <drv>` a pinned derivation, but never evaluates.

Adding `schemaVersion` now is free; retrofitting versioning onto a shipped artifact is not.

## The Iteration Loop (the deterministic correctness gate)

Evolving a project is one loop, and that loop *is* the correctness gate the framework promises:

```
edit typed Nix  →  nfd compile  (Nix evaluation: type-checks + assertions)  →  new compiled output  →  nfd run
                          │
                          └─ on error: typed compile-time failure, nothing runs
```

This loop must be **fast and first-class**, not hidden, because it is how humans and agents iterate safely:

- `nfd compile` is the only thing that triggers evaluation on the common path; the dev shell offers a watch mode that recompiles on change.
- Compile failures are typed and actionable, surfaced before any process starts.
- `nfd validate --deep` is an explicit, opt-in surface that *is allowed* to invoke Nix to re-check the live config against the live filesystem (e.g. referenced source dirs exist). It is never part of `run`/`runs`/reconcile, so it does not violate SEAM-1.
- Shallow `nfd validate` (no Nix) checks the already-compiled output and the live filesystem using the compiled `schema.json` — the fast, hot-path-safe check.

## Required Capabilities

1. **Deterministic env and slot execution.** Identity and placement derive from explicit `(project, env, slot, run)`; ports are reserved and verified at runtime. Multiple slots of the same env run without collision.
2. **Reconciled process registry.** The runtime records services, workflows, tasks, child processes, lifecycle state, readiness, health, cancellation, artifacts, summaries, leases, and stop ownership in one inspectable registry, and reconciles records against OS liveness on read.
3. **Service and workflow lifecycle ownership.** A run knows which services it needs, which instances it owns or reuses, when they are ready, how they are checked, and how they are stopped or left running by policy.
4. **Polyglot project coordination.** Coordinate many small codebases and toolchains without forcing one language, package manager, service runner, or repo shape.
5. **Typed model and pure compiler.** Project intent is typed Nix, evaluated into the canonical immutable compiled output (model + schema + pinned store paths), validated before emit, and exposed through stable introspection and schemas. The compile step is the correctness gate; nothing that fails evaluation can run.
6. **One runtime authority.** Execution, process ownership, run records, summaries, artifacts, cancellation, cleanup, env sandboxing, and reconciliation belong to the `nfd` binary. No shell or Nix builder owns runtime semantics.
7. **Optional service adapters.** Core defines the typed lifecycle contract and dispatcher. Concrete adapters (postgres, nginx, minio, …) are opt-in modules, not framework identity.
8. **Installable downstream wrapper.** A project adopts or upgrades the framework without vendoring it into project-owned files or losing project-owned configuration. `schemaVersion` gates compatibility.
9. **Proof through real execution.** Proof is tiered (see Testing Strategy), anchored by downstream-shaped example workspaces that exercise the public surfaces.

## Runtime Invariants (the laws the codebase must hold)

- **SEAM-1:** The run hot path never triggers Nix evaluation; evaluation is confined to the explicit `compile` and `validate --deep` surfaces. The runtime *uses* and *realises* pinned store paths but never *evaluates*.
- **PORT-1:** No service starts without a reserved, verified port.
- **PARITY-1:** Placement derivation has one authoritative definition (Rust), parity-checked against Nix.
- **REG-1:** One writer per slot registry for mutations; appends atomic; snapshots written via atomic rename.
- **PROC-1:** Every spawned process belongs to a runtime-owned process group. There are no untracked children.
- **PROC-2:** Cancellation propagates to the whole process group; cleanup is idempotent and safe to re-run after a crash.
- **LIVE-1:** Liveness is always reconciled against the OS before being reported; the registry is never trusted as a liveness oracle.

These exist so the rebuild does not silently regrow the v1 failure mode.

## Adapter Strategy

Concrete service declarations do not live in framework configuration. The framework core stays minimal and adapter-free.

Core owns only the typed adapter contract:

- service model shape
- lifecycle phases and a **closed enum of operation classes** (`start`, `stop`, `ready`, `health`, `migrate`, `reset`, …) validated by the compiler
- the `svc <service> <op>` dispatch surface, backed by typed operations rather than string parsing
- registry and ownership expectations
- adapter validation rules
- generated docs/schema for adapter contracts from typed metadata

Concrete adapters live under `adapters/` and are opt-in only. A minimal Nixfied project must not evaluate postgres, nginx, minio, or any concrete adapter by default.

Best-practice usage lives under `examples/`, each a downstream-shaped workspace with its own `flake.nix`, `README.md`, and `AGENTS.md`.

Initial examples:

- `examples/minimal`
- `examples/postgres-api`
- `examples/web-nginx`
- `examples/object-storage`
- `examples/polyglot-stack`

Postgres is the canonical reference adapter because it exercises ports, state, lifecycle, readiness, health, workflows, and persistence. Other examples stay smaller and adapter-specific.

## Public Surfaces

The public API is derived from the execution model, not from a command wishlist. V2 exposes only surfaces needed to:

- compile project intent into the compiled output (`compile`) — the only common-path evaluation surface
- discover the compiled model (`introspect`, `schema`, `docs`)
- validate env/slot/state assumptions (`validate`, and opt-in `validate --deep` which may re-evaluate Nix)
- run tasks and workflows (`run-task`, `run-workflow`)
- start/check/stop service lifecycle operations (`svc <service> <op>`)
- inspect registry state (`runs`)
- stop owned processes (`stop-run`, `stop-all-runs`)
- reap crashed runs (`gc`)
- install or upgrade a downstream wrapper (`install`, `upgrade`)

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What compiler pass validates it?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those five, it is not core. This gate is the contributor rule, not a one-time review.

## Build Order

### Milestone 0 — Walking skeleton (thin slice through every layer)

The first runnable thing exercises the whole spine and nothing more:

- one project, one env (`dev`), one slot (`0`)
- `nfd compile` evaluates the typed Nix and emits a minimal compiled output (`model.json` with `schemaVersion` + `schema.json`)
- `nfd` loads it, enforces the schema, derives identity & placement, creates the state root
- one task runs to completion and records a run in the registry
- one trivial service starts → reports ready → is stopped, with port reserved and released
- `nfd runs` reconciles and reports liveness
- `nfd introspect` reads the model

No adapters, no parallel slots, no workflows, no install. Everything else is layered on this spine.

### Subsequent milestones

1. **Workflows** — task/service-phase DAG, artifacts, summaries.
2. **Parallel slots** — concurrent slots of the same env, port windows, registry isolation, reconciliation under contention.
3. **Cancellation & GC** — signal propagation, leases, crash recovery, `kill -9` survival.
4. **Adapter contract + Postgres** — typed operation classes, the first real adapter, `examples/postgres-api`.
5. **Install/upgrade wrapper** — downstream adoption without vendoring; `schemaVersion` gating.
6. **Polyglot example** — `examples/polyglot-stack`.

## Testing Strategy (tiered, to avoid a proof monolith)

Proof is layered so no single workspace becomes the only evidence:

- **Compiler/eval unit proofs** — pure Nix: passes validate, reject bad input, derive placement; `model.json` matches schema.
- **Runtime unit proofs** — Rust: identity/placement parity, port reservation, registry append/snapshot/reconciliation, process-group lifecycle, GC.
- **Example end-to-end proofs** — downstream-shaped workspaces exercising public surfaces, including the `kill -9` reconciliation scenario.

> Risk noted from v1: a single giant proof workspace tends to become a second framework snapshot. The tiered split is deliberate; the examples prove integration, not internals.

## Definition of Done (per concept)

Each concept ships only when its acceptance check passes:

- **Model:** a malformed model is rejected at the `nfd-model` boundary with a typed error; `schemaVersion` mismatch is refused.
- **Environment:** `dev`, `test`, `ci` run with disjoint state roots and no cross-env leakage.
- **Slot:** two slots of the same env run concurrently, bind disjoint ports, write disjoint state, and `kill -9` of one leaves the other intact and reconciling within one `nfd runs` call.
- **State:** all mutable roots resolve under the derived state root; `gc` removes a finished run's state and leaves owned-by-policy state untouched.
- **Registry:** after a runtime crash, `nfd runs` reports the orphaned services as such (not "running"), and `nfd gc` reaps them and releases their ports.
- **Service:** start → ready → health → stop is owned end-to-end, with a reserved port and a process group that fully tears down on stop.
- **Workflow:** a DAG with a service phase produces deterministic artifacts and a reproducible summary; cancellation mid-run leaves no orphans.
- **Adapter:** Postgres implements its declared operation classes; the compiler rejects an adapter missing a declared class.

## Non-Goals

- No broad downstream project template as framework core.
- No required service adapter for a minimal project.
- No duplicate command families for the same lifecycle action.
- No runtime semantics in shell or Nix builders (SEAM-1, "one runtime authority").
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Deferred (explicit non-goals *for now*, to keep them from leaking back in)

These are real and intentionally out of v2's first cut. Listing them keeps them from being designed in by accident:

- **Secrets/credentials management** — beyond passing through env; no sourcing/redaction model yet.
- **Inter-service dependency DAG** — services declare readiness; cross-service ordering beyond workflow phases is deferred.
- **Log aggregation/retention** — logs are placed per run; centralized aggregation is later.
- **Multi-host / remote execution** — the registry and state model are local-disk; distributed execution is out of scope and would require revisiting REG-1 and LIVE-1.

## Open Questions

1. Port window sizing and stride defaults — what range avoids realistic host collisions without a huge reservation space?
2. Lease TTL and the cadence of the opportunistic GC sweep.
3. Snapshot cadence vs. pure log-replay — when does the NDJSON log get compacted?
4. Adapter operation-class enum — final closed set for v2.
5. `nfd` distribution — pinned in the flake only, or also a standalone release artifact?
6. Dependency artifacts — built eagerly at `compile` (slower compile, zero runtime build) or pinned as derivations and `nix build`-realised lazily on first use (faster compile, first-use latency)?
7. Compile latency target and watch-mode incrementality — what eval time keeps the iteration loop ergonomic for agents, and where do we cache?
8. `schema.json` representation — JSON Schema, or a bespoke typed constraint format that both Nix emits and Rust enforces without drift?

## Constraints From v1

Captured so the rebuild does not repeat history:

- Runtime semantics drifted into shell helpers and Nix builders, with no single owner. Hence SEAM-1 and a Rust-only runtime.
- The registry was treated as the source of truth for liveness, which drifts from reality. Hence LIVE-1 and reconciliation.
- A single large proof workspace tends to become a second framework snapshot. Hence the tiered testing strategy.
- Duplicate command families accreted for the same lifecycle action. Hence the model-derived public surface and the 5-question gate.

## Success Criteria

A downstream project can define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, and state policy in one typed model; compile it to an immutable, versioned `model.json`; then run and inspect any environment or slot through the single `nfd` runtime — with deterministic placement, isolated state, owned and reconciled lifecycle, clean cancellation, crash-safe GC, durable registry history, and reproducible summaries. The acceptance checks in [Definition of Done](#definition-of-done-per-concept) are the measurable form of this statement.

