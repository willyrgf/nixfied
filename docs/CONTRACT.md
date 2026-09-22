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
- **VERB-1:** framework project-app names are reserved; their publication
  declarations supply both validation and the reference inventory. Use
  `nix run .#docs -- topic discovery` for the related apps and
  `nix run .#docs -- api app` for exact publication names.
  `nixfied.surface.verbs` is an attrset mapping each
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
- **Reference discovery:** the Nix-only `docs` app reads the authoring and API
  reference from the same supplying framework source as `projectApps`. Root and
  project apps use `packages.<system>.docs`, whose supported interfaces are
  `bin/nixfied-docs` and `share/nixfied/reference/API.md`. Exact option/API queries,
  namespace listing, topic navigation and source provenance are read-only after
  realization: no Nix invocation, model admission, runtime or secret lookup.
  Topic queries compose ordered, explicitly selected sections from their authored
  documents, including each section's subsections, with concise entries from
  their owning definitions and related references. Cross-document composition
  preserves one authored source for each explanation.
  Forward and reverse navigation derive from checked relationships; a reference
  does not establish a behavioral prerequisite. Missing or ambiguous section
  headings, unknown or wrong-kind reference targets, and invalid topic selectors
  fail reference construction before packaging.
  Private lookup files are presentation data, not model artifacts. Outer flake
  evaluation still occurs; recovery from broken project evaluation uses that
  project's explicit supplying framework source, never an unpinned substitute.
- **Hermetic child environment:** every leaf, probe, and service start receives
  only its declared environment plus the runtime-owned `PATH` assembled from
  invocation tool roots. Runtime environment inheritance and append-to-inherited
  patterns are unsupported.

## Source, identity, and endpoints

- **Invocation substitution:** arguments and environment values follow the
  supported forms, endpoint scopes and primary-selection rules in
  [Endpoints and placeholders](ADAPTERS.md#endpoints-and-placeholders).
  Named endpoint references must resolve in their directly declared scope;
  bare task references use the first authored dependency, without skipping an
  endpoint-less service. `${secret:<id>}` requires a declared secret and is
  restricted to environment values. `stdin` is a null/inherit policy, not a
  template. Nix validation and independent runtime lowering reject invalid
  references before child execution. This does not introduce blanket rejection
  of arbitrary `${...}` child-program syntax.
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
  guarantee. A rejected non-UTF-8 environment-secret value is never formatted
  into an admission diagnostic; the error identifies the encoding failure.
- A task's `defaultOutput` is part of the model contract. `summary` is the
  normal default; `task-output` is valid only for a directly selected leaf.
  A leaf's default never propagates through a composite or a service prepare
  execution.

## Output and failure contract

- **UPGRADE-1:** the Nix-only `upgrade` flake app derives its checked
  documentation report from the adopter's old and candidate locked Nixfied
  sources, comparing only `README.md` and regular files under `docs/`. The
  report is framed on stdout; status, warnings, and Nix diagnostics are on
  stderr. Checked mode performs candidate model preflight before changing
  project wiring, preserves `nixfied.nix`, and leaves `flake.nix` and
  `flake.lock` unchanged when lock resolution or model preflight fails.
  `--plan` performs the same inspection without mutation and uses the same
  candidate preflight and status summary as apply, reporting `would change`
  where apply reports `changed`. Source identities are reported as readable
  type, original source, revision, and NAR hash fields rather than raw lock
  JSON. A successful checked operation reports candidate verification, changed
  or unchanged project wiring, preserved `nixfied.nix`, and the post-upgrade
  validation commands; those commands are guidance and are not run by
  `upgrade`. `--no-lock` is an explicit URL-only mode that reports
  documentation and candidate verification as skipped. This surface is Nix-only
  and does not enter `model.json`, `runtimeAbi`, or Rust runtime behavior.
- `run` resolves its output projection as explicit `--output <mode>`, then the
  selected root task's `defaultOutput`, then `summary`. The canonical mode domain
  is rendered by `nix run .#docs -- api command run`; the older mode-specific
  flags are not accepted. `task-output` is valid only for exactly one directly
  selected leaf: stdout is the selected task's exact redacted captured stdout,
  while stderr contains runtime diagnostics and the exact redacted captured
  stderr. It emits no runtime JSON metadata to stdout. Callers must check the
  process status before consuming replayed bytes; accepted child exit codes
  still produce a successful run. Composite selections, missing/unknown/
  repeated selections, invalid repeated output options, and invalid model
  defaults are rejected before state or child side effects. There is no
  `--json`, `--both`, `--summary`, or `--task-output` alias and no `logs`
  control command.
- Task-output replay happens only after capture and redaction complete, with
  both evidence files opened before terminal registry transitions or cleanup.
  The two streams replay concurrently with bounded buffers, preserving each
  stream's bytes and order but not cross-stream interleaving. Replay occurs for
  success, task failure, timeout, and cancellation, before service teardown,
  lease release, aggregate summary, footer, or final error projection. Cleanup
  and finalization continue after a replay failure.
- Runtime errors may carry a non-recursive `causes` array. Projection failures
  use typed redaction-safe `details.projections` entries, whose fields are
  rendered by `nix run .#docs -- api record output-schema/runtime-error-projection`.
  Captured bytes, secrets, and raw OS messages are never serialized.
  Containment, registry, lease, and state failures take precedence over
  `OUTPUT_PROJECTION_FAILED`, which takes
  precedence over task outcomes. Post-admission failures use lifecycle,
  registry, state, or selection codes; `MODEL_ADMISSION` is never a late phase
  projection.
- JSON fields, text projection tokens, error codes, and exit classes are public
  for the current exact ABI. Their authoritative inventory is the capability
  descriptor, and the runtime tests enforce agreement with the typed Rust enums.
- Admission failures and execution failures remain distinct. Once admission has
  succeeded, task, lifecycle, and dependency failures use their execution-class
  codes; a later `MODEL_ADMISSION` is a phase leak.

## Runtime error diagnostics

Read an error's `code` to identify the failure, then its redacted `message` and
`details` for the affected task, path, endpoint or ownership evidence. When
`causes` is present, read those additional failures alongside the primary error;
they are non-recursive diagnostics, not a sequence of recovery commands. Use
`nix run .#docs -- api error <code>` for the code's meaning and related topic.
The related topic explains the relevant ownership and operating constraints;
it does not promise that retrying is safe or will succeed.

`nix run .#docs -- api error` lists the exact error vocabulary. Native constructors
select the exit class; the command's native exit mapping produces statuses 12–38
for these failures. Neither the record declarations nor the error reference
select failure precedence or retry behavior.

Error `details` is open JSON and may be explicitly null. Fixed nested diagnostic
shapes have separate structural definitions.
Native producers place task evidence under `taskRun`, projection issues under
`projections`, and verified endpoint/owner evidence under `portConflict`.
`expectedRegistryIdentity` and `foundRegistryIdentity` share the diagnostic
rendered by `nix run .#docs -- api record local/RegistryIdentityDiagnostic`;
observed slot values stay signed, including negative corrupt values.
Replay diagnostic paths deliberately use lossy display strings. Task, model and
cleanup paths retain native path
serialization and its failure behavior.

Other detail keys are native producer facts, not a closed record or an executable
registry:

| Native producer | Additional detail keys |
| --- | --- |
| Slot selection | `slot`, `slotMin`, `slotMax` |
| Command/run orchestration | `command`, `compositeSteps`, `declaredTasks`, `environment`, `failedNodeId`, `failedService`, `logsDir`, `registryDir`, `registryPath`, `runDir`, `runId`, `runSummaryPath`, `slot`, `stateBase`, `stateRoot`, `stderrPath`, `stdoutPath`, `summaryPath`, `task`, `unknownTask` |
| Runtime error construction | `unsupportedFeature` |
| Service process ownership | `address`, `endpointId`, `port` |
| Registry identity checking | `mismatchedFields` |
| Registry I/O | `registryPath`, `registryDir` |

Native cause projection retains its existing safe-key allowlist and excludes raw
infrastructure text. Non-object details become an empty object in a cause. The
allowlisted `endpoint` and `nixfiedOwner` keys have no direct current production
insertion. Native `with_detail` retains its serialization-failure fallback. Run,
control and summary output keep their existing conversion, redaction, formatting
and write boundaries; the structural declarations perform none of those effects.

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

## Native command parsing

Runtime, installer and upgrade commands share checked syntax declarations.
`nix run .#docs -- api command` lists the declared commands;
`nix run .#docs -- api command <name>` lists each
argument's domain, initial value and visibility. Native parsers own acquisition,
repetition, errors and effects; declarations do not implement parsing policy.

Runtime collects UTF-8 arguments before selecting a command. No arguments selects
`check`; unknown commands reject. For a recognized command, either help spelling
anywhere after the command wins before option validation or model admission.
Signal installation still precedes help. Help does not materialize source/state
or execute declared children. Invalid UTF-8 fails during input collection even
when a help token is present.

Runtime consumes the next token as an operand even when it looks like an option.
Model and state-base paths use the last occurrence; a trailing bare flag clears
an earlier value. Missing model rejection and state-base environment fallback
follow parsing. Slots parse each occurrence as `u32`, timeouts as `u64`; the last
valid occurrence wins. Native integer parsing accepts leading plus and zero,
and rejects whitespace, negative values and overflow. Task repetition acquires
the next operand before checking duplication. Output repetition validates the
next mode before checking duplication, so a repeated invalid mode reports an
invalid mode and a repeated task without an operand reports a missing value.
Flags set their native state idempotently. Purge changes only the native cleanup
policy selection. Output defaults remain explicit mode, then selected task's
default, then summary. The early error projection scans output operands natively:
the last operand-bearing occurrence selects projection; a trailing bare output
flag does not clear it. Removed output aliases retain their specific rejection.

Installer collects OS strings. Missing or non-UTF-8 root command prints successful
root usage. Root `help`, `-h` and `--help` route natively. After `install`, either
help token anywhere wins before option validation. Other standalone invalid
UTF-8 yields `arguments must be valid UTF-8`; an invalid UTF-8 operand follows
the native missing-value error. Operands include empty and option-like strings;
all values use the last occurrence. Project metadata inference/validation and
file ownership checks follow parsing, before native scaffold creation.

Upgrade parses byte strings under `LC_ALL=C` sequentially. An earlier error wins
over later help. Empty and double-hyphen-prefixed operands reject; `--root --help`
is missing a root value, while `--root -h` consumes `-h`. Native command
substitution strips trailing operand newlines. Values use the last occurrence;
flags are idempotent. The empty initial URL preserves current selection, and
`--no-lock` updates the native inverse lock state. Source inspection, Nix calls,
preflight, plan/apply and transaction ownership remain native continuations.

These commands do not accept equals-form options, positional operands, short
clusters or an option terminator. Hidden model/state arguments remain documented
for framework and test integration; generated app wrappers supply the model.
