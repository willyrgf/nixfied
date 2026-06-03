# RFC v2: Problem-First Nixfied Rebuild

Date: 2026-06-03
Status: Draft (build-ready)
Supersedes: all prior Nixfied implementation. This branch is a from-scratch rebuild; nothing from v1 is preserved except the lessons captured in [Constraints From v1](#constraints-from-v1).

## Problem

Modern projects are not one program. They are small systems: frontend, API, workers, databases, queues, object storage, migrations, test harnesses, infrastructure scripts, CI jobs, and local developer workflows, often written in different languages and owned by different tools.

The operational behavior of those systems is usually scattered across `flake.nix`, shell scripts, package-manager scripts, CI YAML, Docker compose files, service wrappers, env files, port conventions, and local tribal knowledge.

The deeper problem is that projects have no clean, reproducible way to run multiple environments, or multiple copies of the same environment, while keeping services, workflows, state, logs, ports, artifacts, and cleanup under one process-aware authority.

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

Nixfied v2 is a runtime coordinator for project-shaped systems with a deterministic model, deterministic placement, and owned execution.

It codifies project intent in Nix, evaluates that intent to typed correctness, compiles it into immutable artifacts, then runs those artifacts through one Rust runtime that can start, inspect, record, reconcile, cancel, and clean up the execution graph across environments, slots, services, workflows, and codebases.

Three properties define the product, and they are not the same:

- **Codified and discoverable.** The entire project, including every capability, service, task, workflow, environment, adapter, state policy, and constraint, is expressed as typed data. A human or agent can discover what the project can do and how to use it by reading the compiled output, without studying the source or assuming conventions.
- **Deterministic model and placement.** Given the same inputs, the compiled model, derived identities, candidate placement, state roots, registry paths, artifact paths, and log paths are reproducible.
- **Owned execution.** Runtime behavior, timing, readiness, process scheduling, and external host state are not deterministic. What the runtime guarantees is that every admitted process is owned, tracked, attributable, reconciled against OS reality, and cleanable.

Nixfied is not primarily a task runner, service template collection, CI wrapper, Docker replacement, or Nix scaffold. Those may exist as surfaces or adapters, but the product is the model plus owned-execution runtime.

## The Load-Bearing Decision: Two Tools, One Seam

Nixfied is split across exactly two substrates, chosen for what each is good at. The split is defined by verbs, not by vague phrases such as "Nix at runtime."

| Verb | Example | Owner |
|------|---------|-------|
| Evaluate | flake to derivations, module assertions, type checks | Nix, at explicit authoring surfaces only |
| Build or realize | dependency binaries, adapter scripts, helper tools | Nix builds them; Rust may use pinned results |
| Enforce | port override bounds, allowed env names, process ownership | Rust, against compiled rules and OS reality |

Nix is the authority for what the project is allowed to be. Rust is the authority for what the project is currently doing.

### Nix Authority

Nix owns project codification, static correctness, and reproducible runtime inputs:

- typed project modules
- model validation
- module assertions
- capability discovery
- generated schemas and docs
- reproducible task, adapter, helper, and dependency closures
- the compiled output consumed by the runtime

Nix never starts project services, touches mutable runtime state, observes liveness, writes registry events, owns cancellation, or performs cleanup.

### Rust Authority

Rust owns admitted execution and observed runtime state:

- model loading
- schema enforcement
- host-specific admission checks
- placement materialization
- port reservation
- process groups
- signal propagation
- service readiness and health observation
- task and workflow execution
- registry events
- reconciliation
- cancellation
- garbage collection
- cleanup

Rust may use Nix-produced store paths and may realize pinned derivations during admission, but it never evaluates Nix after execution admission begins.

### Seam Invariants

These two halves communicate through one immutable compiled output and two core rules:

- **SEAM-1:** The run hot path never evaluates Nix. Nix evaluation is confined to explicit `compile` and `validate --deep` surfaces.
- **BUILD-1:** Runtime realization of pinned derivations is allowed only during admission, before process start. It is forbidden during process supervision, liveness reporting, reconciliation, cancellation, GC, and cleanup.

Confining evaluation to authoring time does not weaken Nix's correctness guarantee. The runtime can only execute an artifact that is the output of a successful Nix evaluation. A configuration that fails type checks or assertions never produces a compiled output, so it can never be admitted.

For humans and agents, this is the better contract: correctness feedback arrives at compile time as typed errors, and discovery happens through stable machine-readable artifacts rather than implicit project conventions.

### Why Rust

Rust is the runtime substrate because:

- process groups, signals, async I/O, timeouts, pidfd support, and crash-safety primitives are first-class enough for the job
- a single binary is easy to ship through Nix and easy to execute from thin flake apps
- strong types mirror the typed model thesis end-to-end
- malformed compiled outputs fail loudly at the deserialization and schema boundary
- registry locking, atomic writes, fsync, and reconciliation logic belong in a typed program, not shell helpers

## Architecture and Data Flow

```text
project intent (typed Nix modules)
            |
            v
   +-----------------+
   |  Nix compiler   |  pure passes: resolve -> validate -> derive -> emit
   |  nix/compiler   |  builds or pins deps, helpers, and adapter scripts
   +--------+--------+
            |
            v
   compiled output directory
     |
     +-- model.json
     +-- schema.json
     +-- closures.json
     +-- capability catalog
     +-- generated docs
            |
            v
   +------------------------------------------------------+
   | nfd (Rust runtime, single execution authority)        |
   |                                                       |
   | loader + enforce -> admission -> orchestrator         |
   |                         |             |               |
   |                         v             v               |
   |                     registry <-> supervisor           |
   |                         |       processes, groups,    |
   |                         v       signals, readiness    |
   |                  state root on disk                   |
   +------------------------------------------------------+
            |
            v
   registry, state roots, logs, artifacts, summaries
```

Thin flake apps may resolve the compiled output and execute `nfd`, but they contain no runtime semantics. Shell may be used as a user convenience or adapter implementation detail, but shell output is not a semantic sidecar.

## Repository Layout

```text
flake.nix                  # apps, devShell, nfd package, compiled-output builder
nix/
  modules/                 # typed module options: core, env, slot, service, task, workflow, state
  compiler/                # pure passes: resolve -> validate -> derive -> emit
  lib/                     # pure helpers; placement may be mirrored here for introspection
runtime/
  Cargo.toml
  crates/
    nfd-model/             # serde types for model.json, schemaVersion, compiled-output contract
    nfd-runtime/           # supervisor, registry, state, reconciliation, admission
    nfd-cli/               # command surfaces over nfd-runtime
adapters/                  # opt-in service adapters; not evaluated by default
examples/                  # downstream-shaped proof workspaces
docs/
```

## Core Concepts

- **Compiled output:** the immutable, versioned directory produced by Nix and consumed by Rust. This is the only semantic seam.
- **Model:** the compiled contract for what exists and how it can run. Serialized as `model.json`.
- **Schema:** compiled runtime-enforceable constraints. Serialized as `schema.json`.
- **Closure:** a reproducible executable, adapter operation, helper, generated script, or dependency output produced or pinned by Nix.
- **Environment:** a named mode of operation, such as `dev`, `test`, `ci`, preview, or prod-like.
- **Slot:** a deterministic parallel instance of an environment.
- **Run:** one admitted runtime invocation with a unique run identity.
- **Placement:** derived paths, candidate ports, state roots, log roots, artifact roots, and registry keys for a project/environment/slot/run.
- **State:** all mutable roots derived from project, environment, slot, service, workflow, and run identity.
- **Registry:** a durable, reconciled projection of intent, history, and ownership. The OS owns liveness.
- **Service:** a long-lived process with lifecycle, readiness, health, logs, state, and ownership.
- **Workflow:** a dependency graph of tasks and service phases with artifacts, summaries, cancellation, and cleanup.
- **Adapter:** language-, service-, or tool-specific implementation behind a stable typed model/runtime contract.

## Correctness Layers

Nixfied v2 separates desired-state correctness from observed-runtime correctness.

### 1. Model Correctness

Nix owns the typed declaration of project capabilities:

- codebases
- environments
- slots
- services
- workflows
- tasks
- adapters
- state policy
- cleanup policy
- generated machine-readable interfaces

Nix validation rejects invalid names, broken references, incompatible adapter declarations, invalid workflow edges, missing required fields, and malformed state or placement policy before runtime execution is possible.

### 2. Closure Correctness

Nix owns reproducible construction or pinning of the commands the runtime may execute:

- runtime command closures
- task command closures
- adapter operation closures
- helper binaries
- generated scripts
- schemas and docs

The Rust runtime receives executable paths from the compiled output instead of discovering project tooling heuristically.

### 3. Admission Correctness

Rust owns host-specific checks that cannot be proven by Nix evaluation alone:

- port availability
- writable state roots
- registry lock acquisition
- stale lease reconciliation
- executable availability or realization on the current host
- process ownership conflicts
- environment-specific runtime preconditions

Admission happens after a compiled output is selected and before any long-lived service is started.

### 4. Execution Correctness

Rust owns the impure execution graph:

- process groups
- signal propagation
- service readiness and health observation
- task execution
- workflow cancellation
- registry events
- summaries
- cleanup
- reconciliation against OS state

This division keeps Nix central to project evolution and agent-readable codification while preventing Nix evaluation from becoming a live process supervisor.

## Compiled Output Contract

The seam is not a single file. The seam is one immutable compiled-output directory.

Required files:

- `model.json`: typed project model consumed by `nfd-model`.
- `schema.json`: runtime-enforceable validation rules authored by Nix and enforced by Rust.
- `closures.json`: pinned executable, helper, adapter, dependency, and optional derivation references.

Optional files:

- `capabilities.json`: machine-readable catalog for agents and humans.
- `docs/`: generated documentation from typed metadata.

Required `model.json` fields:

- `schemaVersion`: integer, initially `1`.
- `modelHash`: hash of the canonical model excluding `modelHash`.
- `project`: stable project metadata.
- `environments`: named environment definitions.
- `slotPolicy`: slot range, defaults, and placement-relevant slot policy.
- `statePolicy`: state roots, cleanup policy, persistence policy, and retention rules.
- `placementPolicy`: port windows, path derivation inputs, collision policy, and override bounds.
- `services`: declared service models.
- `tasks`: declared bounded task models.
- `workflows`: declared workflow graphs.
- `adapters`: adapter metadata and operation bindings.
- `surfaces`: public command surfaces derived from the model.
- `closures`: executable paths or pinned derivation references for runtime-dispatched operations.
- `runtime`: minimum compatible runtime version and required runtime features.

Compiled output rules:

- The compiled output is immutable for a run.
- Every admitted run records the compiled output path, `model.json` path, `schemaVersion`, and `modelHash`.
- The runtime refuses unknown future `schemaVersion` values.
- The runtime validates `modelHash` before execution.
- Canonicalization must be stable across machines for the same typed input.
- Nix compile/check validation errors must happen before runtime execution.
- Runtime validation may reject host-specific conflicts, such as unavailable ports, missing executables, invalid state roots, or incompatible adapter closures.
- The runtime never mutates the compiled output.

## The Iteration Loop

Evolving a project is one explicit correctness loop:

```text
edit typed Nix
  -> nfd compile
  -> typed checks and assertions
  -> compiled output
  -> nfd run
```

This loop must be fast and first-class because it is how humans and agents iterate safely:

- `nfd compile` is the common-path surface that triggers Nix evaluation.
- Compile failures are typed and actionable, surfaced before any process starts.
- `nfd validate` checks an already-compiled output and live host assumptions without Nix evaluation.
- `nfd validate --deep` is explicit and may re-evaluate Nix against the current project tree.
- Watch mode may exist for authoring, but it does not change the admitted model of an already-running process.

## Identity and Placement

Every runtime action is scoped by explicit identity:

```text
project / environment / slot / run_id
```

Definitions:

- `project`: stable project identifier from the model.
- `environment`: model-defined environment name.
- `slot`: explicit integer index, default `0`.
- `run_id`: ULID or equivalent sortable unique identifier minted by the runtime per admitted run.

Placement is deterministic for paths and candidate ports:

```text
state_root    = $NIXFIED_STATE_DIR/<project>/<environment>/<slot>/
registry_dir  = state_root/registry/
run_dir       = state_root/runs/<run_id>/
artifacts_dir = run_dir/artifacts/
logs_dir      = run_dir/logs/

port_window   = port_base
              + (stable_hash(project, environment) % window_count) * window_size
              + slot * slot_stride
```

`port_base`, `window_size`, `slot_stride`, and `window_count` are model-level placement-policy fields. The derivation yields a candidate window, not a guaranteed-free port.

### Port Reservation

Pure derivation is not enough for ports because the host may already have unrelated processes.

Port reservation rules:

- Compute the candidate window for project, environment, and slot.
- Acquire the slot registry lock.
- Pick a candidate port according to the model collision policy.
- Verify the port by binding or equivalent host-specific check.
- Reserve the concrete port in the registry against the run before service start.
- Release the reservation on service stop, cleanup, or reconciliation after proving the owner is gone.

No service starts without successful placement and port reservation.

### Placement Parity

Rust is authoritative for runtime placement. Nix may reproduce placement derivation for introspection, docs, and compile-time previews.

**PARITY-1:** Placement derivation has one authoritative runtime definition, and a parity proof asserts that Nix previews and Rust execution agree. Disagreement is a build failure.

## Registry Semantics

The registry is not the source of truth for OS liveness. The OS is.

The registry is a durable, reconciled projection of:

- intended ownership
- process identity
- lifecycle transitions
- readiness and health observations
- port reservations
- state roots
- logs and artifacts
- workflow and task events
- cancellation events
- stop policy
- cleanup results

Initial storage:

- one append-only NDJSON event log per slot
- one derived snapshot for fast reads
- one advisory lock per slot registry for mutating operations

Registry write rules:

- Event appends are atomic single writes.
- Snapshot updates use temp-file plus atomic rename.
- Mutating operations acquire the slot registry lock.
- Parallel slots do not share registry files.
- Readers tolerate partially completed runs.
- Cleanup is idempotent.
- Stale leases are recoverable.

Every process record must include enough identity to survive PID reuse:

- pid
- process group id
- start time or platform-equivalent process identity
- optional pidfd or platform-specific stable handle where available
- command metadata
- owning run ID

The runtime reconciles registry records with OS state before reporting liveness, reusing a service, releasing reservations, performing cleanup, or reporting a process as orphaned.

Leases and GC are part of the registry contract. A crashed runtime leaves stale leases. `nfd gc`, plus opportunistic sweeps on later invocations, detects expired leases, reaps owned process groups when policy allows, releases ports, and writes terminal events.

## Runtime Responsibilities and Public Surfaces

The public API is derived from the execution model, not from historical command families.

The working binary name is `nfd`. Final naming may change, but the command model should stay verb-explicit.

Required surfaces:

- `compile`: evaluate typed Nix and produce a compiled output.
- `introspect`: read the compiled model and show project capabilities.
- `schema`: expose compiled schema information.
- `docs`: expose generated docs from typed metadata.
- `validate`: validate compiled output and host-specific admission assumptions without Nix evaluation.
- `validate --deep`: explicit opt-in validation that may re-evaluate Nix.
- `run-task`: run a bounded task.
- `run-workflow`: run a workflow graph.
- `svc <service> <op>`: execute a typed service lifecycle operation.
- `runs`: reconcile and list observed runtime state.
- `logs`: locate or stream runtime-owned logs.
- `stop-run`: stop owned processes for one run.
- `stop-all-runs`: stop owned processes for a scope.
- `gc`: reconcile and clean stale state, leases, reservations, and owned processes.
- `install`: add a downstream wrapper.
- `upgrade`: upgrade a downstream wrapper without overwriting project-owned configuration.

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What compiler pass validates it?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those five questions, it is not core. This gate is the contributor rule, not a one-time review.

## Services and Workflows

Services are long-lived processes. Workflows are bounded execution graphs.

Core service lifecycle operation classes are a closed enum:

- `prepare`
- `start`
- `ready`
- `health`
- `stop`
- `clean`

Adapter-specific actions such as migrations or resets may be modeled as typed tasks or workflow nodes until they justify promotion into the core lifecycle contract.

Workflow node classes:

- task
- service requirement
- readiness gate
- artifact collection
- cleanup action

A workflow run must know:

- which services it requires
- whether each service is owned, reused, or forbidden
- readiness criteria before dependent nodes run
- cancellation behavior
- cleanup policy
- artifact and summary locations

No workflow node may depend on an untracked service process.

## Adapter Strategy

Core stays adapter-free.

Core owns only the typed adapter contract:

- service model shape
- lifecycle phases
- closed enum of operation classes
- operation input schema
- operation result schema
- registry and ownership expectations
- validation rules
- generated docs and schema from typed metadata

Concrete operation bindings may compile to executable paths and arguments, but the semantic operation class must be typed. Dispatch surfaces such as `svc <service> <op>` are backed by typed operations rather than free-form string parsing.

Concrete adapters live under `adapters/` and are opt-in only. A minimal Nixfied project must not evaluate postgres, nginx, minio, reth, helios, or any other concrete adapter by default.

Postgres is the canonical reference adapter because it exercises ports, state, lifecycle, readiness, health, workflows, persistence, and cleanup. Other examples should stay smaller and adapter-specific until the core contract is stable.

Adapter docs should be generated from typed metadata:

- operation classes
- env vars
- ports
- lifecycle support
- readiness and health behavior
- state roots
- examples

## Runtime Invariants

These are runtime laws. Features that violate them are not core.

- **SEAM-1:** The run hot path never triggers Nix evaluation. Evaluation is confined to explicit `compile` and `validate --deep` surfaces.
- **BUILD-1:** Runtime realization of pinned derivations is allowed only during admission, before process start.
- **MODEL-1:** The runtime never executes without a validated compiled output.
- **ADMIT-1:** Every admitted run records the exact compiled output path, model path, schema version, and model hash it used.
- **PORT-1:** No service starts without a reserved, verified port.
- **PARITY-1:** Placement derivation has one authoritative runtime definition, parity-checked against Nix previews.
- **REG-1:** Mutating operations have one writer per slot registry; appends are atomic; snapshots use atomic rename.
- **PROC-1:** Every spawned service process belongs to a runtime-owned process group.
- **PROC-2:** Cancellation propagates to the whole owned process group.
- **LIVE-1:** Liveness is always reconciled against the OS before being reported.
- **GC-1:** Cleanup is idempotent and safe to re-run after a crash.
- **SHELL-1:** Shell cannot own graph, registry, summary, validation, liveness, or cleanup semantics.
- **NIX-1:** Nix cannot own live process supervision, liveness reporting, cancellation, registry mutation, or cleanup.

These exist so the rebuild does not silently regrow the v1 failure mode.

## Required Capabilities

1. **Deterministic env and slot execution.** Identity and placement derive from explicit project, environment, slot, and run identity. Ports are reserved and verified at runtime. Multiple slots of the same environment run without collision.
2. **Reconciled process registry.** The runtime records services, workflows, tasks, child processes, lifecycle state, readiness, health, cancellation, artifacts, summaries, leases, and stop ownership, then reconciles records against OS liveness on read.
3. **Service and workflow lifecycle ownership.** A run knows which services it needs, which instances it owns or reuses, when they are ready, how they are checked, and how they are stopped or left running by policy.
4. **Polyglot project coordination.** The framework coordinates many small codebases and toolchains without forcing one language, package manager, service runner, or repo shape.
5. **Typed model and pure compiler.** Project intent is typed Nix, evaluated into the canonical immutable compiled output, validated before emit, and exposed through stable introspection and schemas.
6. **One runtime authority.** Execution, process ownership, run records, summaries, artifacts, cancellation, cleanup, environment sandboxing, and reconciliation belong to the Rust runtime.
7. **Optional service adapters.** Core defines the typed lifecycle contract and dispatcher. Concrete adapters are opt-in modules, not framework identity.
8. **Installable downstream wrapper.** A project adopts or upgrades the framework without vendoring framework internals or losing project-owned configuration.
9. **Proof through real execution.** Proof is tiered and anchored by downstream-shaped example workspaces that exercise public surfaces.

## Build Order

### Milestone 0: Walking Skeleton

The first runnable thing exercises the whole spine and nothing more:

- one typed Nix project module
- one Nix compile/check path producing a compiled output with `model.json`, `schema.json`, and `closures.json`
- one Rust CLI that loads the compiled output and enforces the schema
- one environment: `dev`
- one slot: `0`
- one bounded task that runs to completion
- one trivial service that starts, reports ready, and stops
- one port reservation that is acquired and released
- one state root
- one log root
- one slot registry event log
- one summary file
- one `runs` command that reconciles observed state
- one `introspect` command that reads the compiled model

Milestone 0 is complete when a downstream-shaped minimal example can:

1. compile project intent into a compiled output
2. record the admitted compiled output path and model hash
3. start a service
4. reserve and release a port
5. wait for readiness
6. run a dependent task
7. write registry events and a summary
8. stop the owned service
9. report no live owned processes after reconciliation

No adapters, parallel slots, workflows, or install/upgrade behavior belong in Milestone 0.

### Milestone 1: Slot Isolation

Add two concurrent slots for the same environment.

Acceptance check: two slots of the same environment run concurrently, bind disjoint ports, write disjoint state, write disjoint logs, and reconcile independently.

### Milestone 2: Cancellation and GC

Add signal propagation, leases, crash recovery, stale reservation cleanup, and `kill -9` survival.

Acceptance check: killing a runtime leaves enough registry evidence for `nfd runs` to report stale or orphaned state, and `nfd gc` reaps owned process groups and releases ports without damaging another slot.

### Milestone 3: Workflow Graphs

Add dependency-aware workflows with bounded tasks, service requirements, cancellation, artifacts, cleanup policy, and summaries.

Acceptance check: canceling a workflow propagates to owned tasks and services according to policy, records cancellation events, and leaves enough evidence to explain what ran.

### Milestone 4: Adapter Contract and Postgres

Add the typed adapter contract and Postgres as the canonical reference adapter.

Acceptance check: a downstream-shaped example starts Postgres, waits for readiness, runs a client workflow, records artifacts, stops cleanly, and preserves or deletes state according to policy.

### Milestone 5: Installable Downstream Wrapper

Add the adoption and upgrade path for downstream projects.

Acceptance check: a downstream project can consume Nixfied without vendoring framework internals into project-owned files and can upgrade the framework without losing project-owned configuration.

### Milestone 6: Polyglot Example

Add a multi-codebase example that demonstrates cross-language coordination through the model.

Acceptance check: the example coordinates at least two language ecosystems, one service, one workflow, isolated state, artifacts, summaries, and cleanup through public surfaces only.

## Proof Strategy

Proofs should be tiered, not one monolithic proof workspace.

Required proof layers:

- Nix compiler proofs for model validation, assertions, placement previews, schemas, and canonical JSON.
- Rust unit proofs for identity, placement, port reservation, registry append/snapshot/reconciliation, process identity, process groups, cancellation, and GC.
- Parity proofs between Nix placement previews and Rust placement execution.
- Adapter contract proofs without concrete services.
- Adapter-specific proofs in isolation.
- Downstream-shaped examples that exercise public APIs only.
- End-to-end proofs for slot isolation, cancellation, crash recovery, cleanup, and summaries.

Initial examples:

- `examples/minimal`
- `examples/postgres-api`
- `examples/web-nginx`
- `examples/object-storage`
- `examples/polyglot-stack`

Examples are downstream-shaped workspaces with their own `flake.nix`, framework import, `README.md`, and operating notes. They are not framework-owned fixtures that encode historical implementation structure.

## Definition of Done

Each concept ships only when its acceptance check passes.

### Model

A malformed model is rejected at the `nfd-model` boundary with a typed error. Unsupported `schemaVersion` values are refused.

### Discoverability

A human or agent can run `nfd introspect` against a compiled output and discover available environments, slots, services, tasks, workflows, adapters, state policy, and supported surfaces without reading project source.

### Environment

At least `dev`, `test`, and `ci` can express different services, workflows, state policy, and cleanup policy through the same typed model without cross-environment leakage.

### Slot

Two slots of the same environment run concurrently, bind disjoint ports, write disjoint state, and reconcile independently. Killing one slot leaves the other intact and reconciling within one `nfd runs` call.

### State

All mutable roots resolve under the derived state root. GC removes finished-run state and leaves policy-owned persistent state untouched.

### Registry

After a runtime crash, `nfd runs` reports orphaned or stale services based on OS reconciliation, not stale registry trust. `nfd gc` reaps owned processes and releases ports.

### Service

A service can run `start -> ready -> health -> stop` under runtime ownership, with a reserved port and a process group that fully tears down on stop.

### Workflow

A workflow can express service requirements, task dependencies, cancellation behavior, artifact collection, and cleanup policy, then produce a durable summary.

### Adapter

Postgres implements its declared operation classes, and the compiler rejects an adapter missing a declared class.

### Install and Upgrade

A downstream project can install or upgrade Nixfied without vendoring framework internals or overwriting project-owned configuration.

## Non-Goals

- No compatibility with the current repository layout.
- No migration layer for v1 commands.
- No broad downstream project template as framework core.
- No required service adapter for a minimal project.
- No daemon requirement for the initial runtime.
- No remote or multi-host execution.
- No duplicate command families for the same lifecycle action.
- No runtime semantics in shell or Nix builders.
- No live Nix evaluation during admitted execution, process supervision, cancellation, registry mutation, reconciliation, or cleanup.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Deferred

These are real and intentionally out of v2's first cut. Listing them keeps them from being designed in by accident.

- Secrets and credentials management beyond modeled environment requirements and explicit environment passthrough.
- Inter-service dependency DAGs beyond workflow service requirements and readiness gates.
- Centralized log aggregation and retention beyond per-run log placement.
- Multi-host or remote execution.
- A persistent daemon as a required runtime substrate.
- UI or dashboard surfaces.

## Open Questions

1. What port window size, slot stride, and default base avoid realistic host collisions without consuming too much space?
2. What lease TTL and opportunistic GC cadence should be used by default?
3. When should the NDJSON registry log be compacted into snapshots?
4. What is the final closed set of core lifecycle operation classes?
5. Should `nfd` be distributed only through flakes or also as a standalone release artifact?
6. Should dependency closures be built eagerly at compile time or pinned and realized during admission?
7. What compile latency target keeps the iteration loop ergonomic for humans and agents?
8. Should `schema.json` be JSON Schema or a bespoke typed constraint format emitted by Nix and enforced by Rust?
9. What is the minimum cross-platform process identity strategy for macOS and Linux?
10. Are selected logs modeled artifacts, or are logs purely runtime-owned files unless promoted by workflows?

## Constraints From v1

Captured so the rebuild does not repeat history:

- Runtime semantics drifted into shell helpers and Nix builders, with no single owner. Hence `SEAM-1`, `SHELL-1`, `NIX-1`, and a Rust-owned execution runtime.
- The registry was treated as the source of truth for liveness, which drifts from reality. Hence `LIVE-1` and reconciliation.
- Pure port derivation was treated as sufficient, but the host has unrelated processes. Hence `PORT-1` and runtime reservation.
- Placement rules can drift when both Nix and runtime derive them. Hence `PARITY-1`.
- A single large proof workspace tends to become a second framework snapshot. Hence the tiered proof strategy.
- Duplicate command families accreted for the same lifecycle action. Hence model-derived public surfaces and the 5-question contributor gate.

## Success Criteria

A downstream project can define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, runtime closures, and state policy in one typed Nix model; evaluate it for static correctness; compile it into an immutable, versioned compiled output; then run and inspect any environment or slot through the single Rust runtime with deterministic placement, isolated state, owned and reconciled lifecycle, clean cancellation, crash-safe GC, durable registry history, and reproducible summaries.

The acceptance checks in [Definition of Done](#definition-of-done) are the measurable form of this statement.
