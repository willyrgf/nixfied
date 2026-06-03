# RFC v2 (v3 merge): Greenfield Nixfied Build Spec

Date: 2026-06-03
Status: Draft (build-ready)
Merges: `RFC_v2.md` and `RFC_v2cx.md` into one spec. Fork resolutions and their provenance are recorded in the [Decision Log](#decision-log-resolved-forks); the body states the resolved decision directly.
Supersedes: all prior Nixfied implementation. This branch is a from-scratch rebuild; nothing from v1 is preserved except the lessons captured in [Constraints From v1](#constraints-from-v1).

## Decision Summary

Nixfied v2 is a greenfield rebuild. The existing framework, proof workspaces, shell sidecars, historical command structure, and repository layout are not compatibility constraints.

The load-bearing architecture decision:

- **Nix is the authority for what the project is allowed to be** — codification, static correctness, and reproducible runtime inputs (closures).
- **Rust is the authority for what the project is currently doing** — admitted execution, observed runtime state, process ownership, and cleanup.
- Nix compile/check phases produce immutable, versioned, content-hashed artifacts consumed by the Rust execution engine. The primary artifact is `model.json`.
- The split is enforced *structurally* by two binaries: `nixfied` (ergonomic, may compile) and `nixfied-runtime` (pure engine, cannot evaluate Nix).
- The runtime realises pinned closures only at admission, before any process starts; it never evaluates Nix and never builds a flake reference on the hot path.
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
- **Deterministic model & logical placement.** Given the same typed input, the compiled model, its hash, the derived identities, and the *logical* placement (template roots, relative layouts, registry keys, candidate port windows) are byte-for-byte reproducible across machines. Host-absolute paths are **not** part of this guarantee: they are materialised at admission from a host-resolved base (see [Identity & Placement](#identity--placement)). What is reproducible is the logical placement; what is host-specific is resolved at runtime.
- **Owned execution.** Runtime behavior (timing, readiness, scheduling, external services) is *not* deterministic. The runtime instead guarantees that every process is owned, tracked, attributable, reconciled against OS reality, and cleanable.

Nix is the authority for what the project is allowed to be. Rust is the authority for what the project is currently doing.

Nixfied is not primarily a task runner, service template collection, CI wrapper, Docker replacement, or Nix scaffold. Those may exist as surfaces or adapters, but the product is the model/runtime contract.

## The Load-Bearing Decision: Two Tools, One Seam

The split is defined by the **verb**, not by timing. "Nix at runtime" conflates three different operations; we separate them:

| Verb | Example | Owner |
|------|---------|-------|
| **Evaluate** | flake → derivations, type-checks, module assertions | **Nix**, at authoring surfaces (`compile`, `validate --deep`) only |
| **Build / realise** | the postgres binary, an adapter's `start` closure | **Nix** builds it; the result is a store path pinned in the model and *realised by the runtime at admission* |
| **Enforce** | "is this runtime port override valid?" | **Rust**, against rules Nix authored and compiled into a schema |

1. **Nix is the source of truth and the correctness gate.** Typed Nix modules, with no impure access, *evaluate* into the compiled output. Nix also *builds* the closures the runtime needs — dependency binaries, adapter operations, helper scripts — and pins them into the model as store paths. Nix never starts a process, never touches mutable state, never observes the running system.

2. **Rust is the runtime, and the only runtime.** The execution engine reads the compiled output and owns everything impure: admission checks, process supervision, process-group ownership, signal propagation, readiness/health polling, the registry, state placement, cancellation, reconciliation, and cleanup. It *uses* Nix's outputs and *realises* pinned store paths at admission, but never *evaluates* and never builds a flake reference.

These two halves communicate through one compiled output and two rules:

> **Invariant SEAM-1 (no evaluation after admission).** Nix *evaluation* is confined to the explicit `compile` and `validate --deep` surfaces. Once [execution admission](#correctness-layers) begins, nothing — `run`, `up`, `down`, `ps`, `svc`, reconciliation, GC — triggers evaluation. The engine reads the compiled output, realises/uses pinned store paths, and enforces the compiled schema. The `nixfied-runtime` binary makes this structural: it cannot import a Nix expression, shell to `nix eval`, or `nix build` a flake reference.

> **Invariant BUILD-1 (realisation only at admission).** Realisation of pinned closures happens only during admission, before any process starts. It is forbidden during process supervision, readiness/health polling, reconciliation, cancellation, GC, and cleanup. The engine realises **only concrete store paths or pinned `.drv` references** (e.g. `nix-store --realise /nix/store/….drv`); it never runs `nix build <flake-ref>` or any command that can evaluate. This is what makes SEAM-1 structural rather than aspirational — realisation cannot smuggle evaluation in, and no run can stall mid-flight on a store build or network substitution.

The two-binary split below is what enforces both invariants by construction rather than by review discipline.

- **`nixfied`** — the ergonomic, integrated CLI. May run an explicit Nix `compile`/`check` phase, then delegate to the engine. (`nfd` is an accepted short alias.)
- **`nixfied-runtime`** — the pure execution engine. Consumes compiled artifacts by path, realises pinned store paths at admission, and **cannot** evaluate Nix. SEAM-1 and BUILD-1 live here as properties of the binary, not promises.

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
- **Run:** one runtime invocation with a unique run identity (`runId`).
- **Service instance:** a reusable running service identified by `serviceInstanceId`, a stable function of `(projectId, env, slot, serviceName, logicalPortSet)` — **distinct from `runId`**. Reservations, leases, logs, and ownership attach to the instance; a run *holds* or *reuses* an instance via a lease. Reference-counting and lease handoff for reuse are specified at the [Workflow milestone](#milestones); the identity is defined now so reuse cannot blur ownership, logging, cleanup, or cancellation later.
- **Placement:** logical (template) roots, relative layouts, registry keys, and candidate port windows derived per `(project, env, slot)`, plus the host-materialised absolute paths and run-scoped paths the runtime composes from them.
- **State:** all mutable roots derived from project, environment, slot, service, workflow, and run identity. Owned and cleaned by the runtime.
- **Registry:** a durable, reconciled projection of runtime state and history — *not* the source of truth for liveness. The OS owns liveness.
- **Service:** a long-lived process with lifecycle, readiness, health, logs, state, and ownership.
- **Workflow:** a dependency graph of tasks and service phases with artifacts, summaries, cancellation, and cleanup.
- **Adapter:** language-, service-, or tool-specific implementation behind a stable, typed model/runtime contract.
- **Capability catalog:** a generated, agent-readable index of everything the project exposes (environments, slots, services, tasks, workflows, adapters, surfaces, state policy), emitted as part of the compiled output.

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
      ├─ capabilities.json generated, agent-readable capability catalog
      ├─ docs/             generated docs from typed metadata
      └─ closures          pinned dep binaries, adapter ops, helper scripts (store paths)
      │
      ▼
nixfied-runtime  (Rust engine, the single execution authority)
      ├─ model loader + schema enforce
      ├─ admission (host checks, locks, reservations, closure realisation)
      ├─ orchestrator / workflow runner
      ├─ service dispatcher
      ├─ readiness + health probes
      ├─ process-group owner + signal propagation
      └─ registry reconciler
      │                                   ▲ realises concrete store paths at admission
      ▼                                   │ (never `nix build` a ref; never evaluates) SEAM-1/BUILD-1
registry, state roots, logs, artifacts, summaries
```

The flake apps and the ergonomic `nixfied` CLI are thin: they resolve the cached compiled output and `exec` the engine. They contain no execution semantics.

### Boundary Rules

- Nix validates typed project intent and emits `model.json`, `schema.json`, the capability catalog, generated docs, and reproducible store paths for runtime commands and adapter operations.
- The engine reads compiled artifacts and performs all impure execution.
- The engine never evaluates Nix, imports Nix modules, shells to `nix eval`, `nix build`s a flake reference, or mutates the model artifact after admission begins. The most it does with Nix is `nix-store --realise` a concrete pinned `.drv`/store path, and only during admission (BUILD-1).
- The ergonomic CLI may run an explicit compile/check phase before execution, but the resulting model **path and hash** must be recorded as the input to the admitted run.
- Adapter code may use shell internally, but shell output is never a semantic sidecar. Semantic state returns through typed operation results or runtime-owned observation.

## Correctness Layers

Nixfied v2 separates desired-state correctness from observed-runtime correctness across four layers.

1. **Model correctness (Nix).** Reject invalid names, broken references, incompatible adapter declarations, invalid workflow edges, missing fields, and malformed state/placement policy before runtime is possible.
2. **Closure correctness (Nix).** Reproducibly construct the commands the runtime may execute — runtime/task/adapter closures, helpers, generated scripts, schemas, docs. The engine receives executable paths from the model instead of discovering tooling heuristically.
3. **Admission correctness (Rust).** Host-specific checks Nix cannot prove: closure realisation on this host, base-path resolution and writable state roots, port availability, registry lock acquisition, stale-lease reconciliation, ownership conflicts, env-specific preconditions. **Admission is the named moment after a model is selected and before any long-lived service starts** — the moment SEAM-1 takes effect and the only moment BUILD-1 permits realisation.
4. **Execution correctness (Rust).** The impure graph: process groups, signals, readiness/health, task execution, workflow cancellation, registry events, summaries, cleanup, reconciliation.

This keeps Nix central to project evolution and agent-readable codification while preventing Nix evaluation from becoming a live process supervisor.

## The Compiled Output (the seam)

One immutable, content-addressed directory — the entire contract between Nix and Rust.

Required components: `model.json`, `schema.json`, `capabilities.json`, the pinned closures, and `docs/` (generated). All are produced by Nix; none are mutated by the runtime.

### `model.json`

Versioned, serde-typed, owned by the shared `nixfied-model` crate. Required top-level fields:

- `schemaVersion` — integer, initially `1`. The engine refuses an unknown major version.
- `modelHash` — hash of the canonical model excluding `modelHash` (see [Canonicalization & hashing](#canonicalization--hashing)). The engine validates it before execution; every admitted run records the path and hash it used.
- `runtime` — minimum compatible runtime version and feature requirements. Gives bidirectional version negotiation (model ⇄ engine), not just one-directional.
- `project` — stable project metadata and `projectId`.
- `environments` — named environment definitions.
- `slotPolicy` — slot index range, defaults, and placement-relevant slot policy.
- `services` — declared service models.
- `tasks` — declared bounded task models (a task is first-class, not only a workflow node).
- `workflows` — declared workflow graphs.
- `adapters` — adapter metadata and operation bindings.
- `surfaces` — the public command surfaces derived from the model, so an agent can discover the callable interface from the artifact rather than from a hardcoded CLI.
- `placement` — placement policy plus the *logical* run-independent roots, layouts, and candidate port windows (see [Identity & Placement](#identity--placement)). No host-absolute paths.
- `state` — state policy and root declarations (persistence, cleanup, retention).
- `closures` — store paths for runtime-dispatched tasks, adapters, and helper operations.

### `schema.json`

The validation rules for **runtime inputs** (slot index ranges, allowed env names, port-override bounds, collision policy, …). These rules are *authored in Nix* and *enforced in Rust*; the engine does not re-derive them.

### `capabilities.json` and `docs/`

The generated, agent-readable catalog of project capabilities and the human-readable docs derived from the same typed metadata. Discoverability is a product property (see [Definition of Done](#definition-of-done-per-concept)), so these are part of the contract, not optional extras.

### Pinned closures

The dependency binaries, adapter operations, and helper scripts the engine executes, referenced by store path. Evaluated and (optionally) built at compile time. At admission the engine **realises** the concrete store paths/`.drv`s it will need (BUILD-1); it never evaluates and never builds a flake reference.

### Canonicalization & hashing

`modelHash` is the cross-language contract between Nix (producer) and Rust (validator), so the canonical form is fixed and both sides obey it:

- UTF-8, Unicode **NFC** normalization.
- Object keys sorted lexicographically by Unicode code point.
- Arrays in declared (semantic) order — order is meaningful and preserved.
- Numbers in a single canonical form; no insignificant whitespace.
- `null`/default fields omitted explicitly rather than emitted.
- The hash covers semantic model content **including referenced closure store paths and `schemaVersion`**, excluding only the `modelHash` field itself.

> **Nix emits the canonical bytes; the engine hashes the raw on-disk bytes and never re-serializes to validate.** Re-serialization in Rust would create a second canonicalizer that can disagree with Nix's, turning the seam into a parity problem. Hashing the exact bytes Nix wrote makes Nix/Rust disagreement impossible by construction. The hash algorithm (e.g. SHA-256) is fixed in `schemaVersion`.

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
- `nixfied up` compiles by default when the model is stale, then delegates. It always reports which compiled-output path and `modelHash` it admitted and whether it rebuilt; `--no-compile` admits the existing model without recompiling.

## Identity & Placement

Every runtime action is scoped by explicit identity:

```text
projectId / environment / slot / runId
```

- `projectId` — stable, explicit identifier from the model.
- `environment` — model-defined environment name.
- `slot` — user-selected deterministic parallel-instance index (`0..N`, default `0`).
- `runId` — runtime-created unique ID per invocation. Use a **ULID**: lexicographically sortable by creation time, encodes the timestamp, and is contention-safe.

### Placement derivation: logical (baked) vs materialised (admission)

Placement is split by phase so derivation logic exists in exactly one place per phase and no host-specific path is ever baked into a content-hashed artifact:

- **Logical placement is derived by Nix and baked into `model.json`** (`placement` field): template roots, relative layouts, registry keys per `(project, env, slot)`, and candidate port windows. This is machine-independent and byte-for-byte reproducible — it is what the determinism guarantee covers, and it is discoverable directly from the artifact.
- **Host-absolute placement is materialised by the engine at admission.** The engine resolves a host base — `$NIXFIED_STATE_DIR`, else a documented platform default — and joins the logical template onto it. Absolute paths depend on the host and therefore are not part of `modelHash`.
- **Run-scoped paths are composed at runtime** by joining `runId` onto the materialised roots, since `runId` does not exist at compile time.
- **Port binding is runtime** (see below).

```text
# logical (baked in model.json, hashed, machine-independent):
state_root_tmpl = <project>/<env>/<slot>/
port_window     = <candidate window for (project, env, slot)>

# materialised at admission (host-specific, NOT hashed):
state_base      = $NIXFIED_STATE_DIR | <platform default>
state_root      = <state_base>/<state_root_tmpl>
registry_dir    = state_root/registry/
run_dir         = state_root/runs/<runId>/          # composed at runtime
artifacts_dir   = run_dir/artifacts/
logs_dir        = run_dir/logs/
```

### Port reservation (runtime)

Pure derivation is insufficient for ports, and so is a registry reservation: the registry coordinates Nixfied processes only — it is **not** an OS reservation, so a port that is free at preflight can be taken by an unrelated host process before the service binds. Reservation therefore reflects *intent*; ownership is proven only after the owned process binds.

> **Invariant PORT-1: No service is considered *ready* without verified ownership of its port.**

1. Take the baked candidate window for `(project, env, slot)`.
2. Under the slot registry lock, record a reservation *intent* against `serviceInstanceId`/`runId` and pick the next candidate port in the window.
3. Start the owned service with the selected port.
4. **Readiness verifies that the expected owned process actually bound the expected port** (probe + owner check), not merely that the port is open. Only then is the service `ready`.
5. On bind conflict, record a collision event and retry per the model's **collision policy** — `fail`, `probe-in-range`, or `request-override`.
6. For adapters that support socket activation, the runtime may hold the listening socket and pass the file descriptor to the service. This is the only mechanism by which "reserved" means OS-reserved; it closes the check-to-bind gap entirely.
7. Release on normal stop, cleanup, or reconciliation after proving the owner is gone.

## The Registry (reconciled projection)

The registry is a durable record, not a liveness oracle. The OS owns liveness.

- **Storage:** an append-only NDJSON event stream per run, plus a derived per-slot snapshot for fast reads.
- **Durability & integrity:** event appends are single `O_APPEND` writes and fsynced; each event carries a monotonic event ID and the `schemaVersion` of its shape. The snapshot is rebuilt from the streams and written via temp-file + fsync + atomic rename. Readers tolerate truncated/partial trailing records; a corrupted snapshot is rebuilt from the streams. Compaction of old streams into the snapshot is policy-driven (see [Open Questions](#open-questions)).
- **Ownership keys:** reservations and leases attach to `serviceInstanceId`, not `runId` alone, so a reused service has one unambiguous owner record across runs.
- **Process identity:** every record carries enough to survive PID reuse — `pid`, process-group id, start-time (or platform-equivalent), optional `pidfd`/stable handle, command metadata, and the parent `runId`. Never `pid` alone.
- **Reconciliation:** `ps` does not trust the log; it reconciles each record against the OS before reporting `running`, `stopped`, `stale`, `canceled`, or `orphaned`.
- **Leases & GC:** every run holds a lease. A crashed runtime leaves its services parented to the process group; `clean`/GC (and an opportunistic sweep on each invocation) detects expired leases, reaps the owned process groups, releases ports, and writes terminal events. This is the direct answer to "started by one script, stopped by another, if at all."

### Concurrency model

Parallel slots are a first-class product feature, not an implementation detail.

> **Invariant REG-1: One writer per slot registry for mutating shared state.** Reservation, lease acquisition, and snapshot updates take an advisory lock (`flock`) on the registry. Per-run event streams are append-only; the snapshot is rebuilt and written via temp-file + atomic rename. Slot roots and run roots are disjoint by construction, so concurrent slots cannot corrupt shared state. Readers tolerate partially completed runs; cleanup is idempotent; stale leases are recoverable. The design must not assume a remote database, daemon, or multi-host coordinator. (Advisory-lock semantics over network filesystems are not assumed portable; supported filesystems are documented.)

## Services and Workflows

Services are long-lived processes; workflows are bounded execution graphs.

**Service lifecycle phases** (also the adapter operation-class enum) — the universal six; `migrate`/`reset` are adapter-specific extensions, not core classes:

```
Prepare · Start · Ready · Health · Stop · Clean
```

**Workflow node classes**: task · service requirement · readiness gate · artifact collection · cleanup action.

A workflow run must know which services it requires; whether each is owned, reused, or forbidden; readiness criteria before dependent nodes run; cancellation behavior; cleanup policy; and artifact/summary locations.

**Service reuse and instance identity.** When a workflow reuses a service rather than owning it, the service is addressed by `serviceInstanceId` (not the reusing `runId`). A reused instance carries an owning lease and a reference count: a run that reuses an instance increments the count and does not stop it on exit; the instance is torn down by its owner or by GC when the last lease is released. This keeps logging, port ownership, cancellation, and cleanup attributable to one instance. The full ref-count/lease-handoff state machine is specified at the Workflow-graphs milestone; the identity and the ownership rule are fixed now.

> **Invariant PROC-3: No workflow node may depend on an untracked service process,** and a long-lived process is "started" only once it has a registry record.

## Adapter Strategy

Core stays adapter-free. Core owns only the typed adapter contract:

- service model shape
- lifecycle phases and the **closed enum of operation classes** above
- operation input schema and operation result schema
- registry and ownership expectations
- **foreground-execution requirement** — a service must run in the foreground under the runtime's process group. An adapter may daemonize or fork a long-lived child **only** if it returns a stable child identity (pid + start-time, or a pidfd/handle) that the runtime can record and reconcile. Otherwise process-group ownership is not enough to defeat double-fork/`setsid` escape (see PROC-1).
- validation rules
- generated docs/schema from typed metadata (operation classes, env vars, ports, lifecycle support, readiness/health behavior, state roots, examples)

Concrete operation bindings compile to executable closure paths and arguments, but the semantic operation class is always typed. String dispatch such as `svc::<service>::<op>` may exist only as a generated compatibility ABI, never as the primary model.

Concrete adapters live under `adapters/` and are opt-in. A minimal Nixfied project must not evaluate postgres, nginx, minio, reth, helios, or any concrete adapter by default — and this laziness is proved by a test, not assumed.

Postgres is the canonical reference adapter — it exercises ports, state, lifecycle, readiness, health, workflows, persistence, and cleanup. Others stay smaller until the core contract is stable.

## Public Surfaces

Derived from the execution model, not a command wishlist (the `surfaces` field of `model.json` is the source). Two command layers; friendlier verbs mapped to single responsibilities (final names may change, but no duplicate families for the same action):

**`nixfied` (ergonomic, may compile):** `compile`, `validate [--deep]`, `model`/`introspect`, `schema`, `docs`, `up`, `down`, `run`, `ps`, `logs`, `clean`, `install`, `upgrade`.

**`nixfied-runtime` (pure engine, never evaluates):** `model` (inspect compiled model), `schema` (expose compiled schema), `docs` (expose generated docs), `check` (model + host admission assumptions), `run` (workflow/task), `up` (start required services), `down` (stop owned services), `ps` (reconcile + list observed state), `logs`, `clean` (reconcile + reap stale state/leases/processes).

`schema` and `docs` are first-class because discoverability is a product property: an agent or human must be able to read the schema and generated docs from a compiled output without source access.

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What compiler pass validates it?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those five, it is not core. This is the contributor rule, not a one-time review.

## Runtime Invariants (the laws the codebase must hold)

- **SEAM-1:** No Nix evaluation after admission; evaluation is confined to `compile` and `validate --deep`. The engine uses/realises pinned closures but never evaluates. Enforced structurally by the `nixfied-runtime` binary.
- **BUILD-1:** Runtime realisation of pinned closures is allowed only during admission, before process start, and only of concrete store paths/`.drv`s; never `nix build` of a flake reference. Forbidden during supervision, readiness/health, reconciliation, cancellation, GC, and cleanup.
- **MODEL-1:** The runtime never executes without a validated `model.json` whose `modelHash` matches its bytes.
- **ADMIT-1:** Every admitted run records the exact compiled-output path, `model.json` path, `schemaVersion`, and `modelHash` it used.
- **PORT-1:** No service is considered ready without verified ownership of its port; a registry reservation is intent, not an OS reservation.
- **PLACE-1:** Logical placement is baked by Nix (machine-independent, hashed into the model); host-absolute paths are materialised at admission from a resolved base; run-scoped paths are composed at runtime. Derivation for each phase lives in exactly one place.
- **REG-1:** One writer per slot registry for shared mutations; per-run streams are append-only and fsynced; snapshots are written via atomic rename and rebuildable from the streams.
- **PROC-1:** Every spawned process belongs to a runtime-owned process group, and there are no untracked children. Because process groups alone do not defeat double-fork/`setsid`/daemonization, containment is hardened per platform (Linux: cgroup v2 + `pidfd` + subreaper where available; macOS: process group + best-available handle, acknowledged weaker), and the adapter contract requires foreground execution unless a stable child identity is returned.
- **PROC-2:** Cancellation propagates to the whole process group.
- **PROC-3:** Every long-lived process has a registry record before it is considered started; no node depends on an untracked process.
- **LIVE-1:** Liveness is always reconciled against the OS before being reported; the registry is never trusted as a liveness oracle.
- **GC-1:** Cleanup is idempotent and safe to re-run after a crash; GC never deletes policy-protected persistent state.
- **SHELL-1:** Shell cannot own graph, registry, summary, validation, liveness, or cleanup semantics.
- **NIX-1:** Nix cannot own live process supervision, liveness reporting, cancellation, registry mutation, or cleanup.

These exist so the rebuild does not silently regrow the v1 failure mode.

## Runtime Error Contract

The product targets agents as first-class callers, so runtime errors are part of the contract, not just log text. The engine emits **stable, machine-readable error categories**, each with a stable code, a typed payload, and an exit class. Codes are versioned with `schemaVersion`; new categories are additive.

- `MODEL_MISMATCH` — `modelHash` mismatch / tampered or truncated model artifact.
- `SCHEMA_UNSUPPORTED` — unknown/future `schemaVersion`, or a model whose `runtime` requirements this engine cannot satisfy.
- `SCHEMA_VIOLATION` — a runtime input fails `schema.json` (bad slot index, out-of-bounds port override, …).
- `CLOSURE_MISSING` — a pinned store path/`.drv` is unavailable or cannot be realised at admission.
- `PORT_CONFLICT` — no port in the window could be owned within the collision policy.
- `STATE_UNWRITABLE` — the resolved state base/root cannot be created or written.
- `LEASE_STALE` / `LEASE_CONFLICT` — a lease expired, or is contended by another live owner.
- `PROC_ESCAPE` — a spawned process could not be tracked or reconciled (escaped the group).
- `READINESS_TIMEOUT` — a service did not reach `ready` within policy.
- `CANCELED` — the run was canceled.
- `CLEANUP_REFUSED` — GC declined to delete policy-protected persistent state.
- `REGISTRY_CORRUPT` — a registry stream/snapshot failed an integrity check.

## Milestones

Ordering hardens failure and lifecycle semantics **before** the first real stateful adapter, so adapter complexity cannot accumulate on top of unproven cancellation/GC (the documented v1 failure mode).

### Milestone 0 — Walking skeleton (thin slice through every layer)

Smallest end-to-end vertical slice; exercises the whole spine and nothing more:

- one typed Nix project module → one `compile` path producing `model.json` (`schemaVersion` + `modelHash`) and `schema.json`
- `nixfied-runtime` loads and validates the model (hash over raw bytes + schema)
- one env (`dev`), one slot (`0`), one task, one service with `Start`/`Ready`/`Stop`
- one registry event stream, one port reservation with ownership-verified readiness, one state root, one log root, one summary file
- `ps` reconciles and reports observed state; `model`/`introspect` reads the model

Complete when a downstream-shaped minimal example can: build `model.json`; start a service; verify port ownership at readiness; run a dependent task; write registry events and a summary; stop the owned service; record the admitted compiled-output path, model path, `schemaVersion`, and `modelHash`; and report no live owned processes after reconciliation. No adapters, parallel slots, workflows, or install yet.

### Subsequent milestones

1. **Slot isolation** — two concurrent slots of the same env: disjoint ports, state, logs, registry streams; independent reconciliation.
2. **Cancellation & GC hardening** — signal propagation, leases, `kill -9` survival and reconciliation, stale-reservation cleanup. Crash-safety is proven before any stateful adapter exists.
3. **Reference adapter (minimal Postgres)** — start → readiness (port ownership verified) → simple client task → artifacts → clean stop, with state preserved/deleted per policy. No full workflow complexity yet.
4. **Workflow graphs** — dependency-aware workflows with bounded tasks, service requirements (incl. reuse via `serviceInstanceId` leases), readiness gates, cancellation, artifacts, summaries; cancellation propagates and leaves explanatory evidence.
5. **Installable downstream wrapper** — adoption/upgrade without vendoring internals or losing project-owned config; `schemaVersion`/`runtime` gating.
6. **Polyglot example** — `examples/polyglot-stack`.

## Proof Strategy (tiered, to avoid a proof monolith)

Layered so no single workspace becomes the only evidence:

- Nix correctness/compiler proofs — model validation, closures, canonical JSON, stable `modelHash`.
- Runtime unit proofs — placement composition, port reservation, registry events, process identity, reconciliation, GC.
- Adapter-contract proofs without concrete services; adapter-specific proofs in isolation; a proof that a minimal project does **not** evaluate any concrete adapter.
- Downstream-shaped examples exercising public APIs only.

### Adversarial / negative proofs (required)

Failure-path coverage is explicit because the product's thesis is owned, reconciled, cleanable execution:

- tampered/truncated `modelHash` is rejected at the boundary;
- unknown future `schemaVersion` is refused;
- `nixfied-runtime` with `nix` unavailable still runs an already-realised model (no hot-path Nix);
- byte-identical `modelHash` between Nix-emitted bytes and the engine's on-disk hash;
- port conflict at *bind* time (not only preflight), and ownership-verified readiness;
- service daemonization/double-fork attempt is detected (`PROC_ESCAPE`) or contained;
- PID-reuse simulation and a stale lease whose PID is reused by an unrelated process;
- corrupted/truncated registry stream and snapshot rebuild after a partial write;
- two slots racing on registry and port allocation;
- GC refusing to delete policy-protected persistent state.

Initial examples (downstream-shaped workspaces with their own `flake.nix`, framework import, `README.md`, operating notes — not framework-owned fixtures):

- `examples/minimal`
- `examples/postgres-api`
- `examples/web-nginx`
- `examples/object-storage`
- `examples/polyglot-stack`

> Risk noted from v1: a single giant proof workspace becomes a second framework snapshot. The tiered split is deliberate; examples prove integration, not internals.

## Definition of Done (per concept)

- **Model:** a project compiles typed intent into canonical `model.json`; the engine rejects invalid, unsupported, or tampered (`modelHash` mismatch) artifacts before execution.
- **Discoverability:** a human or agent can run `model`/`introspect`, `schema`, and `docs` against a compiled output and discover available environments, slots, services, tasks, workflows, adapters, state policy, and supported surfaces without reading project source.
- **Environment:** `dev`, `test`, `ci` express different services, workflows, state and cleanup policy through the same typed model, with disjoint state roots and no cross-env leakage.
- **Slot:** two slots of the same env run concurrently with disjoint placement, ports, state, logs, artifacts, registry streams, and summaries; `kill -9` of one leaves the other intact and reconciling within one `ps`.
- **State:** roots are derived, inspectable, policy-controlled, and safe to clean without guessing which process owns them.
- **Registry:** the runtime reports live/stopped/stale/canceled/orphaned via reconciliation, not by trusting stale files; `clean` reaps orphans and releases their ports.
- **Service:** start → ready → health → stop owned end-to-end, with a port whose ownership is verified at readiness and a process group that fully tears down, and reconciliation after abnormal exit.
- **Workflow:** expresses service requirements (incl. reuse), task dependencies, cancellation, artifact collection, and cleanup policy, then produces a durable summary; cancellation mid-run leaves no orphans.
- **Adapter:** implements typed operation classes behind the stable contract without becoming framework core; the compiler rejects an adapter missing a declared class.
- **Install & Upgrade:** a downstream project can install or upgrade Nixfied without vendoring framework internals or overwriting project-owned configuration; `schemaVersion`/`runtime` gating refuses an incompatible pairing with a typed error.

## Repository Layout (greenfield)

```
flake.nix                  # apps, devShell, packages, compiled-output builder
nix/
  modules/                 # typed module options (core, env, slot, service, task, workflow, state)
  compiler/                # pure passes: resolve → validate → derive → emit + build closures
  lib/                     # pure helpers; logical placement derivation baked into model.json
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
- No live Nix evaluation during admitted execution, supervision, cancellation, registry mutation, or cleanup; no realisation outside admission.
- No host-absolute paths baked into the content-hashed model.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Deferred (explicit non-goals *for now*, to keep them from leaking back in)

Real and intentionally out of v2's first cut; listed so they are not designed in by accident:

- **Secrets/credentials management** — beyond modeled environment passthrough to runtime-owned commands; no sourcing/redaction model yet.
- **Inter-service dependency DAG** — services declare readiness; cross-service ordering beyond workflow phases is deferred.
- **Log aggregation/retention** — logs are runtime-owned files per run; centralized aggregation and modeled-log-artifacts are later.
- **Multi-host / remote execution** — registry and state are local-disk; distributed execution would require revisiting REG-1 and LIVE-1.
- **UI / dashboard surfaces.**

## Open Questions

Ranked by the gate at which each must be decided.

### Must decide before Milestone 0

1. Hash algorithm for `modelHash` (e.g. SHA-256) and final confirmation of the canonical form in [Canonicalization & hashing](#canonicalization--hashing) — including that closure store paths are inside the hash.
2. Is `model.json` produced as a Nix derivation output, a CLI output, or both?
3. Closure realisation mode: built eagerly at `compile` (slower compile, zero admission build) vs realised-at-admission from pinned `.drv`s (faster compile, admission latency). BUILD-1 forbids first-use-mid-run realisation, so "lazy on first use" is **not** a candidate.
4. `schema.json` representation — JSON Schema, or a bespoke typed constraint format that both Nix emits and Rust enforces without drift.
5. Runtime ⇄ model compatibility negotiation rules for the `runtime` field.
6. Registry storage for M0 — NDJSON event streams, a file-lock protocol over JSON, or SQLite.
7. Minimum cross-platform process-identity strategy for Linux and macOS.

### Before Slot Isolation / Cancellation & GC

8. Port-window base, size, and slot stride that avoid realistic host collisions without consuming too much space.
9. Default lease TTL and opportunistic GC cadence.
10. When the NDJSON registry log is compacted into the snapshot.

### Before Adapter / Workflow

11. Final closed set of adapter operation classes beyond the core six.
12. How much service-dependency behavior belongs in the Workflow milestone vs deferred.
13. Reference-count / lease-handoff state machine for reused service instances.

### Before Install

14. `nixfied`/`nixfied-runtime` distribution — pinned in the flake only, or also standalone release artifacts.

## Constraints From v1

Captured so the rebuild does not repeat history:

- Runtime semantics drifted into shell helpers and Nix builders, with no single owner. Hence SEAM-1, BUILD-1, SHELL-1, NIX-1, and the two-binary split.
- The registry was treated as the source of truth for liveness, which drifts from reality. Hence LIVE-1 and reconciliation.
- Pure port derivation (and even a registry reservation) was treated as sufficient, but the host has unrelated processes and the OS does not know the registry. Hence the ownership-verified PORT-1.
- Placement rules drifted when both Nix and runtime derived them. Hence the logical/materialised phase split and PLACE-1.
- Adapter complexity accumulated before lifecycle ownership was proven. Hence cancellation/GC hardening precedes the reference adapter in the milestone order.
- A single large proof workspace becomes a second framework snapshot. Hence the tiered proof strategy.
- Duplicate command families accreted for the same lifecycle action. Hence the model-derived public surface and the 5-question gate.

## Success Criteria

A downstream project can define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, runtime closures, and state policy in one typed Nix model; evaluate it for static correctness; compile it deterministically into a versioned, hashed `model.json`; then execute that admitted artifact through one Rust runtime with isolated placement, managed lifecycle, process-aware reconciliation, clean cancellation, crash-safe GC, durable registry history, and reproducible summaries. The acceptance checks in [Definition of Done](#definition-of-done-per-concept) are the measurable form of this statement.

## Decision Log (resolved forks)

Fork resolutions, pulled out of the body so the spec reads as decisions, not archaeology. Each notes the resolution and corrects the provenance where earlier inline notes were inaccurate.

- **One binary vs two → two binaries.** `nixfied` (ergonomic, may compile) and `nixfied-runtime` (pure engine). *Provenance correction:* both predecessor drafts centred a single binary — `RFC_v2cx` named it `nfd` with a cli/runtime *crate* split, not a binary split. The two-binary split is introduced here to make SEAM-1/BUILD-1 structural; it was not "adopted from `RFC_v2cx`" as an earlier note stated.
- **Run-ID format → ULID.** Sortable, timestamped, contention-safe. *Provenance correction:* `RFC_v2cx` already specified "ULID or equivalent"; the earlier note that it used a `YYYYMMDD-HHMMSS-<suffix>` format was inaccurate.
- **Placement derivation → phase split.** Logical placement baked by Nix; host-absolute paths materialised at admission; run-scoped paths composed at runtime. Supersedes dual Nix+Rust derivation guarded by a parity test (the prior `PARITY-1`), removing the drift class instead of testing for it.
- **`modelHash` agreement → hash the emitted bytes.** Nix writes canonical bytes; the engine hashes raw on-disk bytes and never re-serializes, so there is no second canonicalizer to disagree.
- **Realisation timing → admission only (BUILD-1).** Restored as a named law; "lazy realisation on first use" is explicitly rejected because it can stall a run mid-flight and blur the eval/realise boundary.
- **Service lifecycle phases → the universal six.** `Prepare · Start · Ready · Health · Stop · Clean`; `migrate`/`reset` are adapter extensions, not core classes.
- **Public surface naming → friendlier verbs.** `up/down/run/ps/logs/clean` over fragmented families, with `schema`/`docs` restored as first-class discoverability surfaces.
- **Milestone order → cancellation/GC before the reference adapter.** Hybrid of both drafts: skeleton → slot isolation → cancellation/GC hardening → minimal Postgres → workflow graphs → install → polyglot.
