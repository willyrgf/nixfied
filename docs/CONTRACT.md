# Nixfied contract

This document is the normative product and behavioral contract for the current
Nixfied architecture, authoring surface, manifest/runtime boundary, and public
outputs. [`ARCHITECTURE.md`](ARCHITECTURE.md) explains why these constraints
exist; [`DEVELOPMENT.md`](DEVELOPMENT.md) explains how to work on the
implementation.

The runtime ABI's manifest-field and enum names, hidden runtime-command surface,
output-field vocabulary, and error vocabulary are inventoried in
[`runtime/crates/nixfied-manifest/capability.txt`](../runtime/crates/nixfied-manifest/capability.txt).
That authored descriptor is hashed identically by Nix and Rust to derive the
`runtimeAbi` suffix. The complete typed shape and validation live in the Nix
producer and `nixfied-manifest`; the descriptor records the vocabulary the runtime
understands, and its digest ensures a recorded manifest/runtime change rotates the
ABI. Nix-only library and flake-app surfaces are public integration API but stay
outside `runtimeAbi` unless they change emitted manifest data or runtime behavior.
[`DERIVATION_SPEC.md`](DERIVATION_SPEC.md) is separately normative for facts
derived from the task/service graph.

Do not weaken an invariant below without an explicit contract change and
coordinated implementation, documentation, and test updates. Rotate the ABI
when the manifest/runtime contract changes.

## Manifest and version boundary

- **MANIFEST-SEAM-1 / SINGLE-MANIFEST-1:** `manifest.json` is the only required semantic
  artifact. The generated `views/docs.md` human reference is a disposable
  projection and never independent authority. Do not add a required sidecar or envelope.
- **MANIFEST-ORIGIN-1:** normal admission requires `manifest.json` under the Nix store.
  `--allow-non-store-manifest` is an unstable framework test/development escape
  hatch, not an adopter path.
- **MANIFEST-CONTRACT-1:** the manifest carries the complete admission contract:
  generator and toolchain identity, runtime ABI, target, source policy, closure
  metadata, the inputs needed to derive layered service identity, state policy,
  and secret descriptors. Secret descriptors are references; secret values are
  never manifest data.
- **HASH-1:** the runtime computes `computedManifestHash` as SHA-256 over the raw
  manifest bytes. The manifest contains no self-hash.
- **ABI-1:** admission requires exact `runtimeAbi` and `toolchainId` matches.
  Backward compatibility imposes no design constraint: a deliberate contract
  change may break any prior manifest, runtime command, output, or error surface.
  The new contract replaces the old one; old contracts are rejected, never
  migrated, translated, or admitted through a compatibility fallback. The
  runtime ABI suffix is the capability-descriptor digest, computed identically
  by `nixfied-manifest::constants` and `nix/spec/constants.nix`.
- **PREPARE-1:** Nix realises every referenced closure before runtime start. The
  runtime verifies existence, executability, target, and declaration; it never
  builds missing closures.
- Admission is a global pre-spawn barrier. Invalid manifest origin, ABI, toolchain,
  target, source, closure, state policy, or unsupported host feature fails before
  any child process starts.

## Ownership boundary

- **SEAM-1:** `nixfied-runtime` never invokes `nix`, `nix-store`, `nix build`, or
  `nix eval`, and never imports Nix expressions.
- SEAM-1 governs the runtime binary. A declared child invocation may execute a
  Nix tool if the manifest supplies it. Framework compiler/install tests stay
  outside runtime tasks because their open-ended Nix builds and external fetches
  do not fit the bounded role of those framework tasks and blur test ownership,
  not because the child process is technically unable to run Nix.
- **RUNTIME-GENERIC-1:** Rust executes generic primitives and knows no service
  domain. Concrete adapters are Nix-side manifest generators. A domain-specific
  runtime need signals a missing generic primitive, not permission to specialize
  the runtime.
- **NIX-API-1:** typed Nix modules are the public integration and correctness
  layer. Project behavior compiles into generic primitives. Compilation consumes
  native bindings and validates executable intent independently of documentation
  references and unrelated output wire metadata. Invalid presentation rejects
  reference construction and release checks, without blocking valid manifest
  compilation. Release checks require both consumers and whole-inventory coverage.
- **SHELL-1 / NIX-1:** shell cannot own graph, validation, registry, liveness,
  summary, or cleanup semantics. Nix cannot own live supervision, cancellation,
  liveness, reconciliation, registry mutation, or cleanup.

## Task and service algebra

- Task invocation `timeoutMs` is optional. Absence (or explicit null) has no
  finite default; a supplied value must be a positive `u64`. Nix emits absent
  deadlines by omitting the field. Lowering and execution preserve the absence.
  Task waits remain cancelable and observe every started service, including
  transitive and preparation-only dependencies. An observed service exit,
  including exit zero, fails the running work. Endpoint substitutions remain
  limited to the task's declared dependencies.
- Preparation follows the same task deadline rule. Its completion precedes
  readiness probing; no implicit readiness or command timeout caps preparation.
  Probe attempts and graceful teardown retain their own finite lifecycle limits.
  Runtime operation timeouts do not create a task or composite-wide deadline.

- **KIND-2:** the manifest has exactly two semantic kinds: task and service. New
  adopter vocabulary must first be expressed as names over that algebra; a new
  schema kind requires proof that the algebra cannot represent it.
- **INVOKE-1:** invocations are inline anonymous structural values. There is no
  invocation registry or invocation ID in the wire format.
- **STATIC-1:** composites are fully applied static DAGs. Parameters,
  conditionals, retries, and loops belong in Nix-side expansion or inside an
  opaque leaf, not in the runtime contract.
- **DERIVE-1:** admission derives service requirements and execution ordering
  from graph inputs according to `DERIVATION_SPEC.md` before child effects.
  The manifest carries neither `servicesRequired` nor `operationBindings`.
  Nix independently checks graph feasibility and authored binding restrictions;
  those restrictions reject at authoring, not through a runtime authorization
  list. Operation IDs remain explicit inputs with native uniqueness checks.
  Independent vectors and behavioral tests prove derivation, replacing
  per-manifest equality checks. Effects remain authored attestations.
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
  are Nix-only app metadata: they are not emitted into `manifest.json` or
  `views/docs.md`, and do not enter `runtimeAbi`.
- **SURFACE-1:** hidden runtime commands are framework-owned and listed in the
  capability descriptor, not declared by an adopter in the manifest. The adopter
  declaration owns only its exported task-verb surface; unrelated custom flake
  apps remain ordinary Nix integration. `manifest-check` is the generated app name
  for the runtime's admission-only `check` command. Every runtime-backed control
  and exported task app accepts `-h` and `--help`; that help completes before
  manifest admission, source resolution, state materialisation, or execution. The
  generated `.#help` app prints a catalog constructed from the generated app
  definitions, sorted by name. It includes framework controls and exported verbs;
  separately merged apps and later metadata overrides are excluded. Catalog
  construction rejects invalid descriptions. It invokes no Nix commands, admits
  or executes no manifest, and stays outside `runtimeAbi`. `projectApps` accepts
  ordinary Nix module paths, functions, and attribute sets without requiring
  root-level declaration files; help works independently of the caller directory.
- **Reference discovery:** the Nix-only `docs` app reads the authoring and API
  reference from the same supplying framework source as `projectApps`. Root and
  project apps use `packages.<system>.docs`, whose supported interfaces are
  `bin/nixfied-docs` and `share/nixfied/reference/API.md`. Exact option/API queries,
  namespace listing, topic navigation and source provenance are read-only after
  realization: no Nix invocation, manifest admission, runtime or secret lookup.
  Topic queries compose ordered, explicitly selected sections from their authored
  documents, including each section's subsections, with concise entries from
  their owning definitions and related references. Cross-document composition
  preserves one authored source for each explanation.
  Forward and reverse navigation derive from checked relationships; a reference
  does not establish a behavioral prerequisite. Missing or ambiguous section
  headings, unknown or wrong-kind reference targets, and invalid topic selectors
  fail reference construction before packaging.
  Private lookup files are presentation data, not manifest artifacts. Outer flake
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
  restricted to environment values, including a prohibition in `run[0]`.
  `run[0]` selects a declared executable by its literal basename; only `run[1..]`
  and environment values are templates. `stdin` is a null/inherit policy, not a
  template. Nix validation and independent runtime lowering reject invalid
  references before child execution. This does not introduce blanket rejection
  of arbitrary `${...}` child-program syntax. Authored text is scanned once from
  left to right; recognized references nested inside unknown child syntax still
  resolve. Named payloads must be nonempty and contain no braces or nested `${`.
  Unclosed recognized references reject. Inserted secret values and host paths
  are opaque: they are never parsed or substituted again.
- **Relational admission order:** after target, source/secret, and closure host
  checks, lowering checks references and operation IDs; local invocation,
  endpoint, and effect coherence; task/sibling cycles; combined service cycles;
  carried operation bindings; carried service requirements; then slot capacity.
  All graph relationship errors, including `connectsTo`, are
  `MANIFEST_ADMISSION`, not structural `MANIFEST_INVALID` errors. Every declared
  task/service is checked, including unused graph components.
  Invocation checks visit services in canonical order, completing start, ready,
  and health in that order before proceeding to canonical task order. Lowering
  rejects non-relative cwd paths, parent traversal, and NUL bytes. Directory
  existence and canonical source confinement are rechecked at execution, including
  every probe attempt, because filesystem paths can change after admission.
- **Placement components:** project, environment, and run identifiers are
  nonempty single normal path components. Slash, NUL, `.`, `..`, and `${` syntax
  reject as `STATE_UNWRITABLE` before filesystem effects. Original bytes are
  checked before path normalization; Unix backslashes remain ordinary bytes.
  Layout is direct joining of project/environment/slot and runs/runId beneath
  the state base, with registry/project/environment/slot as a parallel tree.
- **SOURCE-1:** runtime operations observe source only through declared
  `codebaseId`s. `live-workspace` roots resolve from the invocation root;
  immutable `snapshot` and `flake-input` roots resolve from the Nix store path in
  `sourceIdentity`, independent of the current working directory.
- Host-absolute placement never enters `manifest.json`; Rust materialises host paths
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
- TCP and exec-probe waits and retry delays observe both the current service and all
  already-started services. Observation failure stops the probe and prevents
  further work. Checkpoints run between waits of at most 10 ms; this cadence is
  not a universal teardown bound, since host observation, capture shutdown,
  registry I/O and OS scheduling also take time.
- Each TCP probe attempt creates one nonblocking connection and polls that same
  socket until completion, cancellation, observation failure, or its existing
  attempt deadline. Polling never reconnects or consumes extra retries. A ready
  descriptor is checked for its socket error and peer association before success.
  Readiness still requires the independent endpoint-ownership observation.
- Readiness and health reobserve service liveness after each completed probe,
  before accepting readiness or settling probe timeout. A monitored service
  escape during a probe remains `PROC_ESCAPE`, including on the last attempt;
  endpoint evidence retains its existing failure precedence.
- Startup guards remain owned through a failed readiness transition and its
  cleanup. Initial health failure after committed readiness retains failed-service
  output and uses failed-start settlement. A failed standing commit also attempts
  owned failure cleanup; a settlement error takes precedence and retains the
  lifecycle failure as its cause. Only a successful standing commit relinquishes
  local child teardown ownership. Borrower finalization releases only its lease.
- **REDACT-1:** runtime-owned persistent output is redacted before write,
  including captured child output, summaries, registry payloads, and runtime
  error JSON. Resolved secrets exist only in runtime memory and hermetic child
  environments. Files and sockets written directly by a child are outside this
  guarantee. A rejected non-UTF-8 environment-secret value is never formatted
  into an admission diagnostic; the error identifies the encoding failure.
- A task's `defaultOutput` is part of the manifest contract. `summary` is the
  normal default; `task-output` is valid only for a directly selected leaf.
  A leaf's default never propagates through a composite or a service prepare
  execution.

## Output and failure contract

- **UPGRADE-1:** the Nix-only `upgrade` flake app derives its checked
  documentation report from the adopter's old and candidate locked Nixfied
  sources, comparing only `README.md` and regular files under `docs/`. The
  report is framed on stdout; status, warnings, and Nix diagnostics are on
  stderr. Checked mode performs candidate manifest preflight before changing
  project wiring, preserves `nixfied.nix`, and leaves `flake.nix` and
  `flake.lock` unchanged when lock resolution or manifest preflight fails.
  `--plan` performs the same inspection without mutation and uses the same
  candidate preflight and status summary as apply, reporting `would change`
  where apply reports `changed`. Source identities are reported as readable
  type, original source, revision, and NAR hash fields rather than raw lock
  JSON. A successful checked operation reports candidate verification, changed
  or unchanged project wiring, preserved `nixfied.nix`, and the post-upgrade
  validation commands; those commands are guidance and are not run by
  `upgrade`. `--no-lock` is an explicit URL-only mode that reports
  documentation and candidate verification as skipped. This surface is Nix-only
  and does not enter `manifest.json`, `runtimeAbi`, or Rust runtime behavior.
- `run` resolves its output projection as explicit `--output <mode>`, then the
  selected root task's `defaultOutput`, then `summary`. The canonical mode domain
  is rendered by `nix run .#docs -- api command run`; the older mode-specific
  flags are not accepted. `task-output` is valid only for exactly one directly
  selected leaf: stdout is the selected task's exact redacted captured stdout,
  while stderr contains runtime diagnostics and the exact redacted captured
  stderr. It emits no runtime JSON metadata to stdout. Callers must check the
  process status before consuming replayed bytes; accepted child exit codes
  still produce a successful run. Composite selections, missing/unknown/
  repeated selections, invalid repeated output options, and invalid manifest
  defaults are rejected before state or child side effects. There is no
  `--json`, `--both`, `--summary`, or `--task-output` alias and no `logs`
  control command.
- Every attempted task node, including service prepares, receives a run-local
  occurrence number starting at zero before execution. Completed evidence is
  retained once per occurrence, preserving task identity, `stepPath`, and order;
  attempts without terminal evidence may leave gaps. Logs are named
  `task.<occurrence>.stdout.log` and `task.<occurrence>.stderr.log`; task summaries
  are named `summary.<occurrence>.json`. Each file uses exclusive creation without
  overwriting or retrying a conflicting name. Log creation failure rejects before
  spawn with the existing redacted/unredacted creation error. Summary creation
  failure reports `STATE_UNWRITABLE` after completion and retains completed task
  evidence and any selected replay ticket. Repeated prepares and prepare/root
  overlap retain separate files even when their task IDs and step paths match.
- Task-output replay happens only after capture and redaction complete, with
  both evidence files opened before terminal registry transitions or cleanup.
  The two streams replay concurrently with bounded buffers, preserving each
  stream's bytes and order but not cross-stream interleaving. Replay occurs for
  success, task failure, timeout, and cancellation, before service teardown,
  lease release, aggregate summary, footer, or final error projection. Cleanup
  and finalization continue after a replay failure.
- Bounded task/probe capture always uses pipes, including without secrets. After
  containment and reap attempts, both stream workers receive one absolute
  shutdown deadline 1,000 ms away. Workers check control and expiry before reads,
  poll for at most 10 ms, and read at most 8 KiB per iteration. Only actual EOF
  completes capture. Expiry discards the undecided redactor tail, closes both
  evidence writers, and retains safe prefix files without completed task evidence
  or replay. Both workers are joined even when either fails. This bound excludes
  blocked regular-file writes/flushes and OS scheduling; it proves neither that
  an escaped process died nor that the runtime discovered every descendant.
- Incomplete capture reports `SECRET_LEAK_BLOCKED` with the fixed message
  `captured stdout did not reach EOF before shutdown deadline` (or `stderr`).
  This reports inability to prove completed capture, not an observed secret leak.
  Containment/reap failure remains primary; capture errors follow in stdout then
  stderr order, before subordinate task outcomes. Capture failure outranks task
  and projection outcomes. Terminal service cleanup uses the same bounded
  shutdown; successful standing explicitly transfers persistent relay ownership.
- Task and probe child completion share containment, reaping, and capture
  ordering. Tasks record their process after spawn and before completion; a
  recording failure still consumes the child through cleanup. Cancellation and
  timeout intent are recorded before any signal, and intent-recording failure
  cannot bypass containment, reaping, or capture shutdown.
- Runtime errors may carry a non-recursive `causes` array. Projection failures
  use typed redaction-safe `details.projections` entries, whose fields are
  rendered by `nix run .#docs -- api record output-schema/runtime-error-projection`.
  Captured bytes, secrets, and raw OS messages are never serialized.
  Containment, registry, lease, and state failures take precedence over
  `OUTPUT_PROJECTION_FAILED`, which takes
  precedence over task outcomes. Post-admission failures use lifecycle,
  registry, state, or selection codes; `MANIFEST_ADMISSION` is never a late phase
  projection.
- Finalization retains the first error among equal-priority failures. An incoming
  lower/equal-priority error appends its existing causes, then its safe root cause.
  A higher-priority error becomes primary, retaining its own causes before the
  previous primary's causes and safe root cause. Finalization records its own
  cancellation observation at most once across replay and teardown checkpoints;
  distinct errors may still carry distinct cancellation causes. Late cancellation
  continues service teardown, lease release, and terminal registry settlement.
- JSON fields, text projection tokens, error codes, and exit classes are public
  for the current exact ABI. Their authoritative inventory is the capability
  descriptor, and the runtime tests enforce agreement with the typed Rust enums.
- Admission failures and execution failures remain distinct. Once admission has
  succeeded, task, lifecycle, and dependency failures use their execution-class
  codes; a later `MANIFEST_ADMISSION` is a phase leak.

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
Runtime output structs, borrowed views and private storage are authored in Rust;
shared declarations describe wire fields and vocabularies. Independent literal
serialization tests preserve those bytes and native failure behavior.
Native producers place task evidence under `taskRun`, projection issues under
`projections`, and verified endpoint/owner evidence under `portConflict`.
`expectedRegistryIdentity` and `foundRegistryIdentity` share the diagnostic
rendered by `nix run .#docs -- api record local/RegistryIdentityDiagnostic`;
observed slot values stay signed, including negative corrupt values.
Replay diagnostic paths deliberately use lossy display strings. Task, manifest and
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

1. Record every manifest/runtime contract change in the capability descriptor so
   the runtime ABI rotates. This includes wire data, admitted vocabulary,
   behavioral semantics, hidden runtime command surfaces, output schemas, and
   error vocabulary. Nix-only flake-app changes do not enter the descriptor
   unless they also change emitted manifest data or runtime behavior.
2. Update the derived ABI snapshot and confirm Nix and Rust compute the same
   value. Change numeric manifest, ABI-base, or toolchain versions only when their
   defined semantics require it.
3. When the manifest seam is affected, change the Nix producer and Rust consumer
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
anywhere after the command wins before option validation or manifest admission.
Signal installation still precedes help. Help does not materialize source/state
or execute declared children. Invalid UTF-8 fails during input collection even
when a help token is present.

Runtime consumes the next token as an operand even when it looks like an option.
Manifest and state-base paths use the last occurrence; a trailing bare flag clears
an earlier value. Missing manifest rejection and state-base environment fallback
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
clusters or an option terminator. Hidden manifest/state arguments remain documented
for framework and test integration; generated app wrappers supply the manifest.
