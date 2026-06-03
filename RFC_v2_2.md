# RFC v2.2: Manifest-Sealed Greenfield Nixfied Build Spec

Date: 2026-06-03
Status: Draft (architecture-ready, not implementation-ready until Milestone 0 gates are closed)
Derives from: `RFC_v2.md` after architecture review. Fork resolutions and new design corrections are recorded in the [Decision Log](#decision-log-resolved-forks); the body states the resolved decision directly.
Supersedes: all prior Nixfied implementation. This branch is a from-scratch rebuild; nothing from v1 is preserved except the lessons captured in [Constraints From v1](#constraints-from-v1).

## Decision Summary

Nixfied v2 is a greenfield rebuild. The existing framework, proof workspaces, shell sidecars, historical command structure, and repository layout are not compatibility constraints.

The load-bearing architecture decision:

- **Nix is the authority for what the project is allowed to be** — codification, static correctness, and reproducible runtime inputs (closures).
- **Rust is the authority for what the project is currently doing** — admitted execution, observed runtime state, process ownership, and cleanup.
- Nix compile/check phases produce an immutable, versioned, **manifest-sealed compiled output** consumed by the Rust execution engine. The primary seam is `manifest.json`; the primary semantic artifact is `model.json`.
- The split is enforced *structurally* by two binaries: `nixfied` (ergonomic, may evaluate/build/realise during explicit authoring or prepare phases) and `nixfied-runtime` (pure engine, cannot invoke Nix at all).
- For Milestone 0, all runtime closures are realised before `nixfied-runtime` starts. The runtime admits only concrete, already-realised executable store paths; it never evaluates Nix, never calls `nix-store`, and never builds or realises during execution admission.
- Shell may exist as a user convenience or an adapter implementation detail, never as the owner of graph, registry, summary, validation, process-lifecycle, or cleanup semantics.
- Local runtime state uses a per-slot SQLite WAL registry from Milestone 0; NDJSON is an optional audit/export format, not the source of shared mutable truth.
- Source/codebase identity, target-system identity, service compatibility identity, runtime capabilities, adapter ABI, secrets non-leakage, and cleanup safety are part of the first artifact/runtime contract, not later polish.

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
- **Deterministic model & logical placement.** Given the same typed input, target identity, and source identity policy, the compiled model, manifest hashes, derived identities, and *logical* placement (template roots, relative layouts, registry keys, candidate port windows) are byte-for-byte reproducible. Host-absolute paths are **not** part of this guarantee: they are materialised at admission from a host-resolved base (see [Identity & Placement](#identity--placement)). What is reproducible is the logical placement; what is host-specific is resolved at runtime.
- **Owned execution.** Runtime behavior (timing, readiness, scheduling, external services) is *not* deterministic. The runtime instead guarantees that every process is owned, tracked, attributable, reconciled against OS reality, and cleanable.

Nix is the authority for what the project is allowed to be. Rust is the authority for what the project is currently doing.

Nixfied is not primarily a task runner, service template collection, CI wrapper, Docker replacement, or Nix scaffold. Those may exist as surfaces or adapters, but the product is the model/runtime contract.

## The Load-Bearing Decision: Two Tools, One Seam

The split is defined by the **verb**, not by timing. "Nix at runtime" conflates three different operations; we separate them:

| Verb | Example | Owner |
|------|---------|-------|
| **Evaluate** | flake → derivations, type-checks, module assertions | **Nix**, at authoring surfaces (`compile`, `validate --deep`) only |
| **Build / realise** | the postgres binary, an adapter's operation closure | **Nix / `nixfied` prepare surfaces**, before `nixfied-runtime` is invoked |
| **Enforce** | "is this runtime port override valid?" | **Rust**, against rules and constraints emitted into the manifest-sealed model |

1. **Nix is the source of truth and the correctness gate.** Typed Nix modules, with no impure access, *evaluate* into the compiled output. Nix also *builds/realises* the closures the runtime needs — dependency binaries, adapter operations, helper scripts — before `nixfied-runtime` is invoked. Nix never starts a project service, never touches mutable runtime state, never observes the running system.

2. **Rust is the runtime, and the only runtime.** The execution engine reads the manifest-sealed compiled output and owns everything impure: admission checks, process supervision, process-group ownership, signal propagation, readiness/health polling, the registry, state placement, cancellation, reconciliation, and cleanup. It *uses* Nix's already-realised outputs, but never invokes Nix, never calls `nix-store`, never evaluates, and never builds or realises anything.

These two halves communicate through one compiled output and three rules:

> **Invariant SEAM-1 (no Nix in the runtime).** Nix *evaluation* is confined to the explicit `compile` and `validate --deep` surfaces. Nix build/realisation is confined to `compile` or explicit prepare/realise surfaces owned by `nixfied`, before `nixfied-runtime` is invoked. Once `nixfied-runtime` starts, nothing — `run`, `up`, `down`, `ps`, reconciliation, GC, cleanup — invokes `nix`, `nix-store`, `nix build`, `nix eval`, imports a Nix expression, or builds/realises a path.

> **Invariant BUILD-1 (runtime admits realised paths only).** For Milestone 0, the runtime accepts only concrete, already-realised executable store paths recorded in the manifest-sealed compiled output. It verifies existence, executability, target-system compatibility, and manifest hash binding. It does not realise pinned `.drv`s. Any later admission-realisation mode must be an explicit, separately named capability and must not be part of the default runtime contract.

> **Invariant MANIFEST-1 (the seam is sealed by a manifest).** `model.json` does not contain a hash of itself. `manifest.json` records the hashes, target identity, source identity, generator identity, closure metadata, and bundle hash for the compiled output. The runtime validates `manifest.json` before trusting or deserializing the artifacts it names.

The two-binary split below is what enforces these invariants by construction rather than by review discipline.

- **`nixfied`** — the ergonomic, integrated CLI. May run explicit Nix `compile`, `validate --deep`, and prepare/realise phases, then delegate to the engine. (`nfd` is an accepted short alias.)
- **`nixfied-runtime`** — the pure execution engine. Consumes manifest-sealed compiled artifacts by path and **cannot** invoke Nix. SEAM-1, BUILD-1, and MANIFEST-1 live here as properties of the binary, not promises.

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

- **Compiled output:** the immutable directory produced by Nix/`nixfied` and consumed by the runtime. Its integrity and identity are sealed by `manifest.json`.
- **Manifest:** `manifest.json`, the artifact envelope for the compiled output. It records hash algorithm, artifact hashes, compiled-output hash, source identity, target identity, generator identity, runtime requirements, and closure metadata.
- **Model:** `model.json`, the compiled semantic contract for what exists and how it can run. It does **not** contain a hash of itself.
- **Closure:** a reproducible executable, adapter operation, helper, schema, or generated script produced by Nix and referenced by concrete realised store path.
- **Codebase / source identity:** a first-class model entry describing a logical codebase, its source mode, allowed live-workspace policy, and the source fingerprint or snapshot identity an admitted run may observe.
- **Target identity:** the Nix `system`, Rust target triple, OS family, architecture, ABI/libc where relevant, closure system, and required runtime capabilities for which a compiled output is valid.
- **Environment:** a named mode of operation, such as `dev`, `test`, `ci`, preview, or prod-like.
- **Slot:** a deterministic parallel instance of an environment, identified by an integer index.
- **Run:** one runtime invocation with a unique run identity (`runId`).
- **Service address:** the stable logical address of a service in a slot, derived from `(projectId, env, slot, serviceName)`.
- **Service spec hash:** the compatibility identity for a service definition: adapter ABI/version, closure paths, command/args/env shape, endpoint policy, readiness/health, state epoch, cleanup policy, target identity, and required runtime features.
- **Service instance:** a reusable running service identified by `serviceInstanceId = hash(serviceAddress, serviceSpecHash, logicalPortSet, targetIdentity)`. It is **distinct from `runId`**. Reservations, leases, logs, and ownership attach to the instance; a run *holds* or *borrows* an instance through explicit leases.
- **Run lease:** a heartbeat/TTL record for an active runtime invocation.
- **Service lease / claim:** the durable ownership/lifetime record for a service instance that may outlive a single run.
- **Borrower lease:** a per-run usage lease for a reused service instance. Reference count is derived from live borrower leases, not stored as independent mutable truth.
- **Placement:** logical (template) roots, relative layouts, registry keys, and candidate port windows derived per `(project, env, slot)`, plus the host-materialised absolute paths and run-scoped paths the runtime composes from them.
- **State:** all mutable roots derived from project, environment, slot, service, workflow, and run identity. Owned and cleaned by the runtime.
- **Registry:** a per-slot SQLite WAL database containing durable runtime state, ordered events, reservations, leases, service state, and history — *not* the source of truth for liveness. The OS owns liveness.
- **Service:** a long-lived process with lifecycle, readiness, health, logs, state, and ownership.
- **Workflow:** a dependency graph of tasks and service phases with artifacts, summaries, cancellation, and cleanup.
- **Adapter:** language-, service-, or tool-specific implementation behind a stable, typed model/runtime contract and versioned executable ABI.
- **Secret reference:** a model-level descriptor for a secret value that is resolved at admission and injected by the runtime. Secret values are never present in compiled artifacts.
- **Capability catalog:** a generated, agent-readable index of everything the project exposes (environments, slots, services, tasks, workflows, adapters, surfaces, state policy), emitted as part of the compiled output.

## Architecture & Data Flow

```text
typed Nix modules
      │
      ▼
Nix correctness + compiler passes      (pure: resolve → validate → derive → emit)
      │                                 + build/realise closures before runtime
      ▼
compiled output  ── the ONLY seam (immutable, manifest-sealed, versioned)
      ├─ manifest.json     artifact envelope: hashes, sources, target, closures, generator
      ├─ model.json        typed semantic model (no self-hash)
      ├─ schema.json       hash-bound runtime-input schema/projection
      ├─ capabilities.json generated, agent-readable capability catalog
      ├─ docs/             generated docs from typed metadata
      └─ closures          realised dep binaries, adapter ops, helper scripts (store paths)
      │
      ▼
nixfied-runtime  (Rust engine, the single execution authority)
      ├─ manifest validator + model/schema loader
      ├─ admission (host checks, target checks, locks, reservations, closure verification)
      ├─ orchestrator / workflow runner
      ├─ service dispatcher
      ├─ readiness + health probes
      ├─ process-group owner + signal propagation
      └─ SQLite registry reconciler
      │
      ▼
SQLite registry, state roots, logs, artifacts, summaries
```

The flake apps are thin, and the runtime-facing path of the ergonomic `nixfied` CLI is thin: after any explicit compile/check/realise work, it resolves the compiled output and `exec`s the engine. Execution semantics live only in `nixfied-runtime`.

### Boundary Rules

- Nix validates typed project intent and emits `manifest.json`, `model.json`, `schema.json`, the capability catalog, generated docs, and realised store paths for runtime commands and adapter operations.
- The engine reads compiled artifacts and performs all impure execution.
- The engine never invokes Nix, imports Nix modules, shells to `nix eval`, shells to `nix-store`, `nix build`s a flake reference, realises a `.drv`, or mutates compiled artifacts.
- The ergonomic CLI may run an explicit compile/check/prepare phase before execution, but the resulting compiled-output **path, manifest hash, model hash, source identity, and target identity** must be recorded as the input to the admitted run.
- Adapter code may use shell internally, but shell output is never a semantic sidecar. Semantic state returns through typed operation results or runtime-owned observation.

## Correctness Layers

Nixfied v2 separates desired-state correctness from observed-runtime correctness across four layers.

1. **Model correctness (Nix).** Reject invalid names, broken references, incompatible adapter declarations, invalid workflow edges, missing fields, and malformed state/placement policy before runtime is possible.
2. **Closure correctness (Nix / prepare).** Reproducibly construct and realise the commands the runtime may execute — runtime/task/adapter closures, helpers, generated scripts, schemas, docs. The engine receives concrete executable paths from the manifest/model instead of discovering tooling heuristically or building on demand.
3. **Admission correctness (Rust).** Host-specific checks Nix cannot prove: manifest validation, target compatibility, already-realised closure availability, base-path resolution and writable state roots, port ownership strategy, SQLite registry acquisition, stale-lease reconciliation, ownership conflicts, env-specific preconditions. **Admission is the named moment after a manifest/model is selected and before any long-lived service starts** — the moment SEAM-1 and BUILD-1 take effect.
4. **Execution correctness (Rust).** The impure graph: process groups, signals, readiness/health, task execution, workflow cancellation, registry events, summaries, cleanup, reconciliation.

This keeps Nix central to project evolution and agent-readable codification while preventing Nix evaluation from becoming a live process supervisor.

## The Compiled Output (the seam)

One immutable, manifest-sealed directory — the entire contract between Nix/`nixfied` and Rust. Ordinary Nix store paths are treated as locators unless content-addressed derivations are explicitly required; Nixfied's own artifact identity is the manifest hash tree.

Required components: `manifest.json`, `model.json`, `schema.json`, `capabilities.json`, realised closures, and `docs/` (generated). All are produced before runtime; none are mutated by the runtime.

### `manifest.json`

The artifact envelope. It is validated before the engine trusts or deserializes `model.json`.

Required top-level fields:

- `manifestVersion` — integer, initially `1`.
- `hashAlgorithm` — fixed for the manifest version, initially `sha256`.
- `compiledOutputHash` — Merkle/tree hash over the sealed payload directory, excluding `manifest.json` fields that would make the hash self-referential.
- `modelHash` — hash of the exact canonical `model.json` bytes. `model.json` does **not** contain this field.
- `schemaHash`, `capabilitiesHash`, `docsHash` — hashes of the emitted projections.
- `closureSetHash` — hash over closure metadata, including realised out paths, executable paths, operation bindings, target system, and optional NAR/hash metadata when available.
- `sourceSetHash` — hash over the source/codebase identities admitted by the compiled model.
- `target` — Nix `system`, Rust target triple, OS family, architecture, ABI/libc where relevant, closure system, and required runtime capabilities.
- `generator` — Nixfied compiler version, model-spec version, git/release identity where available.
- `runtime` — minimum compatible runtime version and feature requirements. Gives bidirectional version negotiation (model ⇄ engine), not just one-directional.

### `model.json`

Versioned, serde-typed, owned by the shared `nixfied-model` crate. Required top-level fields:

- `schemaVersion` — integer, initially `1`. The engine refuses an unknown major version.
- `project` — stable project metadata and `projectId`.
- `target` — target identity expected by the model; must match the manifest and runtime host capability checks.
- `codebases` — first-class source identities and admission policies. Runtime operations observe source only through declared `codebaseId` plus relative paths, never implicit cwd.
- `environments` — named environment definitions.
- `slotPolicy` — slot index range, defaults, and placement-relevant slot policy.
- `services` — declared service models, including service address inputs, service spec hash inputs, service lifetime policy, endpoints, containment requirements, and state epoch.
- `tasks` — declared bounded task models (a task is first-class, not only a workflow node), each declaring the codebases and secrets it may observe.
- `workflows` — declared workflow graphs.
- `adapters` — adapter metadata, ABI version, operation bindings, input/result schemas, and compatibility rules.
- `surfaces` — public command surfaces derived from the model, with canonical name, aliases, input schema, output schema, exit classes, evaluation permission, and maturity.
- `placement` — placement policy plus the *logical* run-independent roots, layouts, and candidate port windows (see [Identity & Placement](#identity--placement)). No host-absolute paths.
- `state` — state policy, ownership markers, cleanup policy, persistence, retention, and explicit purge rules.
- `secrets` — secret descriptors and injection targets only. Secret values are never serialized into artifacts.
- `runtimeConstraints` — authoritative runtime-input constraints for slots, env names, port override bounds, collision policy, source dirty policy, and other dynamic inputs.
- `closures` — realised store paths for runtime-dispatched tasks, adapters, and helper operations.

### `schema.json`

The generated projection of **runtime input** validation rules for agents, docs, and runtime enforcement. The authoritative constraints live in `model.json` under `runtimeConstraints`; `schema.json` is hash-bound in `manifest.json` and must embed the `modelHash` it was generated from. The engine rejects a schema whose manifest hash or embedded model hash does not match the admitted model.

Long term, cross-language model/schema definitions should come from one versioned model specification or IDL that generates Rust serde types, Nix option metadata, runtime constraint data, `schema.json`, `capabilities.json`, and docs. Until then, parity tests are mandatory.

### `capabilities.json` and `docs/`

The generated, agent-readable catalog of project capabilities and the human-readable docs derived from the same typed metadata. Discoverability is a product property (see [Definition of Done](#definition-of-done-per-concept)), so these are part of the Milestone 0 contract, not optional extras.

### Realised closures

The dependency binaries, adapter operations, and helper scripts the engine executes, referenced by concrete store path. They are evaluated, built, and realised before `nixfied-runtime` is invoked. At admission the engine verifies that each required path exists, is executable when needed, is recorded in the manifest closure set, and is compatible with the admitted target. The runtime never realises `.drv`s.

### Canonicalization & hashing

`manifest.json` is the cross-language contract between Nix/`nixfied` (producer) and Rust (validator). The canonical form is fixed and both sides obey it:

- UTF-8, Unicode **NFC** normalization.
- Object keys sorted lexicographically by Unicode code point.
- Arrays in declared (semantic) order — order is meaningful and preserved.
- Numbers in a single canonical form; no insignificant whitespace.
- `null`/default fields omitted explicitly rather than emitted.
- No digest field is contained in the byte sequence it hashes.

> **Nix emits the canonical bytes; the engine hashes raw on-disk bytes and never re-serializes to validate.** Re-serialization in Rust would create a second canonicalizer that can disagree with Nix's. The runtime validates manifest entries against exact artifact bytes, then deserializes only after integrity checks pass.

> Model-artifact rules: compiled output is immutable for a run; the engine receives the manifest path explicitly (e.g. `nixfied-runtime run --manifest ./result/manifest.json --env dev --slot 0`); the ergonomic wrapper may produce the compiled output first (e.g. `nixfied up dev`) but the admitted run is still tied to one concrete manifest path, `modelHash`, `sourceSetHash`, `target`, and `compiledOutputHash`.

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
- Shallow `nixfied check` / `nixfied validate` (no Nix) checks the already-compiled output, manifest integrity, target compatibility, source admission policy, and live filesystem using the manifest/model/schema — the fast, hot-path-safe check.
- `nixfied up` compiles and realises by default when the manifest/model/source identity is stale, then delegates. It always reports which compiled-output path, `compiledOutputHash`, `modelHash`, `sourceSetHash`, and `target` it admitted and whether it rebuilt; `--no-compile` admits the existing compiled output without recompiling.
- Runtime operations that observe source must name a `codebaseId` and relative path declared in the model. No runtime behavior depends on implicit cwd.

## Identity & Placement

Every runtime action is scoped by explicit identity:

```text
projectId / environment / slot / runId
```

- `projectId` — stable, explicit identifier from the model.
- `environment` — model-defined environment name.
- `slot` — user-selected deterministic parallel-instance index (`0..N`, default `0`).
- `runId` — runtime-created unique ID per invocation. Use a **ULID**: lexicographically sortable by creation time, encodes the timestamp, and is contention-safe.
- `sourceSetHash` — manifest identity for the source/codebase fingerprints the compiled output was built to admit.
- `target` — manifest identity for the platform and runtime capability set the compiled output can execute on.

### Source/codebase identity

Every source tree the runtime may observe is declared in `model.json`:

- `codebaseId` — stable logical identifier.
- `logicalRoot` — project-relative or flake-input-relative source root.
- `sourceMode` — `snapshot`, `flake-input`, or `live-workspace`.
- `sourceIdentity` — rev/NAR hash/lock identity when immutable, or a fingerprint policy for live workspaces.
- `dirtyPolicy` — `allow`, `warn`, or `reject`.
- `admissionFingerprintPolicy` — which files/metadata are checked before a run is admitted.

> **Invariant SOURCE-1:** Every task, service, workflow, and adapter operation declares the `codebaseId`s it may observe. Every admitted run records the source fingerprints used. CI/release environments default to rejecting source mismatch; dev environments may allow dirty workspaces only by explicit policy.

This prevents a compiled model from silently running against a different checkout than the one it describes.

### Service identity and compatibility

Service identity is split into stable address and compatibility identity:

```text
serviceAddress    = hash(projectId, environment, slot, serviceName)
serviceSpecHash   = hash(service-affecting fields)
serviceInstanceId = hash(serviceAddress, serviceSpecHash, logicalPortSet, target)
```

`serviceSpecHash` covers adapter ABI/version, closure paths, command, args, environment variable shape, endpoint policy, readiness and health definitions, state epoch, cleanup policy, containment requirements, target identity, and required runtime features. It intentionally does **not** use the whole `modelHash`, because unrelated model changes should not force service replacement.

> **Invariant SVC-ID-1:** A running service may be reused only when `serviceSpecHash`, logical port set, target identity, and adapter compatibility rules match the requesting model. Otherwise the runtime refuses reuse and either starts a compatible instance or reports an incompatibility.

### Placement derivation: logical (baked) vs materialised (admission)

Placement is split by phase so derivation logic exists in exactly one place per phase and no host-specific path is ever baked into manifest/model identity:

- **Logical placement is derived by Nix and baked into `model.json`** (`placement` field): template roots, relative layouts, registry keys per `(project, env, slot)`, and candidate port windows. This is machine-independent and byte-for-byte reproducible — it is what the determinism guarantee covers, and it is discoverable directly from the artifact.
- **Host-absolute placement is materialised by the engine at admission.** The engine resolves a host base — `$NIXFIED_STATE_DIR`, else a documented platform default — and joins the logical template onto it. Absolute paths depend on the host and therefore are not part of `modelHash` or `compiledOutputHash`.
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

### Port reservation and ownership proof (runtime)

Pure derivation is insufficient for ports, and so is a registry reservation: the registry coordinates Nixfied processes only — it is **not** an OS reservation, so a port that is free at preflight can be taken by an unrelated host process before the service binds. Reservation therefore reflects *intent*; ownership is proven only after the owned process binds.

> **Invariant PORT-1: No service is considered *ready* without verified ownership of its port.**
> **Invariant PORT-2: Ready requires proof that the listener belongs to the recorded service process identity or containment domain; unverifiable ownership fails readiness.**

1. Take the baked candidate window for `(project, env, slot)`.
2. Under the slot registry lock, record a reservation *intent* against `serviceInstanceId`/`runId` and pick the next candidate port in the window.
3. Start the owned service with the selected endpoint (`protocol`, address policy, port).
4. **Readiness verifies that the expected owned process actually bound the expected endpoint** (probe + owner check), not merely that the port is open. Only then is the service `ready`.
5. On bind conflict, record a collision event and retry per the model's **collision policy** — `fail`, `probe-in-range`, or `request-override`.
6. For adapters that support socket activation, the runtime may hold the listening socket and pass the file descriptor to the service. This is the only mechanism by which "reserved" means OS-reserved; it closes the check-to-bind gap entirely.
7. Without socket activation, the runtime uses platform socket-owner inspection and matches the listener to the tracked process identity or containment domain. Open-port probes are health signals only.
8. Release on normal stop, cleanup, or reconciliation after proving the owner is gone.

## The Registry (reconciled projection)

The registry is a durable record, not a liveness oracle. The OS owns liveness.

- **Storage:** one SQLite WAL database per `(projectId, env, slot)` from Milestone 0. Tables track ordered events, runs, services, reservations, leases, ports, processes, state roots, summaries, and cleanup records. NDJSON may be generated as audit/export, but is not the source of shared mutable truth.
- **Ordering:** every state-mutating event has a total per-slot sequence (`slotSeq INTEGER PRIMARY KEY` or equivalent). Timestamps are evidence only, never ordering truth.
- **Transactions:** reservation, lease acquisition/release, service state transitions, process registration, and cleanup intent/commit happen transactionally.
- **Durability & integrity:** SQLite WAL provides crash recovery and atomic commits. Registry schema version is explicit; migrations are versioned. Corrupt or incompatible registries fail with typed errors rather than silently rebuilding runtime truth.
- **Ownership keys:** reservations and leases attach to `serviceInstanceId`, not `runId` alone, so a reused service has one unambiguous owner record across runs.
- **Process identity:** every record carries enough to survive PID reuse — `pid`, process-group id, start-time (or platform-equivalent), optional `pidfd`/stable handle, command metadata, and the parent `runId`. Never `pid` alone.
- **Reconciliation:** `ps` does not trust the log; it reconciles each record against the OS before reporting `running`, `stopped`, `stale`, `canceled`, or `orphaned`.
- **Leases & GC:** every active invocation holds a `runLease`. Long-lived services hold `serviceLease`/claim records according to lifetime policy. Reuse is represented by `borrowerLease`s. `clean`/GC (and an opportunistic sweep on each invocation) detects expired leases, reaps only policy-eligible owned process groups, releases ports, and writes terminal events.

### Concurrency model

Parallel slots are a first-class product feature, not an implementation detail.

> **Invariant REG-1: One transactional per-slot registry owns shared mutable runtime state.** Reservation, lease acquisition, process registration, service state, and cleanup transitions are SQLite transactions. Slot roots and run roots are disjoint by construction, so concurrent slots cannot corrupt shared state. Readers tolerate partially completed runs; cleanup is idempotent; stale leases are recoverable. The design must not assume a remote database, daemon, or multi-host coordinator. Supported local filesystems and SQLite locking assumptions are documented.

> **Invariant REG-ORDER-1:** Every state-mutating event has a total per-slot order. Wall-clock timestamps are diagnostic evidence only.

### Lease and service lifetime policy

No daemon is required in the initial runtime, so lease semantics must not depend on an always-running supervisor.

- `run-scoped` — service is stopped when the owning run exits or is canceled.
- `until-idle` — service may outlive a run while live borrower leases exist; GC stops it after idle policy is satisfied.
- `persistent-until-down` — service persists after the run exits and is stopped only by explicit `down`/`clean` with matching scope and policy.

Reference count is derived from active borrower leases. It is not an independently mutated counter.

> **Invariant LEASE-1:** GC may reap a service only after taking the instance lock and proving: the process identity is owned, no active borrower lease remains, no active service lease forbids stop, and cleanup policy permits it.

### State cleanup safety

Every runtime-owned state root contains an ownership marker, `.nixfied-state.json`, with project, environment, slot, optional service/workflow identity, state kind, state epoch, cleanup policy, and owning manifest/model identity.

Cleanup is path-confined and marker-gated:

- canonicalize before deletion;
- refuse paths outside the resolved state base;
- refuse symlink traversal and path escapes;
- refuse unmarked roots;
- refuse marker identity mismatch;
- refuse active leases, live process refs, and active port reservations;
- require explicit purge for policy-protected persistent state;
- write cleanup intent and terminal cleanup event transactionally.

> **Invariant GC-2:** Cleanup is path-confined, marker-gated, lease-gated, process-gated, and policy-gated.

## Services and Workflows

Services are long-lived processes; workflows are bounded execution graphs.

**Service lifecycle phases** (also the adapter operation-class enum) — the universal six; `migrate`/`reset` are adapter-specific extensions, not core classes:

```
Prepare · Start · Ready · Health · Stop · Clean
```

**Workflow node classes**: task · service requirement · readiness gate · artifact collection · cleanup action.

A workflow run must know which services it requires; whether each is owned, reused, or forbidden; readiness criteria before dependent nodes run; cancellation behavior; cleanup policy; and artifact/summary locations.

**Service reuse and instance identity.** When a workflow reuses a service rather than owning it, the service is addressed by `serviceInstanceId` (not the reusing `runId`). A reused instance carries a service lease/claim and borrower leases. A run that borrows an instance records a borrower lease and does not stop it on exit; the instance is torn down according to lifetime policy after leases are gone. This keeps logging, port ownership, cancellation, and cleanup attributable to one compatible instance. Full lease handoff is specified at the Workflow-graphs milestone; the identity and ownership rule are fixed now.

> **Invariant PROC-3: No workflow node may depend on an untracked service process,** and a long-lived process is "started" only once it has a registry record.

### Process containment capabilities

Containment is a runtime capability, not a universal promise.

- M0 services must run in the foreground under the runtime-owned process group.
- Linux strict containment may require cgroup v2 + `pidfd` + process group + subreaper where available.
- macOS basic containment is process-group based and explicitly weaker.
- Daemonization/double-fork/`setsid` without stable handoff is rejected as `PROC_ESCAPE` or refused at admission.

> **Invariant PROC-CAP-1:** Admission fails if a service or adapter requires stronger containment than the host runtime can provide.

## Adapter Strategy

Core stays adapter-free. Core owns only the typed adapter contract and versioned executable ABI.

- service model shape
- lifecycle phases and the **closed enum of operation classes** above
- adapter ABI version (`nixfied-adapter-abi/v1` initially)
- operation input schema and operation result schema
- registry and ownership expectations
- operation timeout, cancellation, environment, secret, state-root, and effects policy
- **foreground-execution requirement** — a service must run in the foreground under the runtime's process group. An adapter may daemonize or fork a long-lived child **only** if it returns a stable child identity (pid + start-time, or a pidfd/handle) that the runtime can record and reconcile and the host containment capability is sufficient. Otherwise process-group ownership is not enough to defeat double-fork/`setsid` escape (see PROC-1).
- **runtime-owned start** — adapter `Prepare` returns config and service `Start` returns an `ExecSpec`; the runtime owns spawning, process group, environment injection, log capture, and cancellation. Adapters may not become hidden supervisors.
- validation rules
- generated docs/schema from typed metadata (operation classes, env vars, ports, lifecycle support, readiness/health behavior, state roots, examples)

Concrete operation bindings compile to executable closure paths and arguments, but the semantic operation class is always typed. Operations receive a typed `OperationInvocation` as JSON on stdin and emit exactly one typed `OperationResult` JSON envelope on stdout. Free-form stdout is not semantic; logs go to stderr or runtime-owned log files. Stable exit codes map to the runtime error contract. String dispatch such as `svc::<service>::<op>` may exist only as a generated compatibility ABI, never as the primary model.

Concrete adapters live under `adapters/` and are opt-in. A minimal Nixfied project must not evaluate postgres, nginx, minio, reth, helios, or any concrete adapter by default — and this laziness is proved by a test, not assumed.

Postgres is the canonical reference adapter — it exercises ports, state, lifecycle, readiness, health, workflows, persistence, and cleanup. Others stay smaller until the core contract is stable.

> **Invariant ADAPTER-ABI-1:** stdout is reserved for semantic ABI JSON. Free-form output is non-semantic log data only.

> **Invariant ADAPTER-START-1:** Adapters may not become hidden supervisors. Runtime-owned process spawning is the default; daemon handoff is exceptional and capability-gated.

## Secrets Boundary

Secrets/credentials management is deferred, but secret **non-leakage** is not.

- Compiled artifacts may contain secret descriptors (`SecretRef`, env var name, file injection target, required/optional, source policy), never secret values.
- Secret values are resolved by the runtime at admission and injected only through approved channels.
- Secret values are forbidden in argv, store paths, docs, capabilities, registry events, summaries, operation results, and persisted error payloads.
- Logs, errors, adapter results, registry writes, and summaries are redacted before persistence.

> **Invariant SECRET-1:** Model artifacts may contain secret references, never secret values.

> **Invariant REDACT-1:** Persistent runtime output is redacted before write.

## Public Surfaces

Derived from the execution model, not a command wishlist (the `surfaces` field of `model.json` is the source). Two command layers; friendlier verbs mapped to single responsibilities. Every stable surface declares canonical name, aliases, input schema, output schema, exit classes, evaluation permission, and maturity level.

**`nixfied` (ergonomic, may compile/realise before delegation):** `compile`, `realise`, `check`, `validate [--deep]`, `model`, `schema`, `docs`, `up`, `down`, `run`, `ps`, `logs`, `clean`, `install`, `upgrade`.

**`nixfied-runtime` (pure engine, never invokes Nix):** `model` (inspect compiled model), `schema` (expose compiled schema), `docs` (expose generated docs), `check` (manifest/model + host admission assumptions), `run` (workflow/task), `up` (start required services), `down` (stop owned services), `ps` (reconcile + list observed state), `logs`, `clean` (reconcile + reap stale state/leases/processes).

`model` is the canonical discoverability command. `introspect` may exist as an alias, but it is not a separate normative surface. `svc` may exist only as a generated compatibility alias if the model exposes it; it is not core.

`schema` and `docs` are first-class because discoverability is a product property: an agent or human must be able to read the schema and generated docs from a compiled output without source access.

> **Invariant SURFACE-1:** A public command is stable only when its schemas, exit classes, aliases, maturity, and evaluation permission are declared in `model.surfaces`.

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What compiler pass validates it?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those five, it is not core. This is the contributor rule, not a one-time review.

## Runtime Invariants (the laws the codebase must hold)

- **SEAM-1:** `nixfied-runtime` never invokes Nix. Evaluation is confined to `compile` and `validate --deep`; build/realisation is confined to `compile`/`realise`/prepare surfaces before runtime invocation.
- **BUILD-1:** The runtime admits only concrete, already-realised store paths; it never realises `.drv`s, never calls `nix-store`, and never runs `nix build`.
- **MANIFEST-1:** The runtime never executes without a validated `manifest.json` whose artifact hashes match exact on-disk bytes and whose target/source identities are admitted.
- **HASH-1:** No digest field is contained in the byte sequence it hashes.
- **CONTENT-1:** Content identity is a verified manifest property unless Nix CA derivations are explicitly required; a Nix store path alone is not treated as semantic content identity.
- **MODEL-1:** The runtime never executes without a validated `model.json` whose raw bytes match `manifest.modelHash`.
- **SCHEMA-1:** Cross-language model/runtime constraints have one source of truth; generated artifacts must be hash-bound and parity-checked.
- **ADMIT-1:** Every admitted run records the exact compiled-output path, manifest path, `compiledOutputHash`, `model.json` path, `schemaVersion`, `modelHash`, `sourceSetHash`, and `target` it used.
- **SOURCE-1:** Every runtime operation that observes source declares `codebaseId`; admitted runs record source fingerprints and enforce dirty/source mismatch policy.
- **SYSTEM-1:** Artifact admission is scoped to compatible target identity; executable artifacts are system-specific.
- **SVC-ID-1:** Service reuse is allowed only when `serviceSpecHash`, logical port set, target identity, and adapter compatibility rules match.
- **PORT-1:** No service is considered ready without verified ownership of its port; a registry reservation is intent, not an OS reservation.
- **PORT-2:** Ready requires proof that the listener belongs to the recorded service process identity or containment domain; unverifiable ownership fails readiness.
- **PLACE-1:** Logical placement is baked by Nix (machine-independent, hashed into the model); host-absolute paths are materialised at admission from a resolved base; run-scoped paths are composed at runtime. Derivation for each phase lives in exactly one place.
- **REG-1:** One transactional per-slot SQLite registry owns shared mutable runtime state.
- **REG-ORDER-1:** Every state-mutating event has a total per-slot order; timestamps are evidence only.
- **LEASE-1:** GC may reap a service only after proving ownership, no active borrower lease, no service lease forbidding stop, and cleanup policy permission.
- **PROC-1:** Every spawned process belongs to a runtime-owned process group, and all M0 services run in the foreground. Stronger containment is capability-gated per platform.
- **PROC-2:** Cancellation propagates to the whole process group.
- **PROC-3:** Every long-lived process has a registry record before it is considered started; no node depends on an untracked process.
- **PROC-CAP-1:** Admission fails if a service requires stronger containment than the host runtime supports.
- **LIVE-1:** Liveness is always reconciled against the OS before being reported; the registry is never trusted as a liveness oracle.
- **GC-1:** Cleanup is idempotent and safe to re-run after a crash; GC never deletes policy-protected persistent state.
- **GC-2:** Cleanup is path-confined, marker-gated, lease-gated, process-gated, and policy-gated.
- **ADAPTER-ABI-1:** Adapter stdout is reserved for semantic ABI JSON; free-form output is log data only.
- **ADAPTER-START-1:** Adapters may not become hidden supervisors; runtime-owned spawning is the default.
- **SECRET-1:** Model artifacts may contain secret references, never secret values.
- **REDACT-1:** Persistent runtime output is redacted before write.
- **SURFACE-1:** Stable public commands have schemas, exit classes, aliases, maturity, and evaluation permission declared in `model.surfaces`.
- **UPGRADE-1:** Install/upgrade may update Nixfied-owned pins/import shims only; it never overwrites user-owned model declarations and fails before mutation on incompatible schema/runtime pairing.
- **SHELL-1:** Shell cannot own graph, registry, summary, validation, liveness, or cleanup semantics.
- **NIX-1:** Nix cannot own live process supervision, liveness reporting, cancellation, registry mutation, or cleanup.

These exist so the rebuild does not silently regrow the v1 failure mode.

## Runtime Error Contract

The product targets agents as first-class callers, so runtime errors are part of the contract, not just log text. The engine emits **stable, machine-readable error categories**, each with a stable code, a typed payload, and an exit class. Codes are versioned with `schemaVersion`; new categories are additive.

- `MANIFEST_MISMATCH` — manifest hash tree mismatch / tampered compiled output.
- `MODEL_MISMATCH` — `modelHash` mismatch / tampered or truncated model artifact.
- `SCHEMA_UNSUPPORTED` — unknown/future `schemaVersion`, or a model whose `runtime` requirements this engine cannot satisfy.
- `SCHEMA_VIOLATION` — a runtime input fails `schema.json` (bad slot index, out-of-bounds port override, …).
- `SOURCE_MISMATCH` — admitted source fingerprint does not satisfy the model's source/dirty policy.
- `PLATFORM_UNSUPPORTED` — manifest/model target identity or required runtime capability does not match this host/runtime.
- `CLOSURE_MISSING` — a realised store path is unavailable, not executable when required, or not hash-bound in the manifest.
- `PORT_CONFLICT` — no port in the window could be owned within the collision policy.
- `PORT_UNVERIFIABLE` — a listener exists but ownership cannot be matched to the service process identity or containment domain.
- `STATE_UNWRITABLE` — the resolved state base/root cannot be created or written.
- `STATE_UNOWNED` — cleanup target lacks a matching `.nixfied-state.json` ownership marker or escapes the state base.
- `LEASE_STALE` / `LEASE_CONFLICT` — a lease expired, or is contended by another live owner.
- `PROC_ESCAPE` — a spawned process could not be tracked or reconciled (escaped the group).
- `READINESS_TIMEOUT` — a service did not reach `ready` within policy.
- `SECRET_UNAVAILABLE` — a required secret reference could not be resolved at admission.
- `SECRET_LEAK_BLOCKED` — a secret value was detected in a payload that would be persisted.
- `ADAPTER_PROTOCOL` — adapter ABI input/output/exit behavior violated the versioned protocol.
- `CANCELED` — the run was canceled.
- `CLEANUP_REFUSED` — GC declined to delete policy-protected persistent state.
- `REGISTRY_CORRUPT` — SQLite registry failed an integrity/schema check.

## Milestones

Ordering hardens failure and lifecycle semantics **before** the first real stateful adapter, so adapter complexity cannot accumulate on top of unproven cancellation/GC (the documented v1 failure mode).

### Milestone 0 gates — must be decided before implementation starts

M0 implementation must not start until these architecture choices are frozen:

1. `manifest.json` shape, hash algorithm, canonicalization, and `compiledOutputHash` tree rules.
2. One source of truth for model/runtime constraints and how `schema.json` is generated/bound.
3. Runtime closure mode: M0 runtime is Nix-free and admits only already-realised store paths.
4. Per-slot SQLite WAL registry schema and total event ordering.
5. M0 process identity and containment matrix for Linux and macOS.
6. Source/codebase identity and dirty/source mismatch admission policy.
7. Target identity fields and runtime capability negotiation.
8. Adapter ABI v1 envelope, even if no concrete adapter ships in M0.
9. Secrets non-leakage and redaction contract.
10. State ownership marker and cleanup refusal rules.

### Milestone 0 — Walking skeleton (thin slice through every layer)

Smallest end-to-end vertical slice; exercises the whole spine and nothing more:

- one typed Nix project module → one `compile` path producing `manifest.json`, `model.json`, `schema.json`, `capabilities.json`, minimal `docs/`, and realised closures
- `nixfied-runtime` loads and validates the manifest, model, schema, target identity, source policy, and realised closure paths
- one env (`dev`), one slot (`0`), one task, one service with `Start`/`Ready`/`Stop`
- one per-slot SQLite registry, one port reservation with ownership-verified readiness, one state root with `.nixfied-state.json`, one log root, one summary file
- one synthetic foreground lifecycle fixture service, not Postgres and not a concrete adapter
- `ps` reconciles and reports observed state; `model`, `schema`, and `docs` read the compiled output
- runtime works with `nix` unavailable when all closures are already realised

Complete when a downstream-shaped minimal example can: build the compiled output; validate `manifest.json`; start a foreground service; verify port ownership at readiness; run a dependent task; write SQLite registry events and a summary; stop the owned service; record the admitted compiled-output path, manifest path, `compiledOutputHash`, model path, `schemaVersion`, `modelHash`, `sourceSetHash`, and `target`; clean only marker-owned state; and report no live owned processes after reconciliation. No concrete adapters, parallel slots, workflow graphs, reuse, install/upgrade, or crash-hardening GC yet.

### Subsequent milestones

1. **Slot isolation** — two concurrent slots of the same env: disjoint ports, state, logs, SQLite registries; independent reconciliation.
2. **Cancellation & GC hardening** — signal propagation, lease expiry, `kill -9` survival and reconciliation, stale-reservation cleanup, marker-gated cleanup, persistent-state refusal. Crash-safety is proven before any stateful adapter exists.
3. **Reference adapter (minimal Postgres)** — start → readiness (port ownership verified) → simple client task → artifacts → clean stop, with state preserved/deleted per policy. No full workflow complexity yet.
4. **Workflow graphs** — dependency-aware workflows with bounded tasks, service requirements (incl. reuse via `serviceInstanceId` leases), readiness gates, cancellation, artifacts, summaries; cancellation propagates and leaves explanatory evidence.
5. **Installable downstream wrapper** — adoption/upgrade without vendoring internals or losing project-owned config; `schemaVersion`/`runtime` gating.
6. **Polyglot example** — `examples/polyglot-stack`.

## Proof Strategy (tiered, to avoid a proof monolith)

Layered so no single workspace becomes the only evidence:

- Nix correctness/compiler proofs — model validation, closure construction, canonical JSON, and stable raw-byte artifact emission.
- Manifest/compiler proofs — hash tree validation, no self-hash, target/source identity binding, schema/capabilities/docs hash binding.
- Runtime unit proofs — placement composition, port reservation, SQLite registry events, process identity, reconciliation, lease policy, marker-gated GC.
- Adapter-contract proofs without concrete services; adapter-specific proofs in isolation; a proof that a minimal project does **not** evaluate any concrete adapter.
- Downstream-shaped examples exercising public APIs only.

### Adversarial / negative proofs (required)

Failure-path coverage is explicit because the product's thesis is owned, reconciled, cleanable execution:

- tampered/truncated `manifest.json`, `model.json`, `schema.json`, `capabilities.json`, or docs are rejected at the boundary;
- unknown future `schemaVersion` is refused;
- target mismatch is refused (`PLATFORM_UNSUPPORTED`);
- source mismatch / dirty-policy violation is refused (`SOURCE_MISMATCH`);
- `nixfied-runtime` with `nix` unavailable still runs an already-realised compiled output (no runtime Nix);
- byte-identical artifact hashes between Nix-emitted bytes and the engine's on-disk hash;
- port conflict at *bind* time (not only preflight), and ownership-verified readiness;
- open port owned by an unrelated process fails readiness (`PORT_UNVERIFIABLE`/`PORT_CONFLICT`);
- service daemonization/double-fork attempt is detected (`PROC_ESCAPE`) or contained;
- PID-reuse simulation and a stale lease whose PID is reused by an unrelated process;
- SQLite registry corruption/schema mismatch fails typed checks; transaction rollback preserves invariants after interrupted writes;
- two slots racing on registry and port allocation;
- GC refusing to delete policy-protected persistent state;
- cleanup refusing unmarked roots, symlink escapes, path traversal, and marker mismatch;
- secret values are rejected or redacted before registry/log/summary persistence;
- adapter stdout that is not valid ABI JSON fails as `ADAPTER_PROTOCOL`.

Initial examples (downstream-shaped workspaces with their own `flake.nix`, framework import, `README.md`, operating notes — not framework-owned fixtures):

- `examples/minimal`
- `examples/postgres-api`
- `examples/web-nginx`
- `examples/object-storage`
- `examples/polyglot-stack`

> Risk noted from v1: a single giant proof workspace becomes a second framework snapshot. The tiered split is deliberate; examples prove integration, not internals.

## Definition of Done (per concept)

- **Manifest:** a project compiles typed intent into a manifest-sealed compiled output; the engine rejects invalid, unsupported, mismatched-target, mismatched-source, or tampered artifacts before execution.
- **Model:** a project compiles typed intent into canonical `model.json`; the engine rejects invalid, unsupported, or tampered (`manifest.modelHash` mismatch) artifacts before execution.
- **Discoverability:** a human or agent can run `model`, `schema`, and `docs` against a compiled output and discover available environments, slots, services, tasks, workflows, adapters, state policy, source policy, target identity, runtime capabilities, and supported surfaces without reading project source.
- **Source identity:** tasks/services/workflows declare `codebaseId`s; admitted runs record source fingerprints and enforce dirty/source mismatch policy.
- **Target identity:** runtime admission refuses compiled outputs whose target identity or required runtime capabilities do not match the host/runtime.
- **Environment:** `dev`, `test`, `ci` express different services, workflows, state and cleanup policy through the same typed model, with disjoint state roots and no cross-env leakage.
- **Slot:** two slots of the same env run concurrently with disjoint placement, ports, state, logs, artifacts, SQLite registries, and summaries; `kill -9` of one leaves the other intact and reconciling within one `ps`.
- **State:** roots are derived, inspectable, policy-controlled, marker-owned, path-confined, and safe to clean without guessing which process owns them.
- **Registry:** the runtime reports live/stopped/stale/canceled/orphaned via reconciliation, not by trusting stale files; `clean` reaps eligible orphans and releases their ports using transactional lease/cleanup records.
- **Service:** start → ready → health → stop owned end-to-end, with a port whose ownership is verified at readiness and a process group that fully tears down, and reconciliation after abnormal exit.
- **Service reuse:** reuse is gated by `serviceSpecHash`, logical port set, target identity, adapter compatibility, and live lease state.
- **Workflow:** expresses service requirements (incl. reuse), task dependencies, cancellation, artifact collection, and cleanup policy, then produces a durable summary; cancellation mid-run leaves no policy-forbidden orphans.
- **Adapter:** implements typed operation classes behind `nixfied-adapter-abi/v1` without becoming framework core or hidden supervisor; the compiler rejects an adapter missing a declared class.
- **Secrets:** compiled artifacts contain secret refs only; runtime persistent output is redacted before write.
- **Install & Upgrade:** a downstream project can install or upgrade Nixfied without vendoring framework internals or overwriting project-owned configuration; `schemaVersion`/`runtime` gating refuses an incompatible pairing with a typed error.

## Repository Layout (greenfield)

```
flake.nix                  # apps, devShell, packages, compiled-output builder
nix/
  modules/                 # typed module options (core, env, slot, service, task, workflow, state)
  compiler/                # pure passes: resolve → validate → derive → emit + build/realise closures
  spec/                    # versioned model/schema/adapter ABI definitions and generators
  lib/                     # pure helpers; logical placement derivation baked into model.json
runtime/                   # Rust workspace
  Cargo.toml
  crates/
    nixfied-model/         # serde types for model.json, manifest, schemas; the shared contract
    nixfied-adapter-abi/   # adapter invocation/result protocol
    nixfied-runtime/       # pure execution engine: admission, supervisor, SQLite registry, reconciliation
    nixfied-cli/           # ergonomic `nixfied` front-end (may compile, then delegate)
adapters/                  # opt-in service adapters (postgres first); NOT evaluated by default
examples/                  # downstream-shaped proof workspaces
docs/
```

## Install & Upgrade Ownership

Install and upgrade are adoption surfaces, not model mutation tools.

- `install` may add a pinned flake input, import shim, wrapper app, or generated Nixfied-owned file under an explicit ownership marker.
- `upgrade` may update Nixfied-owned pins/import shims and lock metadata.
- Neither command overwrites project-owned model declarations, service definitions, source declarations, workflows, secrets policy, or state policy.
- A previous compiled output remains runnable while a new Nixfied version is being adopted, subject to runtime compatibility gates.
- On incompatible schema/runtime pairing, upgrade fails before mutation and reports a typed error.

> **Invariant UPGRADE-1:** Upgrade never overwrites user-owned model declarations and fails before mutation on incompatible schema/runtime pairing.

## Non-Goals

- No compatibility with the current repository layout; no migration layer for v1 commands.
- No broad downstream project template as framework core.
- No required service adapter for a minimal project.
- No daemon requirement for the initial runtime; no remote or multi-host execution.
- No duplicate command families for the same lifecycle action.
- No shell-owned semantic sidecars for graph, registry, summary, validation, or cleanup behavior.
- No Nix invocation at all from `nixfied-runtime`; no evaluation, build, `.drv` realisation, or `nix-store` calls during runtime admission, execution, supervision, cancellation, registry mutation, reconciliation, or cleanup.
- No host-absolute paths baked into the manifest/model hash identity.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Deferred (explicit non-goals *for now*, to keep them from leaking back in)

Real and intentionally out of v2's first cut; listed so they are not designed in by accident:

- **Full secrets/credentials management** — provider integrations, rotation, access control, and secret storage are deferred. The non-leakage/redaction contract is not deferred.
- **Inter-service dependency DAG** — services declare readiness; cross-service ordering beyond workflow phases is deferred.
- **Log aggregation/retention** — logs are runtime-owned files per run; centralized aggregation and modeled-log-artifacts are later.
- **Multi-host / remote execution** — registry and state are local-disk; distributed execution would require revisiting REG-1 and LIVE-1.
- **UI / dashboard surfaces.**

## Open Questions

Ranked by the gate at which each must be decided.

### Must decide before Milestone 0

1. Final `manifest.json` schema and `compiledOutputHash` tree rules.
2. Whether `model.json`, manifest, docs, and realised closures are produced only as a Nix derivation output, also as a CLI output, or both.
3. Exact model/spec source of truth: IDL/generator now vs generated Nix/Rust parity tests as an interim.
4. Exact `schema.json` representation: JSON Schema subset vs bespoke runtime constraint projection.
5. Runtime ⇄ model compatibility negotiation rules for the `runtime` field and manifest `target`.
6. SQLite registry M0 schema, migration policy, and supported filesystem assumptions.
7. Minimum cross-platform process-identity and port-owner strategy for Linux and macOS.
8. M0 adapter ABI v1 envelope and exit-code mapping.

### Before Slot Isolation / Cancellation & GC

9. Port-window base, size, and slot stride that avoid realistic host collisions without consuming too much space.
10. Default runLease TTL, serviceLease policy defaults, borrower lease expiry, and opportunistic GC cadence.
11. SQLite registry compaction/vacuum/export policy.
12. State marker schema and explicit persistent purge command shape.

### Before Adapter / Workflow

13. Final closed set of adapter operation classes beyond the core six.
14. How much service-dependency behavior belongs in the Workflow milestone vs deferred.
15. Borrower lease / service lease handoff state machine for reused service instances.
16. Adapter compatibility override rules when `serviceSpecHash` changes but reuse is safe.

### Before Install

17. `nixfied`/`nixfied-runtime` distribution — pinned in the flake only, or also standalone release artifacts.
18. Install/upgrade ownership boundaries: which files may be created/updated, how project-owned model declarations are protected, and rollback behavior after incompatible schema/runtime pairing.

## Constraints From v1

Captured so the rebuild does not repeat history:

- Runtime semantics drifted into shell helpers and Nix builders, with no single owner. Hence SEAM-1, BUILD-1, SHELL-1, NIX-1, and the two-binary split.
- Artifact identity was at risk of becoming self-referential or over-claimed as content-addressed. Hence MANIFEST-1, HASH-1, and the manifest-sealed seam.
- Live checkout state was implicit. Hence SOURCE-1 and first-class `codebases`.
- The registry was treated as the source of truth for liveness, which drifts from reality. Hence LIVE-1 and reconciliation.
- Pure port derivation (and even a registry reservation) was treated as sufficient, but the host has unrelated processes and the OS does not know the registry. Hence the ownership-verified PORT-1.
- Placement rules drifted when both Nix and runtime derived them. Hence the logical/materialised phase split and PLACE-1.
- Service reuse could blur incompatible runtime configs. Hence service address/spec-hash split and SVC-ID-1.
- Lease/refcount semantics could depend on a daemon that does not exist. Hence run/service/borrower lease split and LEASE-1.
- Process containment differs by platform. Hence PROC-CAP-1 and foreground-only M0 services.
- Adapter complexity accumulated before lifecycle ownership was proven. Hence cancellation/GC hardening precedes the reference adapter in the milestone order.
- A single large proof workspace becomes a second framework snapshot. Hence the tiered proof strategy.
- Duplicate command families accreted for the same lifecycle action. Hence the model-derived public surface and the 5-question gate.

## Success Criteria

A downstream project can define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, realised runtime closures, secret references, target requirements, and state policy in one typed Nix model; evaluate it for static correctness; compile it deterministically into a versioned, manifest-sealed compiled output; then execute that admitted artifact through one Nix-free Rust runtime with isolated placement, SQLite-backed lifecycle records, process-aware reconciliation, compatible service reuse, clean cancellation, marker-gated crash-safe GC, durable registry history, redacted persistent outputs, and reproducible summaries. The acceptance checks in [Definition of Done](#definition-of-done-per-concept) are the measurable form of this statement.

## Decision Log (resolved forks)

Fork resolutions, pulled out of the body so the spec reads as decisions, not archaeology. Each notes the resolution and corrects the provenance where earlier inline notes were inaccurate.

- **One binary vs two → two binaries.** `nixfied` (ergonomic, may compile/realise) and `nixfied-runtime` (pure engine). *Provenance correction:* both predecessor drafts centred a single binary — `RFC_v2cx` named it `nfd` with a cli/runtime *crate* split, not a binary split. The two-binary split is introduced here to make SEAM-1/BUILD-1 structural; it was not "adopted from `RFC_v2cx`" as an earlier note stated.
- **Run-ID format → ULID.** Sortable, timestamped, contention-safe. *Provenance correction:* `RFC_v2cx` already specified "ULID or equivalent"; the earlier note that it used a `YYYYMMDD-HHMMSS-<suffix>` format was inaccurate.
- **Placement derivation → phase split.** Logical placement baked by Nix; host-absolute paths materialised at admission; run-scoped paths composed at runtime. Supersedes dual Nix+Rust derivation guarded by a parity test (the prior `PARITY-1`), removing the drift class instead of testing for it.
- **Self-hashing model → manifest-sealed compiled output.** `modelHash` moves out of `model.json` into `manifest.json`; the runtime validates raw bytes against the manifest before deserializing. This resolves hash circularity and binds schema/docs/capabilities/closures/source/target together.
- **Runtime realisation → Nix-free runtime for M0.** `nixfied-runtime` admits only already-realised store paths and never invokes Nix or `nix-store`. Any future admission-realisation mode must be explicit and capability-gated.
- **Registry storage → per-slot SQLite WAL.** NDJSON is demoted to optional export/audit. SQLite owns transactional reservations, leases, events, process records, and cleanup state from M0.
- **Source identity → first-class codebases.** Runtime operations observe source only through declared `codebaseId`s and recorded source fingerprints.
- **Service identity → address + spec hash.** Reuse is gated by service compatibility, not just `(project, env, slot, serviceName)`.
- **Lease model → run/service/borrower leases.** Mutable refcount is replaced by derived borrower lease count and explicit service lifetime policy.
- **Process containment → capability-gated.** M0 requires foreground services; stronger platform containment is admitted only when supported.
- **Adapter ABI → versioned executable protocol.** Adapter semantic IO is typed JSON; runtime-owned spawning is the default.
- **Secrets → non-leakage contract now.** Secret management remains deferred, but secret values are forbidden in artifacts and redacted before persistent runtime writes.
- **Service lifecycle phases → the universal six.** `Prepare · Start · Ready · Health · Stop · Clean`; `migrate`/`reset` are adapter extensions, not core classes.
- **Public surface naming → canonical verbs + aliases.** `model` is canonical; `introspect` and `svc` may exist only as aliases if declared in `model.surfaces`. `schema`/`docs` remain first-class discoverability surfaces.
- **Milestone order → cancellation/GC before the reference adapter.** Hybrid of both drafts: skeleton → slot isolation → cancellation/GC hardening → minimal Postgres → workflow graphs → install → polyglot.
