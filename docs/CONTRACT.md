# Nixfied contract

This document is the normative product and behavioral contract for the current
Nixfied architecture, authoring surface, model/runtime boundary, and public
outputs. [`ARCHITECTURE.md`](ARCHITECTURE.md) explains why these constraints
exist; [`DEVELOPMENT.md`](DEVELOPMENT.md) explains how to work on the
implementation.

The runtime ABI's model-field and enum names, hidden runtime-command surface,
output-field vocabulary, and error vocabulary are inventoried in
[`runtime/crates/nixfied-model/capability.txt`](../runtime/crates/nixfied-model/capability.txt).
That authored descriptor is hashed identically by Nix and Rust to derive the
`runtimeAbi` suffix. The complete typed shape and validation live in the Nix
producer and `nixfied-model`; the descriptor records the vocabulary the runtime
understands, and its digest ensures a recorded model/runtime change rotates the
ABI. Nix-only library and flake-app surfaces are public integration API but stay
outside `runtimeAbi` unless they change emitted model data or runtime behavior.
[`DERIVATION_SPEC.md`](DERIVATION_SPEC.md) is separately normative for facts
derived from the task/service graph.

Do not weaken an invariant below without an explicit contract change and
coordinated implementation, documentation, and test updates. Rotate the ABI
when the model/runtime contract changes.

## Model and version boundary

- **MODEL-SEAM-1 / SINGLE-MODEL-1:** `model.json` is the only required semantic
  artifact. The generated `views/docs.md` human reference is a disposable
  projection and never independent authority. Do not add a required manifest,
  sidecar, or envelope.
- **MODEL-ORIGIN-1:** normal admission requires `model.json` under the Nix store.
  `--allow-non-store-model` is an unstable framework test/development escape
  hatch, not an adopter path.
- **MODEL-CONTRACT-1:** the model carries the complete admission contract:
  generator and toolchain identity, runtime ABI, target, source policy, closure
  metadata, the inputs needed to derive layered service identity, state policy,
  and secret descriptors. Secret descriptors are references; secret values are
  never model data.
- **HASH-1:** the runtime computes `computedModelHash` as SHA-256 over the raw
  model bytes. The model contains no self-hash.
- **ABI-1:** admission requires exact `runtimeAbi` and `toolchainId` matches.
  Backward compatibility imposes no design constraint: a deliberate contract
  change may break any prior model, runtime command, output, or error surface.
  The new contract replaces the old one; old contracts are rejected, never
  migrated, translated, or admitted through a compatibility fallback. The
  runtime ABI suffix is the capability-descriptor digest, computed identically
  by `nixfied-model::constants` and `nix/spec/constants.nix`.
- **PREPARE-1:** Nix realises every referenced closure before runtime start. The
  runtime verifies existence, executability, target, and declaration; it never
  builds missing closures.
- Admission is a global pre-spawn barrier. Invalid model origin, ABI, toolchain,
  target, source, closure, state policy, or unsupported host feature fails before
  any child process starts.

## Ownership boundary

- **SEAM-1:** `nixfied-runtime` never invokes `nix`, `nix-store`, `nix build`, or
  `nix eval`, and never imports Nix expressions.
- SEAM-1 governs the runtime binary. A declared child invocation may execute a
  Nix tool if the model supplies it. Framework compiler/install tests stay
  outside runtime tasks because their open-ended Nix builds and external fetches
  do not fit the bounded role of those framework tasks and blur test ownership,
  not because the child process is technically unable to run Nix.
- **RUNTIME-GENERIC-1:** Rust executes generic primitives and knows no service
  domain. Concrete adapters are Nix-side model generators. A domain-specific
  runtime need signals a missing generic primitive, not permission to specialize
  the runtime.
- **NIX-API-1:** typed Nix modules are the public integration and correctness
  layer. Project behavior compiles into generic primitives.
- **SHELL-1 / NIX-1:** shell cannot own graph, validation, registry, liveness,
  summary, or cleanup semantics. Nix cannot own live supervision, cancellation,
  liveness, reconciliation, registry mutation, or cleanup.

## Task and service algebra

- **KIND-2:** the model has exactly two semantic kinds: task and service. New
  adopter vocabulary must first be expressed as names over that algebra; a new
  schema kind requires proof that the algebra cannot represent it.
- **INVOKE-1:** invocations are inline anonymous structural values. There is no
  invocation registry or invocation ID in the wire format.
- **STATIC-1:** composites are fully applied static DAGs. Parameters,
  conditionals, retries, and loops belong in Nix-side expansion or inside an
  opaque leaf, not in the runtime contract.
- **DERIVE-1:** graph facts such as `servicesRequired`, `operationBindings`, and
  operation IDs are derived according to `DERIVATION_SPEC.md`. Nix and Rust
  compute them independently and admission compares them fail-closed.
  Hand-declaration is reserved for choices such as exported verbs and
  attestations such as effects.
- **CACHE-1:** cache is neither a semantic kind nor a runtime resource. Nixfied
  does not identify, place, create, lock, report, retain, or selectively clean
  cache artifacts. Child tools and projects own those concerns through ordinary
  invocation environment or arguments.
- **VERB-1:** `help`, `run`, `ps`, `down`, `clean`, and `model-check` are reserved
  project-app names. `nixfied.surface.verbs` is an attrset mapping each
  explicitly exported task id to its exact nonempty user-facing app
  description. Project verbs derive only from those task names; undeclared ids,
  reserved-name collisions, empty/non-string descriptions, and the former list
  form fail at Nix evaluation. `{}` emits no adopter task apps. The descriptions
  are Nix-only app metadata: they are not emitted into `model.json` or
  `views/docs.md`, and do not enter `runtimeAbi`.
- **SURFACE-1:** hidden runtime commands are framework-owned and listed in the
  capability descriptor, not declared by an adopter in the model. The adopter
  declaration owns only its exported task-verb surface; unrelated custom flake
  apps remain ordinary Nix integration. `model-check` is the generated app name
  for the runtime's admission-only `check` command. Every runtime-backed control
  and exported task app accepts `-h` and `--help`; that help completes before
  model admission, source resolution, state materialisation, or execution. The
  generated `.#help` app is instead a Nix-only projection of final current-flake
  app metadata, sorted by app name and rendered without a caller-relative flake
  reference. It maps to no runtime command, admits or executes no model, and
  stays outside `runtimeAbi`. `projectApps` requires project-root `flake.nix`,
  `flake.lock`, and `nixfied.nix`; its help app is source-bound and rejects a
  current-flake context that does not match that source.
- **Hermetic child environment:** every leaf, probe, and service start receives
  only its declared environment plus the runtime-owned `PATH` assembled from
  invocation tool roots. Runtime environment inheritance and append-to-inherited
  patterns are unsupported.

## Source, identity, and endpoints

- **SOURCE-1:** runtime operations observe source only through declared
  `codebaseId`s. `live-workspace` roots resolve from the invocation root;
  immutable `snapshot` and `flake-input` roots resolve from the Nix store path in
  `sourceIdentity`, independent of the current working directory.
- Host-absolute placement never enters `model.json`; Rust materialises host paths
  during admission and execution.
- **SVC-ID-1:** service reuse requires exact service address, endpoint identity,
  state identity, runtime compatibility hash, and target identity.
- **PORT-1:** when a service declares endpoints, startup is serialized by a host
  endpoint lock and readiness requires exact kernel-observed ownership of every
  endpoint. Exact-bind preflight uses `SO_REUSEADDR` so compatible TCP
  `TIME_WAIT` state does not block restart, then independently rejects a stable
  exact or wildcard listener snapshot even when bind succeeds. An open port
  alone is insufficient; a wildcard listener never satisfies an exact endpoint.
- Endpoint acquisition never signals an existing service to resolve collision or
  ownership mismatch. A live service that is not exactly reusable requires an
  explicit `down` before replacement.
- An endpoint-less service makes no addressability claim. Placeholders toward it
  are invalid in every scope, its probes must be invocations, and its start
  closure must not attest `network-listener`. This is the deliberate scope limit
  of PORT-1, not an ownership bypass.

## Registry, state, and processes

- **REG-1 / REG-ORDER-1 / LIVE-1:** one transactional per-slot SQLite registry
  owns durable shared mutable state and a total per-slot event order. Endpoint
  locks are transient startup coordination, not another registry or semantic
  authority. Liveness is reconciled against OS process identity before it is
  reported; registry evidence alone is not a liveness oracle.
- **GC-1 / GC-2:** cleanup is idempotent, crash-safe, path-confined,
  marker-gated, lease-gated, process-gated, and policy-gated. Explicit purge
  relaxes only the protected/persistent policy gate; confinement, marker,
  live-lease/process, and registry gates remain unconditional. The cleanup target
  itself cannot be a symlink; symlink entries inside an owned tree are unlinked
  without being followed.
- **PROC-1..3 / PROC-CAP-1:** every spawned process belongs to a runtime-owned
  process group, cancellation reaches the whole group, and a long-lived process
  counts as started only after its registry process record exists. Admission
  fails when a service requires stronger containment than the host supports.
- **REDACT-1:** runtime-owned persistent output is redacted before write,
  including captured child output, summaries, registry payloads, and runtime
  error JSON. Resolved secrets exist only in runtime memory and hermetic child
  environments. Files and sockets written directly by a child are outside this
  guarantee.

## Output and failure contract

- **UPGRADE-1:** the Nix-only `upgrade` flake app derives its checked
  documentation report from the adopter's old and candidate locked Nixfied
  sources, comparing only `README.md` and regular files under `docs/`. The
  report is framed on stdout; status, warnings, and Nix diagnostics are on
  stderr. Checked mode performs candidate model preflight before changing
  project wiring, preserves `nixfied.nix`, and leaves `flake.nix` and
  `flake.lock` unchanged when lock resolution or model preflight fails.
  `--plan` performs the same inspection without mutation. Source identities are
  reported as readable type, original source, revision, and NAR hash fields
  rather than raw lock JSON. A successful apply reports candidate verification,
  whether the upgrade was applied, changed or unchanged project wiring,
  preserved `nixfied.nix`, and the post-upgrade validation commands; those
  commands are guidance and are not run by `upgrade`. `--no-lock` is an
  explicit URL-only mode that reports documentation and candidate verification
  as skipped. This surface is Nix-only and does not enter `model.json`,
  `runtimeAbi`, or Rust runtime behavior.
- `run` defaults to the human `summary` projection: progress, pass/fail summary,
  and evidence pointers on stderr, with stdout empty. `--json` is the stable
  structured projection; `--both` explicitly emits both. Captured child output
  remains in redacted log files and is never replayed inline. There is no `logs`
  control command.
- JSON fields, text projection tokens, error codes, and exit classes are public
  for the current exact ABI. Their authoritative inventory is the capability
  descriptor, and the runtime tests enforce agreement with the typed Rust enums.
- Admission failures and execution failures remain distinct. Once admission has
  succeeded, task, lifecycle, and dependency failures use their execution-class
  codes; a later `MODEL_ADMISSION` is a phase leak.

## Definitional boundaries

The following are product redefinitions, not backlog items: an additional
manifest envelope, a dynamic runtime adapter protocol, multi-host execution, a
required daemon, central log aggregation, UI or dashboards, non-`fail` port
policies, richer inter-service DAGs, cross-reference memoization, per-tool
effects, and environment membership.

## Changing the contract

A contract change must be explicit and atomic:

1. Record every model/runtime contract change in the capability descriptor so
   the runtime ABI rotates. This includes wire data, admitted vocabulary,
   behavioral semantics, hidden runtime command surfaces, output schemas, and
   error vocabulary. Nix-only flake-app changes do not enter the descriptor
   unless they also change emitted model data or runtime behavior.
2. Update the derived ABI snapshot and confirm Nix and Rust compute the same
   value. Change numeric model, ABI-base, or toolchain versions only when their
   defined semantics require it.
3. When the model seam is affected, change the Nix producer and Rust consumer
   together, including structural validation and fail-closed admission.
4. Update this contract, relevant rationale or derivation documentation, and
   focused tests/golden vectors, and delete the superseded implementation,
   fixtures, and documentation in the same transition.
5. Run the appropriate gates from `DEVELOPMENT.md`.
