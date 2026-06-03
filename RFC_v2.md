# RFC v2.3: Hardened Single-Model Hybrid Greenfield Nixfied Build Spec

Date: 2026-06-03
Status: Draft (selected architecture; Milestone 0 narrowed)

## Decision Summary

Nixfied v2 is a greenfield rebuild. The existing framework, proof workspaces, shell sidecars, historical command structure, and repository layout are not compatibility constraints.

The load-bearing architecture decision:

- **Nix is the user-facing integration and correctness layer.** Users integrate Nixfied by importing/extending Nix modules in their own project, with their own project shape and toolchain choices. Nix is programmable, open, typed, reproducible, and already the natural place for users to describe arbitrary environments.
- **Rust is the hidden generic execution runtime.** The runtime does not know Postgres, Node, Python, nginx, or any project-specific convention. It executes generic primitives compiled by Nix into `model.json`, while still enforcing Nixfied's typed lifecycle semantics.
- **`model.json` is the only required semantic seam.** There is no required `manifest.json`, `schema.json`, or `capabilities.json`. Schema, docs, and capabilities are model sections or generated views over the model.
- **`model.json` carries the full admission contract.** Source/codebase identity, target identity, closure metadata, generator/toolchain identity, runtime capabilities, layered service identity, state cleanup policy, and secret references are first-class model fields and are covered by `computedModelHash`.
- **`model.json` is normally a Nix store output.** The normal runtime path admits only a `model.json` that lives under `/nix/store` (or the platform's Nix store root). Store location is a local origin/trust policy, not cryptographic proof of compiler provenance; the runtime also validates the model's ABI, toolchain, generator, target, source, and closure fields. Stronger provenance requires a future non-semantic envelope or signature.
- **No cross-version compatibility is promised.** This is a greenfield system. A model is valid only for the exact `runtimeAbi` / `toolchainId` that produced it. Updating Nixfied means recompiling the model.
- **The runtime computes provenance, not self-hashes.** `model.json` does not contain a self-hash. At admission the runtime computes `computedModelHash = sha256(raw model.json bytes)` and records it in the registry, summaries, logs, and error payloads.
- **The runtime never invokes Nix.** Nix evaluation, build, and realisation happen in `nixfied compile` / `nixfied prepare` before `nixfied-runtime` starts. Runtime admission verifies already-realised store paths only.
- **Lifecycle semantics remain typed.** Concrete adapters are Nix-side model generators, but the model preserves semantic lifecycle operation classes and result contracts. Generic `ExecSpec`/`ProbeSpec` primitives are the execution mechanism, not a replacement for lifecycle meaning.
- **Mutable runtime state is owned by Rust.** Runtime state, process ownership, ports, leases, reconciliation, cleanup, logs, artifacts, and summaries are Rust responsibilities.
- **A manifest is not required for Milestone 0.** If Nixfied later needs portable compiled-output bundles, cache export/import, standalone distribution outside the Nix store, or integrity-bound materialized views, a minimal non-semantic manifest envelope may be added. It must bind artifact bytes only; `model.json` remains the semantic authority.

This is a build spec, not a migration plan.

## Problem

Modern projects are not one program. They are small systems: frontend, API, workers, databases, queues, object storage, migrations, test harnesses, infra scripts, CI jobs, and local developer workflows, often written in different languages and owned by different tools.

The operational behavior of those systems is usually scattered across `flake.nix`, shell scripts, package-manager scripts, CI YAML, Docker compose files, service wrappers, env files, port conventions, and local tribal knowledge.

The deeper problem is that projects have no clean, process-aware way to run multiple environments, or multiple copies of the same environment, while keeping services, workflows, state, logs, ports, artifacts, and cleanup under one authority.

Without first-class environment, slot, state, placement, registry, and source concepts:

- `dev`, `test`, `ci`, preview, and prod-like runs collide.
- Two copies of the same environment cannot run predictably.
- Ports and mutable state leak between runs.
- Services are started by one script and stopped by another, if they are stopped at all.
- Workflows cannot reliably know which service instance they own.
- CI failures leave weak evidence about what ran, what was canceled, and what survived.
- Polyglot codebases need too much glue to coordinate simple local and CI execution.

Nixfied v2 exists to solve that problem.

## Product Statement

Nixfied v2 is a Nix-authored project model and correctness system with a generic Rust runtime for owned execution of project-shaped systems.

It codifies project intent in Nix, evaluates that intent to typed correctness, compiles it into one canonical `model.json`, then runs that model through one Rust runtime that can start, inspect, record, reconcile, cancel, and clean up the execution graph across environments, slots, services, tasks, workflows, and codebases.

Three properties define the product, and they are not the same:

- **Codified & discoverable.** The entire project - capabilities, services, tasks, workflows, environments, constraints, state policy, source policy, and public surfaces - is expressed as typed data in `model.json`. A human or agent can discover what the project can do by reading the model or generated views of it.
- **Deterministic model & logical placement.** Given the same typed Nix input, source policy, target identity, and Nixfied toolchain, the compiled model and logical placement are reproducible. Host-absolute paths are not part of this guarantee; they are materialised by the runtime at admission.
- **Owned execution.** Runtime behavior (timing, readiness, process scheduling, external host state) is not deterministic. The runtime instead guarantees that every process is owned, tracked, attributable, reconciled against OS reality, and cleanable.

Nix is the authority for what the project is allowed to be. Rust is the authority for what the project is currently doing.

Nixfied is not primarily a task runner, service template collection, CI wrapper, Docker replacement, or Nix scaffold. Those may exist as surfaces or Nix-side libraries, but the product is the Nix-authored model plus the Rust execution runtime.

## Nix As The Open Integration Layer

Nix is not only a correctness gate. It is also the public extension API.

Users should be able to integrate Nixfied into their own projects by:

- importing a Nixfied module from a flake input;
- defining project-specific environments, services, tasks, workflows, state, ports, and source policy in Nix;
- composing Nixfied with their own flakes, packages, dev shells, overlays, generated files, package managers, and project conventions;
- using Nix-side adapter libraries that compile project-specific declarations into generic runtime primitives;
- avoiding any requirement to vendor Nixfied internals or reshape the repository around Nixfied.

The runtime is deliberately generic and hidden. If a user can describe an environment in Nix and compile it into the Nixfied model primitives, the Rust runtime should be able to execute it without knowing the domain-specific origin of that model.

> **Invariant NIX-API-1:** Nix modules are the user-facing integration API. Runtime-specific project behavior must be expressible as typed Nix data compiled into generic model primitives.

## Two Tools, One Model Seam

The split is defined by the verb, not by timing:

| Verb | Example | Owner |
|------|---------|-------|
| **Evaluate** | flake -> module options, assertions, derivations | **Nix**, at explicit authoring surfaces |
| **Build / realise** | dependency binaries, helper scripts, generated wrappers | **Nix / `nixfied`**, before runtime invocation |
| **Enforce / execute** | port ownership, process groups, readiness, leases, cleanup | **Rust**, against `model.json` and OS reality |

1. **Nix is the source of truth and the correctness gate.** Typed Nix modules evaluate into `model.json`, and invalid project intent never produces an admitted model. Nix can build/realise closures, generate helper scripts, and expose docs/schema views. Nix never starts project services, owns process state, mutates the runtime registry, or reports liveness.

2. **Rust is the runtime, and the only runtime.** `nixfied-runtime` reads `model.json`, validates it, and owns all impure runtime behavior: admission checks, process supervision, process-group ownership, readiness/health polling, registry writes, state placement, cancellation, reconciliation, and cleanup. It uses already-realised Nix store paths, but never invokes Nix.

The model seam is enforced by these rules:

> **Invariant MODEL-SEAM-1:** `model.json` is the only required semantic artifact consumed by `nixfied-runtime`.

> **Invariant MODEL-ORIGIN-1:** Normal runtime admission requires `model.json` to be under the Nix store root. This proves store immutability and local origin policy, not full compiler provenance. The runtime refuses non-store model paths unless an explicitly unstable framework-development/test flag is used.

> **Invariant MODEL-CONTRACT-1:** The model itself carries the admission contract: generator/toolchain identity, runtime ABI, target identity, runtime capability requirements, source/codebase policy, closure metadata, layered service identity, state cleanup policy, and secret descriptors. These fields are semantic and covered by `computedModelHash`.

> **Invariant HASH-1:** `model.json` does not contain a self-hash. The runtime computes `computedModelHash = sha256(raw model.json bytes)` at admission and records it as model identity/provenance.

> **Invariant SEAM-1:** `nixfied-runtime` never invokes Nix, `nix-store`, `nix build`, or `nix eval`, and never imports Nix expressions.

> **Invariant PREPARE-1:** Every referenced runtime closure is realised before `nixfied-runtime` starts. The runtime checks existence, executability, target compatibility, and declaration in the model; it never realises a closure itself.

> **Invariant ABI-1:** A model is admitted only when its `runtimeAbi` and `toolchainId` exactly match the runtime's model/executor contract. No cross-version compatibility is promised in M0.

- **`nixfied`** - the ergonomic CLI. May run Nix `compile`, `validate --deep`, and realisation/prepare surfaces, then delegate to the engine.
- **`nixfied-runtime`** - the hidden, generic execution engine. Consumes `model.json` by path and cannot invoke Nix.

## Generic Runtime Model

The Rust runtime understands generic primitives and Nixfied lifecycle semantics, not project-specific adapters.

Required model primitive classes:

- **`ExecSpec`** - executable store path, args, environment, cwd/codebase ref, stdin policy, timeout, secrets, output capture, cancellation mode.
- **`EndpointSpec`** - protocol, address policy, port source, socket activation support, ownership verification policy.
- **`ProbeSpec`** - TCP, HTTP, process, file, or exec readiness/health probe with timeout and retry policy.
- **`LifecycleOpSpec`** - semantic operation class (`Prepare`, `Start`, `Ready`, `Health`, `Stop`, `Clean`), bound generic primitive(s), typed input/output contract, and terminal result semantics.
- **`ServiceSpec`** - long-lived foreground process defined by lifecycle operations over `ExecSpec`/`ProbeSpec`, endpoint set, readiness, health, stop policy, state roots, logs, lifecycle policy, containment requirement.
- **`TaskSpec`** - bounded `ExecSpec` with inputs, artifacts, summaries, exit policy, and source refs.
- **`WorkflowSpec`** - dependency graph over tasks, service requirements, readiness gates, artifact collection, cleanup actions, and cancellation policy.
- **`StateSpec`** - state roots, persistence, cleanup policy, marker identity, retention, and explicit purge requirements.
- **`PlacementSpec`** - logical roots, slot policy, candidate port windows, state/log/artifact layout.
- **`SurfaceSpec`** - public command/action exposed to humans and agents, with input/output schema, exit classes, aliases, and evaluation permission.
- **`SecretRef`** - descriptor for a secret injection target, never the secret value.

Nix-side adapters are libraries that generate these primitives and lifecycle operation declarations. For example, a Postgres module may produce one `ServiceSpec`, endpoint declarations, probes, state policy, tasks, lifecycle operations, and docs metadata. The runtime should not have Postgres-specific logic.

Dynamic adapter executable protocols may be added later, but they are not part of Milestone 0. In the first cut, adapter behavior should compile into direct model primitives.

> **Invariant RUNTIME-GENERIC-1:** The runtime executes generic model primitives and enforces typed Nixfied lifecycle operation semantics. Concrete adapters are Nix-side model generators unless a later milestone explicitly introduces a runtime adapter protocol.

## Core Concepts

- **Model:** `model.json`, the compiled semantic contract for what exists and how it can run.
- **Computed model hash:** a runtime-computed SHA-256 hash of raw `model.json` bytes, recorded for model identity/provenance but not embedded in the model.
- **Admission contract:** the model fields the runtime must validate before any process starts: exact ABI/toolchain identity, target identity, runtime capabilities, source policy, closure metadata, state policy, and secret descriptors.
- **Toolchain identity:** exact Nixfied model/executor contract identity that produced the model. In M0 this is a literal exact-match string; it should not churn for rendered docs or non-semantic compiler changes.
- **Runtime ABI:** exact runtime model ABI expected by `nixfied-runtime`; no compatibility negotiation in M0.
- **Closure:** an already-realised executable, helper, generated wrapper, or dependency produced by Nix and referenced by concrete store path plus model metadata describing its purpose, executable path, target system, and expected use.
- **Codebase / source identity:** a first-class model entry describing a logical codebase, its source mode, allowed live-workspace policy, and the source fingerprint or snapshot identity an admitted run may observe.
- **Target identity:** Nix `system`, OS family, architecture, runtime capability requirements, and closure system for which the model is valid.
- **Generated view:** schema, docs, capability, or other projection derived from `model.json`. Generated views may be materialised for humans and agents, but they are not independent semantic authority.
- **Environment:** a named mode of operation, such as `dev`, `test`, `ci`, preview, or prod-like.
- **Slot:** a deterministic parallel instance of an environment, identified by an integer index.
- **Run:** one runtime invocation with a unique run identity (`runId`).
- **Service address:** stable logical address of a service in a slot, derived from `(projectId, env, slot, serviceName)`.
- **Service identity hashes:** layered identities for service address, endpoint set, state epoch, runtime compatibility, and target identity. M0 requires exact matches; later adapter-declared compatibility may relax only the runtime-compatibility layer.
- **Service instance:** a reusable running service identified by `serviceInstanceId = hash(serviceAddress, endpointIdentity, stateIdentity, runtimeCompatibilityHash, targetIdentity)`.
- **Run lease:** heartbeat/TTL record for an active runtime invocation.
- **Service lease / claim:** durable ownership/lifetime record for a service instance that may outlive a single run.
- **Borrower lease:** per-run usage lease for a reused service instance. Reference count is derived from live borrower leases, not stored as independent mutable truth.
- **Registry:** a per-slot SQLite WAL database containing durable runtime state, ordered events, reservations, leases, service state, process records, and history. The OS remains the source of truth for liveness.

## Architecture & Data Flow

```text
project Nix modules / user flake
      |
      v
Nix correctness + compiler passes
  resolve -> validate -> derive -> emit model
      |
      +-- build/realise referenced closures
      v
Nix store output
      model.json          # only required semantic seam
      docs/views          # optional/generated from model; not semantic authority
      realised closures   # referenced by store path in model
      |
      v
nixfied-runtime
      model loader + raw hash
      ABI/toolchain/model-origin/admission-contract checks
      admission: source, target, state, ports, registry, closures
      generic executor: services, tasks, workflows, probes
      process owner + reconciler + cleanup
      |
      v
SQLite registry, state roots, logs, artifacts, summaries
```

### Boundary Rules

- Nix validates typed project intent and emits `model.json` as a Nix store output.
- Nix builds/realises executable closures before runtime invocation.
- The runtime reads one model file and performs all impure execution.
- The runtime never invokes Nix, imports Nix modules, shells to `nix eval`, shells to `nix-store`, builds a flake reference, realises a `.drv`, or mutates the model.
- The runtime records the model path, `computedModelHash`, `toolchainId`, `runtimeAbi`, source fingerprints, and target identity for every admitted run.
- The runtime validates the model-origin rule, exact ABI/toolchain identity, target compatibility, runtime capabilities, source policy, closure metadata, and state/secret admission constraints before any long-lived process starts.
- Schema, docs, and capabilities are model sections or generated views, not separate correctness artifacts.

## The Model Contract

`model.json` is versioned, serde-typed, owned by the shared `nixfied-model` crate, and produced by Nix. Required top-level fields:

- `modelVersion` - integer, initially `1`. Exact match only.
- `toolchainId` - exact Nixfied model/executor contract identity. In M0 this is a literal exact-match string emitted by the same build as the runtime.
- `runtimeAbi` - exact runtime ABI expected by this model.
- `generator` - Nixfied compiler version, source/release identity when available, and model emitter identity.
- `project` - stable project metadata and `projectId`.
- `target` - Nix `system`, OS family, architecture, ABI/libc where relevant, closure system, and required runtime capabilities.
- `codebases` - source identities and admission policies. Runtime operations observe source only through declared `codebaseId` plus relative paths, never implicit cwd.
- `environments` - named environment definitions.
- `slotPolicy` - slot index range, defaults, and placement-relevant slot policy.
- `capabilities` - generated, agent-readable catalog of environments, slots, services, tasks, workflows, state policy, source policy, and surfaces.
- `runtimeConstraints` - runtime-input constraints for env names, slot bounds, port overrides, dirty policy, collision policy, and other dynamic inputs.
- `surfaces` - public command surfaces with canonical name, aliases, input schema, output schema, exit classes, evaluation permission, and maturity.
- `placement` - logical run-independent roots, layouts, registry keys, and candidate port windows. No host-absolute paths.
- `state` - state policy, ownership markers, cleanup policy, persistence, retention, and explicit purge rules.
- `secrets` - secret descriptors and injection targets only. Secret values are never serialized into the model.
- `closures` - already-realised store paths for runtime-dispatched commands, helpers, wrappers, and dependencies, with closure ID, target system, executable paths, operation binding, effects classification, and optional content/NAR metadata when available.
- `execs` - reusable `ExecSpec` entries.
- `services` - declared `ServiceSpec` models.
- `tasks` - declared bounded `TaskSpec` models.
- `workflows` - declared `WorkflowSpec` graphs.
- `docs` - semantic documentation metadata when useful. Rendered docs are generated views, not required semantic model content.

### Admission Contract Fields

The runtime must validate the admission contract before any process starts:

- exact `modelVersion`, `runtimeAbi`, and `toolchainId` match;
- `generator` identity is present and recorded for provenance;
- `target` matches the host/runtime capability set;
- `codebases` satisfy the declared source/dirty policy;
- every referenced closure is already realised, executable when required, compatible with `target`, and declared in `closures`;
- every runtime operation observes source only through declared `codebaseId`s;
- state roots are derived from model placement and guarded by the declared marker/cleanup policy;
- secret descriptors are resolvable without serializing secret values into the model, argv, store paths, docs, generated views, registry, summaries, or persisted errors.

These checks replace the need for a required M0 manifest. The model is not merely the execution graph; it is also the runtime admission contract.

### Generated Views

The following commands are views over `model.json`:

- `model` - prints or queries the model.
- `schema` - emits the runtime input/output schema derived from `runtimeConstraints`, `surfaces`, and model types.
- `docs` - emits generated human-readable docs derived from model metadata.
- `capabilities` - emits the agent-readable capability catalog from `model.capabilities`.

These views may be materialised for convenience, but they are not required semantic artifacts and are not separate sources of truth.

> **Invariant SINGLE-MODEL-1:** `model.json` is the only source of semantic truth at the Nix/Rust boundary. Schema, docs, and capabilities are generated from it.

If generated views are materialised as files, they must remain disposable projections. A view may embed the `computedModelHash` it was generated from, and `nixfied` may refuse a stale view, but `nixfied-runtime` admits the model, not the view.

### Optional Manifest Envelope

No manifest is required for Milestone 0. A later compiled-output bundle may add a minimal manifest when the product needs portable bundles, cache export/import, standalone distribution outside the Nix store, or integrity-bound materialised views.

If introduced, the manifest is a non-semantic byte envelope:

- it may record `manifestVersion`, `hashAlgorithm`, `computedModelHash`, `toolchainId`, `runtimeAbi`, `target`, `generator`, `sourceSetHash`, `closureSetHash`, and optional view hashes;
- it must not contain any semantic field that is absent from `model.json`;
- it must be validated before deserializing bundled artifacts when used;
- it must not replace `model.json` as the runtime authority.

### Computed Model Hashing

`computedModelHash` is model identity/provenance, not an artifact field and not a self-check.

- The runtime reads raw `model.json` bytes from the Nix store path.
- It computes `sha256(raw bytes)`.
- It records the hash in registry events, summaries, logs, and error payloads.
- The hash covers the full admission contract because those fields live in the model.
- It never reserializes the model to validate the hash.

This avoids circularity without adding another artifact. For normal store models, integrity is provided by Nix store immutability and path admission. For future portable bundles, a non-semantic manifest or signature may bind artifact bytes before deserialization.

## Exact Toolchain, No Compatibility

Nixfied v2 has no cross-version compatibility goal.

- A runtime admits a model only when `toolchainId` and `runtimeAbi` exactly match.
- Unknown, old, or future model versions are refused.
- There is no migration layer for model artifacts.
- Updating Nixfied means updating the Nix flake input and recompiling the model.
- The ergonomic `nixfied` command must pair compiled models with the matching runtime from the same toolchain whenever it delegates.
- Install/upgrade surfaces may help update pins/import shims, but they do not promise old models continue to run on new runtimes.

> **Invariant ABI-1:** No cross-version model/runtime compatibility is promised in M0. Exact `toolchainId` and `runtimeAbi` match is required.

This keeps the first implementation simple and honest.

M0 identity fields:

- `modelVersion` names the structural model format.
- `runtimeAbi` names the runtime model/executor ABI.
- `toolchainId` names the compiler/runtime contract pair that emitted and executes the model.
- `generator` records compiler provenance for diagnostics and summaries; it is recorded, but exact admission equality is driven by `runtimeAbi` and `toolchainId`.

## Correctness Layers

Nixfied v2 separates desired-state correctness from observed-runtime correctness across four layers.

1. **Model correctness (Nix).** Reject invalid names, broken references, invalid workflow edges, malformed state/placement policy, missing closures, invalid generic primitive declarations, and invalid source/target policy before runtime is possible.
2. **Closure correctness (Nix).** Reproducibly construct and realise the executable store paths the runtime may execute.
3. **Admission correctness (Rust).** Host-specific checks Nix cannot prove: Nix-store model origin, exact ABI/toolchain match, generator provenance recording, source policy, target/runtime capability support, closure metadata and already-realised closure availability, writable marker-owned state roots, SQLite registry acquisition, port ownership strategy, stale-lease reconciliation, secret descriptor/unsupported-behavior policy, and ownership conflicts.
4. **Execution correctness (Rust).** The impure graph: process groups, signals, readiness/health, task execution, workflow cancellation, registry events, summaries, cleanup, and reconciliation.

This keeps Nix central to project evolution and codification while preventing Nix from becoming a live process supervisor.

## The Iteration Loop

Evolving a project is one explicit correctness loop:

```text
edit typed Nix
  -> nixfied compile
  -> Nix type checks + assertions + closure realisation
  -> /nix/store/...-nixfied-model/model.json
  -> nixfied-runtime run --model /nix/store/.../model.json
```

Operational rules:

- `nixfied compile` is the common-path surface that triggers Nix evaluation.
- Compile failures are typed and actionable, surfaced before any process starts.
- `nixfied validate --deep` may re-evaluate Nix and check live filesystem assumptions.
- Shallow `nixfied check` validates an already-compiled store model and host assumptions without invoking Nix from the runtime.
- `nixfied up` may compile by default when the current model is stale, then exec the matching runtime from the same toolchain.
- `--no-compile` admits an existing Nix-store model without recompiling.
- `--allow-non-store-model` is reserved for framework tests and local model development; it is not a stable user-facing runtime mode.

## Identity & Placement

Every runtime action is scoped by explicit identity:

```text
projectId / environment / slot / runId
```

- `projectId` - stable, explicit identifier from the model.
- `environment` - model-defined environment name.
- `slot` - user-selected deterministic parallel-instance index (`0..N`, default `0`).
- `runId` - runtime-created ULID per invocation.
- `computedModelHash` - runtime-computed hash of admitted raw model bytes.
- `target` - model target identity and required runtime capabilities.

### Source / Codebase Identity

Every source tree the runtime may observe is declared in `model.json`:

- `codebaseId` - stable logical identifier.
- `logicalRoot` - project-relative or flake-input-relative source root.
- `sourceMode` - `snapshot`, `flake-input`, or `live-workspace`.
- `sourceIdentity` - rev/NAR hash/lock identity when immutable, or a fingerprint policy for live workspaces.
- `dirtyPolicy` - `allow`, `warn`, or `reject`.
- `admissionFingerprintPolicy` - which files/metadata are checked before a run is admitted.

> **Invariant SOURCE-1:** Every task, service, workflow, and exec that observes source declares the `codebaseId`s it may observe. Every admitted run records the source fingerprints used.

### Service Identity

Service identity is split into stable address and layered compatibility identity:

```text
serviceAddress            = hash(projectId, environment, slot, serviceName)
endpointIdentity          = hash(logical endpoint set and port policy)
stateIdentity             = hash(state epoch, persistence policy, and state layout)
runtimeCompatibilityHash  = hash(exec/lifecycle/probe/containment semantics)
serviceInstanceId         = hash(serviceAddress, endpointIdentity, stateIdentity, runtimeCompatibilityHash, targetIdentity)
```

`runtimeCompatibilityHash` covers exec paths, args, environment shape, lifecycle operation bindings, readiness and health definitions, cleanup policy, containment requirements, target identity, and runtime capabilities. `endpointIdentity` and `stateIdentity` are separate so harmless runtime-adjacent changes do not blur state or port ownership rules.

M0 has no compatibility override: any identity-layer change produces a different service instance. Later milestones may add adapter-declared compatibility rules for the runtime-compatibility layer only; endpoint and state identity remain explicit ownership boundaries.

> **Invariant SVC-ID-1:** Service reuse in M0 is allowed only when service address, endpoint identity, state identity, runtime compatibility hash, and target identity match exactly.

### Placement

Placement is split by phase:

- **Logical placement is derived by Nix and baked into `model.json`:** template roots, relative layouts, registry keys, and candidate port windows.
- **Host-absolute placement is materialised by Rust at admission:** `$NIXFIED_STATE_DIR`, else documented platform default, joined with model-provided logical templates.
- **Run-scoped paths are composed by Rust:** `runId` is not known at compile time.
- **Port binding is runtime:** candidate windows are deterministic, but actual ownership is host-specific.

```text
state_root_tmpl = <project>/<env>/<slot>/
port_window     = <candidate window for (project, env, slot)>

state_base      = $NIXFIED_STATE_DIR | <platform default>
state_root      = <state_base>/<state_root_tmpl>
registry_dir    = state_root/registry/
run_dir         = state_root/runs/<runId>/
artifacts_dir   = run_dir/artifacts/
logs_dir        = run_dir/logs/
```

## Port Reservation And Ownership

Pure derivation is insufficient for ports, and so is a registry reservation. The registry coordinates Nixfied processes only; it is not an OS reservation.

> **Invariant PORT-1:** No service is considered ready without verified ownership of its endpoint.

Runtime algorithm:

1. Take the baked candidate window for `(project, env, slot)`.
2. Under the slot registry transaction, record reservation intent against `serviceInstanceId` and selected endpoint.
3. Start the owned service with the selected endpoint.
4. Readiness verifies that the expected owned process actually bound the expected endpoint, not merely that the port is open.
5. On bind conflict, record collision and retry per model policy: `fail`, `probe-in-range`, or `request-override`.
6. If supported, socket activation lets the runtime hold the listener and pass the descriptor to the service.
7. Without socket activation, runtime uses platform socket-owner inspection and matches the listener to the tracked process identity or containment domain.
8. Release on normal stop, cleanup, or reconciliation after proving the owner is gone.

## Registry

The registry is a durable record, not a liveness oracle. The OS owns liveness.

- **Storage:** one SQLite WAL database per `(projectId, env, slot)`.
- **Ordering:** every state-mutating event has a total per-slot sequence. Timestamps are diagnostic only.
- **Transactions:** reservations, lease updates, process registration, service state transitions, and cleanup records are transactional.
- **Durability & integrity:** SQLite WAL provides crash recovery and atomic commits. Registry schema version is explicit; incompatible or corrupt registries fail with typed errors instead of being silently treated as live truth.
- **Filesystem assumptions:** M0 assumes local filesystems with SQLite WAL and lock semantics that SQLite supports. Network filesystems and remote coordination are not portable guarantees.
- **Ownership keys:** reservations and leases attach to `serviceInstanceId`, not just `runId`.
- **Process identity:** every process record carries enough to survive PID reuse: pid, process-group id, start-time or platform equivalent, optional pidfd/stable handle, command metadata, owning run ID, and service instance where applicable.
- **Reconciliation:** `ps` reconciles records against the OS before reporting `running`, `stopped`, `stale`, `canceled`, or `orphaned`.

> **Invariant REG-1:** One transactional per-slot SQLite registry owns shared mutable runtime state.

> **Invariant REG-ORDER-1:** Every state-mutating event has a total per-slot order. Wall-clock timestamps are diagnostic evidence only.

> **Invariant LIVE-1:** Liveness is always reconciled against the OS before being reported.

## Leases And Service Lifetime

No daemon is required in the initial runtime, so lease semantics cannot depend on an always-running supervisor.

- `run-scoped` - service is stopped when the owning run exits or is canceled.
- `until-idle` - service may outlive a run while live borrower leases exist; GC stops it after idle policy is satisfied.
- `persistent-until-down` - service persists after the run exits and is stopped only by explicit `down`/`clean` with matching scope and policy.

Reference count is derived from active borrower leases. It is not an independently mutated counter.

> **Invariant LEASE-1:** GC may reap a service only after proving: process identity is owned, no active borrower lease remains, no service lease forbids stop, and cleanup policy permits it.

## State Cleanup Safety

Every runtime-owned state root contains an ownership marker, `.nixfied-state.json`, with project, environment, slot, optional service/workflow identity, state kind, state epoch, cleanup policy, and owning model identity.

Cleanup is path-confined and marker-gated:

- canonicalize before deletion;
- refuse paths outside the resolved state base;
- refuse symlink traversal and path escapes;
- refuse unmarked roots;
- refuse marker identity mismatch;
- refuse active leases, live process refs, and active port reservations;
- require explicit purge for policy-protected persistent state;
- write cleanup intent and terminal cleanup event transactionally.

> **Invariant GC-1:** Cleanup is idempotent and safe to re-run after a crash.

> **Invariant GC-2:** Cleanup is path-confined, marker-gated, lease-gated, process-gated, and policy-gated.

## Process Containment

Containment is a runtime capability, not a universal promise.

- Milestone 0 services must run in the foreground under the runtime-owned process group.
- Linux strict containment may later use cgroup v2 + pidfd + process group + subreaper where available.
- macOS basic containment is process-group based and explicitly weaker.
- Daemonization/double-fork/`setsid` without stable handoff is rejected as `PROC_ESCAPE` or refused at admission.

> **Invariant PROC-1:** Every spawned process belongs to a runtime-owned process group, and no service is admitted unless the runtime can either contain it or record a stable child handoff identity.

> **Invariant PROC-2:** Cancellation propagates to the whole tracked process group.

> **Invariant PROC-3:** A long-lived process is considered started only after it has a registry process record, and no workflow node may depend on an untracked service process.

> **Invariant PROC-CAP-1:** Admission fails if a service requires stronger containment than the host runtime supports.

## Secrets Boundary

Full secrets management is deferred, but secret non-leakage is not.

M0 may take the conservative path: allow an empty `secrets` section and reject non-empty secret descriptors with a typed unsupported error. If descriptor-only validation is implemented in M0, injection still remains unsupported unless the non-leakage proof exists.

- The model may contain secret descriptors (`SecretRef`, env var name, file injection target, required/optional, source policy), never values.
- Secret values are resolved by the runtime at admission and injected only through approved channels.
- Approved injection channels are runtime-owned environment variables or runtime-created secret files with restrictive permissions. Secret values are forbidden in argv, store paths, docs, capabilities, generated views, registry events, summaries, operation results, and persisted error payloads.
- Logs, child stderr/stdout capture, errors, adapter results, registry writes, summaries, and persisted diagnostics are redacted before persistence.
- Platform process inspection can expose environment values to sufficiently privileged local users; this is documented as a local-host limitation, not treated as a remote secrecy guarantee.

> **Invariant SECRET-1:** `model.json` may contain secret references, never secret values.

> **Invariant REDACT-1:** Persistent runtime output is redacted before write.

## Public Surfaces

The public API is derived from `model.surfaces`, not from historical command families.

Canonical ergonomic surfaces:

- `compile` - evaluate Nix and produce a Nix-store `model.json`.
- `check` - validate model origin, ABI, source policy, target support, closure availability, and host assumptions.
- `validate --deep` - explicit Nix re-evaluation and deeper project checks.
- `model` - inspect/query the model.
- `schema` - generate schema view from model.
- `docs` - generate docs view from model.
- `capabilities` - emit model capability catalog.
- `up` - start required services.
- `down` - stop owned services.
- `run` - run task or workflow.
- `ps` - reconcile and list observed state.
- `logs` - locate or stream runtime-owned logs.
- `clean` - reconcile and clean stale state, leases, reservations, and owned processes.
- `install` / `upgrade` - update Nixfied-owned flake pins/import shims only; no compatibility promise.

`introspect` may exist as an alias for `model`. `svc` may exist only as a generated alias if declared in `model.surfaces`; it is not core.

> **Invariant SURFACE-1:** A public command is stable only when its schemas, exit classes, aliases, maturity, and evaluation permission are declared in `model.surfaces`.

Every feature must answer:

1. What problem does it solve in deterministic multi-env execution?
2. What model field owns it?
3. What Nix compiler/module validation proves it statically?
4. What runtime path executes or observes it?
5. What proof demonstrates it?

If a feature cannot answer those five, it is not core.

## Runtime Error Contract

The engine emits stable, machine-readable error categories, each with a stable code, typed payload, and exit class.

- `MODEL_NOT_STORE_OUTPUT` - admitted model path is not a Nix store output.
- `MODEL_INVALID` - model failed parse, type, or structural validation.
- `MODEL_ADMISSION` - model-origin, generator/toolchain, target, source, closure metadata, state policy, or secret admission contract failed before execution.
- `RUNTIME_ABI_MISMATCH` - `runtimeAbi` or `toolchainId` does not exactly match the runtime.
- `SOURCE_MISMATCH` - admitted source fingerprint does not satisfy source/dirty policy.
- `PLATFORM_UNSUPPORTED` - target identity or required runtime capability does not match this host/runtime.
- `CLOSURE_MISSING` - an expected realised store path is unavailable, not declared in model closure metadata, incompatible with target, or not executable when required.
- `PORT_CONFLICT` - no endpoint in the window could be owned within policy.
- `PORT_UNVERIFIABLE` - listener ownership cannot be matched to the service process identity or containment domain.
- `STATE_UNWRITABLE` - resolved state root cannot be created or written.
- `STATE_UNOWNED` - cleanup target lacks a matching `.nixfied-state.json` marker or escapes the state base.
- `LEASE_STALE` / `LEASE_CONFLICT` - lease expired or is contended by another live owner.
- `PROC_ESCAPE` - spawned process could not be tracked or reconciled.
- `READINESS_TIMEOUT` - service did not reach ready within policy.
- `SECRET_UNAVAILABLE` - required secret reference could not be resolved.
- `SECRET_LEAK_BLOCKED` - secret value was detected in a payload that would be persisted.
- `CANCELED` - run was canceled.
- `CLEANUP_REFUSED` - cleanup declined to delete protected or unsafe state.
- `REGISTRY_CORRUPT` - SQLite registry failed integrity/schema checks.

## Runtime Invariants

- **MODEL-SEAM-1:** `model.json` is the only required semantic artifact consumed by the runtime.
- **MODEL-ORIGIN-1:** Normal runtime admission requires `model.json` to be a Nix store output; non-store admission is an unstable framework-development/test mode only.
- **MODEL-CONTRACT-1:** The model carries the full admission contract: generator/toolchain identity, runtime ABI, target identity, runtime capabilities, source policy, closure metadata, layered service identity, state cleanup policy, and secret descriptors.
- **SINGLE-MODEL-1:** Generated schema, docs, and capability outputs are views over `model.json`, not separate semantic authorities.
- **HASH-1:** `computedModelHash` is computed from raw model bytes and is never embedded in `model.json`.
- **ABI-1:** Exact `runtimeAbi` and `toolchainId` match is required; no compatibility promise exists.
- **SEAM-1:** `nixfied-runtime` never invokes Nix.
- **PREPARE-1:** All referenced closures are realised before `nixfied-runtime` starts; runtime only verifies them.
- **RUNTIME-GENERIC-1:** The runtime executes generic primitives and enforces typed lifecycle operation semantics.
- **NIX-API-1:** Nix modules are the user-facing integration API.
- **SOURCE-1:** Runtime source observation happens only through declared `codebaseId`s.
- **SVC-ID-1:** M0 service reuse requires exact service address, endpoint identity, state identity, runtime compatibility hash, and target identity match.
- **PORT-1:** Readiness requires verified endpoint ownership.
- **REG-1:** One transactional per-slot SQLite registry owns shared mutable runtime state.
- **REG-ORDER-1:** Every state-mutating event has a total per-slot order; timestamps are diagnostic evidence only.
- **LIVE-1:** Liveness is reconciled against OS reality before reporting.
- **LEASE-1:** GC may reap only policy-eligible services with no live borrower/service lease protection.
- **PROC-1:** Every spawned process belongs to a runtime-owned process group, and services require containment or stable child handoff.
- **PROC-2:** Cancellation propagates to the whole tracked process group.
- **PROC-3:** Long-lived processes are considered started only after registry process records exist.
- **PROC-CAP-1:** Admission fails when required containment exceeds host capability.
- **GC-1:** Cleanup is idempotent and crash-safe.
- **GC-2:** Cleanup is path-confined, marker-gated, lease-gated, process-gated, and policy-gated.
- **SECRET-1:** Models contain secret refs, never secret values.
- **REDACT-1:** Persistent runtime output is redacted before write.
- **SURFACE-1:** Stable public commands are declared in `model.surfaces`.
- **SHELL-1:** Shell cannot own graph, registry, summary, validation, liveness, or cleanup semantics.
- **NIX-1:** Nix cannot own live process supervision, liveness reporting, cancellation, registry mutation, or cleanup.

## Milestones

Ordering hardens failure and lifecycle semantics before the first real stateful adapter, so adapter complexity cannot accumulate on top of unproven cancellation/GC.

### Milestone 0 Gates

M0 implementation must not start until these are decided:

1. Minimal M0 `model.json` shape, including admission contract fields required by the walking skeleton.
2. Exact `runtimeAbi` / `toolchainId` / `generator` identity format.
3. Nix-store model output path, normal admission rules, and unstable non-store test/dev escape hatch.
4. Minimal generic primitive schema for `ExecSpec`, `EndpointSpec`, `ProbeSpec`, `ServiceSpec`, `TaskSpec`, and `LifecycleOpSpec`; `WorkflowSpec`, rich `SurfaceSpec`, and full `SecretRef` behavior are stubbed or deferred.
5. Closure metadata shape and already-realised closure verification rules.
6. SQLite registry schema version `1`, total ordering, create-if-missing behavior, and supported local filesystem assumptions. No migrations, compaction, vacuum, or export policy in M0.
7. M0 process identity and containment matrix for Linux and macOS.
8. Source/codebase identity for one `live-workspace` codebase and the default M0 dirty policy.
9. Target identity fields and runtime capability checks.
10. M0 unsupported-behavior policy for non-empty secret descriptors, workflows, adapters, service reuse, and multi-slot execution.
11. State ownership marker and cleanup refusal rules.

### Milestone 0 - Walking Skeleton

Smallest end-to-end vertical slice:

- one typed Nix project module -> one Nix-store `model.json`;
- one exact `runtimeAbi` / `toolchainId` check;
- one generator identity, target identity, one live-workspace source placeholder, closure metadata, and state policy admission check;
- `nixfied-runtime` refuses non-store model paths;
- one env (`dev`), one slot (`0`), one task, one foreground synthetic service;
- one `ExecSpec`, one `EndpointSpec`, one `ProbeSpec`, one `ServiceSpec`, one `TaskSpec`, and typed lifecycle operation bindings for start/ready/stop;
- one per-slot SQLite registry schema version `1`;
- one port reservation with ownership-verified readiness;
- one state root with `.nixfied-state.json`;
- one log root and one summary file;
- `model` works, and `schema`, `docs`, and `capabilities` are minimal generated views from `model.json`;
- runtime works with `nix` unavailable when all referenced closures are already realised.

Complete when a downstream-shaped minimal example can compile a Nix-store model, validate the model admission contract, start a foreground service, verify endpoint ownership at readiness, run a dependent task, write SQLite registry events and a summary, stop the owned service, record model path / computed model hash / generator / runtime ABI / toolchain ID / target / basic source provenance, clean only marker-owned state, and report no live owned processes after reconciliation.

No required manifest, concrete adapters, runtime adapter ABI, workflow graphs, service reuse, real secret injection, full redaction engine, install/upgrade polish, SQLite migrations, multi-slot execution, or crash-hardening GC yet.

### Subsequent Milestones

1. **Slot isolation** - two concurrent slots of the same env: disjoint ports, state, logs, registries, and summaries.
2. **Cancellation & GC hardening** - signal propagation, lease expiry, `kill -9` survival and reconciliation, stale-reservation cleanup, marker-gated cleanup, persistent-state refusal.
3. **Nix-side reference adapter: minimal Postgres** - Nix module generates generic model primitives for Postgres; runtime remains generic.
4. **Workflow graphs** - dependency-aware workflows with bounded tasks, service requirements, readiness gates, cancellation, artifacts, summaries, and cleanup policy.
5. **Installable downstream wrapper** - adoption/upgrade through Nixfied-owned flake input/import shims without overwriting project-owned declarations.
6. **Polyglot example** - `examples/polyglot-stack`.
7. **Optional manifest envelope** - only if portable bundles, cache export/import, standalone distribution outside the Nix store, or integrity-bound materialised views need it.
8. **Optional runtime adapter protocol** - only if direct generic primitives prove insufficient.

## Proof Strategy

Proofs should be tiered, not one monolithic proof workspace.

- Nix compiler proofs - model validation, admission contract validation, generic primitive validation, closure construction, exact raw-byte model emission.
- Runtime unit proofs - model origin checks, ABI/toolchain mismatch refusal, target/source/closure admission refusal, placement composition, port ownership, SQLite registry transactions, total event ordering, process identity, reconciliation, M0 lease policy, marker-gated cleanup.
- Generic primitive proofs - `ExecSpec`, `EndpointSpec`, `ProbeSpec`, `ServiceSpec`, `TaskSpec`, and lifecycle operation bindings execute without project-specific runtime logic.
- Nix-side adapter proofs - concrete Nix modules generate valid generic primitives without being required by minimal projects.
- Downstream-shaped examples exercising public APIs only.

### Adversarial / Negative Proofs

- non-store `model.json` is refused;
- non-store model admission is accepted only behind the unstable test/dev escape hatch;
- malformed model is refused;
- mismatched `runtimeAbi` / `toolchainId` is refused;
- runtime with `nix` unavailable still runs an already-realised store model;
- source mismatch / dirty-policy violation is refused;
- target mismatch is refused;
- missing, undeclared, target-incompatible, or non-executable closure path is refused;
- port conflict at bind time is recorded and handled;
- open port owned by unrelated process fails readiness;
- service daemonization/double-fork attempt is detected or refused;
- PID reuse does not produce false liveness;
- SQLite transaction rollback preserves invariants after interrupted writes;
- two slots race on registry and port allocation without corrupting state;
- cleanup refuses unmarked roots, symlink escapes, path traversal, marker mismatch, and protected persistent state;
- non-empty secret descriptors are rejected as unsupported in M0, or secret values are rejected/redacted before persistence once implemented.

## Definition Of Done

- **Model:** Nix compiles typed intent into one Nix-store `model.json`; runtime refuses non-store, malformed, ABI-mismatched, target-mismatched, source-mismatched, or closure-invalid models before execution.
- **Admission contract:** generator/toolchain identity, runtime ABI, target identity, source policy, closure metadata, state policy, and secret descriptors are model fields validated before any process starts.
- **Nix integration:** a downstream project can import Nixfied modules and generate a model without adopting a prescribed repo layout or vendoring framework internals.
- **Generic runtime:** service/task behavior is expressed through generic model primitives plus typed lifecycle operation semantics, not runtime service-specific code.
- **Discoverability:** `model`, `schema`, `docs`, and `capabilities` views expose environments, slots, services, tasks, workflows, source policy, state policy, secrets policy, target identity, and surfaces without reading project source.
- **Source identity:** runtime operations observe source only through declared `codebaseId`s and recorded fingerprints.
- **Environment:** `dev`, `test`, and `ci` express different services, workflows, state, and cleanup policy through one typed model.
- **Slot:** two slots of the same env run concurrently with disjoint placement, ports, state, logs, registries, and summaries.
- **Registry:** runtime reports live/stopped/stale/canceled/orphaned via OS reconciliation, not stale files.
- **Service:** start -> ready -> health -> stop is owned end-to-end, with verified endpoint ownership and process-group teardown.
- **State:** cleanup is marker-owned, path-confined, policy-controlled, and safe to retry.
- **Workflow:** workflows express service requirements, task dependencies, cancellation, artifacts, summaries, and cleanup policy after the workflow milestone; M0 rejects workflow execution as unsupported.
- **Secrets:** models contain secret refs only; M0 rejects non-empty secret descriptors unless the explicit descriptor-only proof is implemented. Persistent runtime output must never contain secret values.

## Repository Layout (greenfield)

```text
flake.nix                  # apps, devShell, packages, model builder
nix/
  modules/                 # typed user-facing modules
  compiler/                # resolve -> validate -> derive -> emit model
  spec/                    # versioned model/primitive/admission-contract definitions
  lib/                     # pure helpers
  adapters/                # Nix-side model generators (postgres first)
runtime/
  Cargo.toml
  crates/
    nixfied-model/         # serde types for model.json
    nixfied-runtime/       # generic execution engine
    nixfied-cli/           # ergonomic CLI
examples/                  # downstream-shaped workspaces
docs/
```

## Install & Upgrade Ownership

Install and upgrade are Nix adoption surfaces, not compatibility machinery.

- `install` may add a pinned flake input, import shim, wrapper app, or generated Nixfied-owned file under an explicit ownership marker.
- `upgrade` may update Nixfied-owned pins/import shims and lock metadata.
- Neither command overwrites project-owned model declarations, service definitions, source declarations, workflows, secrets policy, or state policy.
- After upgrade, the project recompiles its model with the new Nixfied input.
- Old compiled models are not promised to run on new runtimes.

## Non-Goals

- No compatibility with the current repository layout.
- No migration layer for v1 commands.
- No cross-version model/runtime compatibility.
- No required manifest envelope in M0.
- No required service adapter for a minimal project.
- No runtime service-specific adapter logic in M0.
- No daemon requirement for the initial runtime.
- No remote or multi-host execution.
- No duplicate command families for the same lifecycle action.
- No shell-owned semantic sidecars.
- No Nix invocation from `nixfied-runtime`.
- No checked-in vendored framework fixture.
- No tests that encode historical structure instead of product guarantees.

## Deferred

Real and intentionally out of v2's first cut:

- Full secrets/credentials management beyond non-leakage/redaction.
- Inter-service dependency DAGs beyond workflow service requirements and readiness gates.
- Centralized log aggregation and retention beyond runtime-owned logs.
- Multi-host / remote execution.
- Persistent daemon as a required runtime substrate.
- UI / dashboard surfaces.
- Manifest-sealed compiled-output bundles, unless portable bundles/cache export/import or standalone distribution require them.
- Runtime adapter protocol, unless generic primitives prove insufficient.

## Open Questions

### Must Decide Before Milestone 0

1. Minimal M0 `model.json` schema, admission contract fields, and primitive shapes.
2. Exact `runtimeAbi` / `toolchainId` / `generator` format.
3. Whether the model builder is only a Nix derivation output or also has a CLI convenience output.
4. Shape and safeguards for the unstable non-store model test/dev escape hatch.
5. Closure metadata schema and `PREPARE-1` already-realised closure verification rules.
6. SQLite registry schema version `1`, total ordering, create-if-missing behavior, and supported local filesystem assumptions.
7. Minimum process identity and port-owner strategy on Linux and macOS.
8. Default M0 source dirty policy for the single `live-workspace` codebase.
9. Target identity fields and runtime capability checks.
10. M0 unsupported-behavior policy for non-empty secrets, workflows, adapters, service reuse, and multi-slot execution.
11. State marker schema.

### Before Slot Isolation / Cancellation & GC

12. Port-window base, size, and slot stride.
13. Default run lease TTL, service lease policy defaults, borrower lease expiry, and opportunistic GC cadence.
14. SQLite migrations, compaction, vacuum, and export policy.
15. Explicit persistent purge command shape.

### Before Adapter / Workflow

16. How much service-dependency behavior belongs in workflow vs deferred.
17. Borrower lease / service lease handoff state machine.
18. Minimal Postgres Nix adapter shape as a model generator.
19. Adapter-declared compatibility rules for the `runtimeCompatibilityHash` layer, if exact M0 reuse becomes too rigid.

### Before Install

20. `nixfied` / `nixfied-runtime` distribution: flake only vs standalone release artifacts.
21. Install/upgrade ownership markers and rollback behavior.
22. Whether standalone distribution requires a minimal non-semantic manifest envelope.

## Constraints From v1

Captured so the rebuild does not repeat history:

- Runtime semantics drifted into shell helpers and Nix builders, with no single owner. Hence SEAM-1, SHELL-1, NIX-1, and the two-binary split.
- Artifact sealing could grow into a mini package format. Hence the single-model seam, runtime-computed provenance hash, and model-owned admission contract.
- Live checkout state was implicit. Hence first-class `codebases`.
- The registry was treated as the source of truth for liveness. Hence OS reconciliation.
- Pure port derivation was treated as sufficient. Hence ownership-verified readiness.
- Placement rules drifted when both Nix and runtime derived them. Hence logical placement in Nix, host materialisation in Rust.
- Service reuse could blur incompatible runtime configs. Hence layered service identity and exact M0 matching.
- Lease/refcount semantics could depend on a daemon that does not exist. Hence run/service/borrower lease split.
- Process containment differs by platform. Hence capability-gated containment, foreground-only M0 services, and stable child handoff for anything that would otherwise escape.
- Adapter complexity accumulated before lifecycle ownership was proven. Hence generic primitives before concrete adapters.
- A single large proof workspace becomes a second framework snapshot. Hence tiered proofs.
- Duplicate command families accreted for the same lifecycle action. Hence model-derived public surfaces.

## Success Criteria

A downstream project can import Nixfied into its own Nix setup, define multiple codebases, tasks, workflows, services, environments, slots, machine outputs, realised runtime closures, secret references, target requirements, source policy, generator/toolchain identity, and state policy in typed Nix; evaluate it for static correctness; compile it into one Nix-store `model.json` whose admission contract is validated before execution; then execute that admitted model through one generic, Nix-free Rust runtime with typed lifecycle semantics, isolated placement, SQLite-backed lifecycle records, process-aware reconciliation, layered service identity, clean cancellation, marker-gated crash-safe cleanup, durable registry history, secret-safe persistent outputs, and reproducible summaries.

## Decision Log

- **Many artifacts -> one semantic model.** `model.json` is the only required semantic seam. `schema`, `docs`, and `capabilities` are views over the model; no required M0 `manifest.json`.
- **Manifest hardening -> model-owned admission contract.** Source identity, target identity, generator/toolchain identity, closure metadata, runtime capabilities, layered service identity, state policy, and secret descriptors live in `model.json` and are covered by `computedModelHash`.
- **Self-hash -> computed runtime provenance hash.** No self-hash is embedded. The runtime computes `computedModelHash = sha256(raw model bytes)` and records it.
- **Model origin -> Nix store output.** Normal runtime admission requires `model.json` to be produced by Nix and live in the Nix store; non-store admission is an unstable framework-development/test escape hatch.
- **Optional manifest -> non-semantic future envelope.** A manifest may be added later for portable bundles, cache export/import, standalone distribution outside the Nix store, or integrity-bound materialised views, but it must not become semantic authority.
- **Compatibility -> exact ABI for M0.** No cross-version compatibility is promised in M0. Exact `runtimeAbi` and `toolchainId` match is required.
- **Runtime role -> generic executor with lifecycle semantics.** Rust executes generic primitives and enforces typed lifecycle operation semantics. Concrete adapters are Nix-side model generators unless a later milestone proves a runtime adapter protocol is necessary.
- **Nix role -> open integration API.** Users integrate Nixfied through Nix modules and their own flake/project shape.
- **Runtime Nix usage -> none.** `nixfied-runtime` never invokes Nix; `PREPARE-1` requires closures to be realised before runtime starts.
- **Registry storage -> per-slot SQLite WAL.** SQLite owns transactional reservations, leases, events, process records, and cleanup state.
- **Service identity -> layered identity with exact M0 matching.** Reuse is gated by exact service address, endpoint identity, state identity, runtime compatibility hash, and target identity. Adapter-declared compatibility rules are deferred until a concrete need appears.
- **Milestone order -> narrowed generic spine first.** Skeleton -> slot isolation -> cancellation/GC -> Nix-side Postgres adapter -> workflows -> install -> polyglot -> optional manifest envelope -> optional runtime adapter protocol.
