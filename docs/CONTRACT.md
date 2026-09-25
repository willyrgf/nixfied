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
  metadata, declared service contracts, state policy,
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

- A session owns every service it starts through final teardown. Tasks in its
  graph share those services; completion of one leaf does not release them.
  Services stop when the session finishes, independently of application-data
  retention. There is no standing service handoff or cross-session borrowing.
  `serviceLifetime` is not a manifest field; authored legacy values reject.
- A writable registry owns the exclusive slot guard. Competing owners reject
  before application-state mutation or child spawn. `ps` reads a coherent
  registry snapshot and observes process liveness without acquiring ownership
  or changing stored records. An absent registry yields an empty process list.
- The state base, the slot's registry directory, and the application root must
  be on a local filesystem. A known network filesystem (Linux: NFS, SMB/CIFS,
  AFS, Ceph; macOS: NFS, SMB, AFP, WebDAV) refuses with `STATE_UNWRITABLE`
  before any lock or mutation. Other filesystems are assumed to provide local
  `flock`, `rename`, and `fsync` semantics; they are not detected.

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
  Application data lives at `data/project/environment/slot` beneath the state
  base. Coordination lives at `registry/project/environment/slot`; run evidence
  lives beneath its `runs/runId` directory. These trees are disjoint for every
  legal project name, including `data` and `registry`. Application cleanup
  accepts only the selected application root and preserves run evidence.
- **SOURCE-1:** runtime operations observe source only through declared
  `codebaseId`s. `live-workspace` roots resolve from the invocation root;
  immutable `snapshot` and `flake-input` roots resolve from the Nix store path in
  `sourceIdentity`, independent of the current working directory.
- Host-absolute placement never enters `manifest.json`; Rust materialises host paths
  during admission and execution.
- **SVC-ID-1:** service process ownership belongs to one session; later sessions
  start fresh processes after interrupted-predecessor cleanup.
- **PORT-1:** when a service declares endpoints, startup is serialized by a host
  endpoint lock and readiness requires exact kernel-observed ownership of every
  endpoint. Exact-bind preflight uses `SO_REUSEADDR` so compatible TCP
  `TIME_WAIT` state does not block restart, then independently rejects a stable
  exact or wildcard listener snapshot even when bind succeeds. An open port
  alone is insufficient; a wildcard listener never satisfies an exact endpoint.
- Endpoint acquisition never signals an existing service to resolve collision or
  ownership mismatch. A conflicting listener rejects startup.
- An endpoint-less service makes no addressability claim. Placeholders toward it
  are invalid in every scope, its probes must be invocations, and its start
  closure must not attest `network-listener`. This is the deliberate scope limit
  of PORT-1, not an ownership bypass.

## Registry, state, and processes

- **REG-1 / REG-ORDER-1 / LIVE-1:** one transactional per-slot SQLite registry
  owns durable shared mutable state and a total per-slot event order. Endpoint
  locks are transient startup coordination, not another registry or semantic
  authority. Liveness is reconciled against OS process identity before it is
  reported; registry evidence alone is not a liveness oracle. There are no
  service leases, heartbeat or expiry transitions, owner tokens, or borrower
  counts. Startup intent is recorded before prepare; endpoint rows are recorded
  atomically with their owning process, never as ownerless reservations. The
  declared service name belongs to its process record; there is no reusable
  service table or mutable service identity registry. A service-instance
  reference identifies one run and declared service name; it does not hash
  endpoint, state, runtime, or target configuration. Startup rejects unresolved
  predecessor process evidence before prepare. Slot cleanup lifecycle events
  identify the declared service without inventing a process instance.
- Session execution outcome has one live writer: the session finalizer records
  `succeeded`, `failed`, or `canceled` before output delivery and resource
  finalization. Task and service transitions record only local evidence. A
  known execution outcome is immutable; late delivery, teardown, or cancellation
  failures do not rewrite it. An exclusive recovery successor records
  `interrupted` only for an unknown predecessor outcome. Recovery validates all
  session records before writing any interruption evidence.
- Session finalization has one writer. After stopping services, the live owner
  applies the application tree's own retention only when every service stop
  settled and no process or endpoint evidence remains active: `run-scoped` data
  is deleted through the marker-last protocol and `persistent` data is retained.
  Only then does it commit `finalization = complete` with a `run.finalized`
  event. Unsettled processes, refused retention, or a failed deletion retain
  data, keep finalization `unfinished`, attempt a
  `run.finalization-unfinished` event, and fail the command. Task failure,
  timeout, and cancellation never change retention.
- Every mutating command first performs exclusive predecessor recovery:
  record unknown outcomes as interrupted, stop recorded process obligations,
  resume a pending deletion, apply the predecessor tree's own retention (the
  marker, never the new manifest, authorizes deletion), then complete every
  unfinished predecessor's finalization with a `run.recovered` event. Recovery
  never resumes tasks or adopts services; any unsafe step refuses and blocks new
  work. `clean` reports a deletion performed by that recovery.
- Every process record carries explicit `ownership`: `unresolved` from
  registration until the owner (or an exclusive recovery successor) proves the
  process, its group, its tracked descendants, and its captured writers are
  gone; then `settled`. Active statuses are always unresolved. A contained and
  capture-settled terminal records `settled`; an escape or unsettled capture
  keeps `unresolved` even after the leader exits, with or without endpoint
  evidence. Recovery settles a row only after its leader, its process group
  (when the leader PID is gone), and its tracked descendants are gone, waiting
  up to one second after termination; a present leader PID with another start
  identity is PID reuse, so that group is not signaled. Recovery never records
  a capture outcome for a predecessor's source. Deletion, state preparation, and new
  service startup refuse while any obligation is unresolved. `ps` reports
  `ownership`. A task or probe interrupted by a session failure after
  containment, reaping, and capture settlement records a terminal status and
  settles.
- Service release after durable registration and the startup grace period
  observe every already-started service as well as cancellation. A failure of
  any owned service, including a prepare-only dependency during preparation,
  withholds release of every further workload.
- Every process record stores the stop signal, stop timeout, and containment
  (`process-group` or `process-tree`) needed to terminate it; tasks and probes
  record SIGTERM, 1,000 ms, and their process group. Recovery terminates with
  these recorded facts, capped by the command timeout, never with a newly
  supplied manifest.
- Each session claims its `runs/<runId>` directory exclusively; an existing
  directory is a run identity collision and refuses before any session fact is
  published. The diagnostic source is created only after the run record that
  names it commits. After source registration closes, no process may register
  a new source.
- Service teardown observes pending exits before recording stop intent; an exit
  observed then remains an unexpected failure. Before it signals each service,
  ordered teardown observes every remaining service, so a service that exits
  while another stops, or during canceled teardown, settles as failed from its
  own exit and never as stopped or canceled. The stop lifecycle start event is
  the durable stop intent and precedes every stop signal; its recording failure
  still contains and reaps the service. An unexpected service exit, including
  status zero, fails the session with `DEPENDENCY_UNAVAILABLE`.
- `run --daemon` places the same session in the background. The launcher
  validates arguments (it rejects `--output`), allocates the immutable run ID,
  and spawns the hidden `__session-owner` mode once in a new OS session with null
  stdio; it holds no slot guard or registry writer. The owner performs the same
  admission, acquisition, recovery, and establishment as a foreground run,
  rejects workloads that inherit interactive stdin, and checks for launcher
  abandonment before slot acquisition, after predecessor recovery, and
  immediately before committing its run record. The committed run
  record is the establishment; the owner then replies with
  `{runId, runDir, logsDir}` and continues independently with no terminal
  presenter. The launcher prints that acknowledgement and exits 0: it means an
  established session, never readiness or task success; later failures belong
  to the session's recorded outcome. A complete rejection reply reports the
  owner's pre-establishment error and exit code with no new workload. EOF,
  timeout, or a malformed reply is `LIFECYCLE_FAILED` with the original `runId`:
  the launch outcome is uncertain. A termination signal to the launcher
  half-closes its channel (abandonment) and waits up to 10 s for a conclusive
  reply: a rejection is reported, and an establishment that won the race is
  canceled through that session's own FIFO; both exit `CANCELED`. If that
  cancellation request reaches no owner, the launcher reports
  `LIFECYCLE_FAILED` with the acknowledged `runId` and `runDir`. A rejected,
  abandoned, or uncertain owner is reaped within one second when it exits.
  Reply failure after the commit never cancels the session; `down` and signals
  remain its cancellation inputs.
- Each session creates a private FIFO named `control` (with `mkfifoat` through
  held `runs` and session directory descriptors that are never followed as
  symlinks and must be private to the effective user) in its never-reused
  `runs/<runId>` evidence directory before publishing its run record, opens a
  reader and a separate keeper writer (both close-on-exec), and removes the
  endpoint while still holding the slot. Any byte requests cancellation; a
  scoped receiver only sets the session's cancellation token. `down` selects the
  newest unfinished session once, opens that endpoint nonblocking without
  following symlinks, verifies a private FIFO, and writes one byte. It then
  observes that selected session until its finalization completes (reporting
  `canceledRunId`) or its owner releases the slot. A missing reader, missing
  endpoint, or broken pipe means no live owner; `down` then acquires the slot
  and performs predecessor recovery. `down` never signals the owner or a
  process group of a live session and never targets a successor; if the owner
  keeps the slot past `--timeout-ms`, it fails with `LIFECYCLE_FAILED` without
  further action. Ordinary SIGINT/SIGTERM/SIGHUP remain cancellation inputs.
- A task's observed execution outcome and exit code commit atomically with its
  observation event before containment and capture settlement. That observation
  does not grant completed output evidence. A later capture failure
  preserves the observed result; if it prevents remaining graph nodes from
  executing, the enclosing session fails. Repeated or mismatched observations
  reject without rewriting the prior result.
- Application-data compatibility belongs to the application and user. There is
  no `stateEpoch` declaration, manifest field, or marker field. Configuration
  changes update provenance without deleting data; application startup failure
  does not authorize deletion. Marker version 3 rejects old marker shapes.
  Existing persistent retention cannot be silently weakened during state
  preparation. Exact framework ABI and schema checks remain mandatory.
- `nixfied.state.persistence` is the sole application-data retention policy;
  the removed `cleanupPolicy` option and manifest field reject. The marker
  records the tree's persistence and a runtime-generated `dataGeneration`,
  preserved by provenance refresh. A fresh marker is published atomically
  (durable temporary file, rename, directory sync). An unmarked tree that holds
  only temporaries of an interrupted marker publication is fresh; those
  temporaries are removed before the marker is published, and any other
  unmarked content refuses. Marker version 3 rejects the
  previous marker shape; old state requires the matching old runtime or an
  explicit offline preservation procedure before upgrade.
- Exclusive predecessor recovery precedes state preparation regardless of
  manifest provenance. State preparation rejects unsettled process or endpoint
  evidence before marker inspection or mutation and never signals processes.
  Recovery does not exempt processes whose manifest hash matches the new run.
- Finalization is distinct from execution outcome and output sealing. A complete
  finalization requires a known outcome; a known outcome alone leaves finalization
  unfinished. Failed result/event transactions publish neither fact. Resource
  completion requires independent process, capture, and data-cleanup proof.
- **GC-1 / GC-2:** cleanup is idempotent, crash-safe, path-confined,
  marker-gated, slot-owned, process-gated, and persistence-gated. `run-scoped`
  data permits ordinary deletion; `persistent` data requires explicit purge,
  which overrides retention only. Confinement, marker, live-process,
  slot-ownership, and registry gates remain unconditional. The target is derived
  from the held slot's identity; state preparation and cleanup open it from the
  slot guard's held state-base descriptor, never by resolving its path again,
  and through directory descriptors without following symlinks; entries inside the owned tree are unlinked relative to their held
  directory without being followed, truncated, or crossing a nested mount.
  A directory on another device or, on Linux, any mount root reported by
  `statx` (including a bind mount of the same filesystem) refuses; so does a
  tree nested more than 128 directories deep. A refused step keeps the intent
  pending.
- Deletion commits one pending intent (operation ID, relative target, data
  generation, marker snapshot, purge authorization, observed root identity) and
  its event before any destructive effect. Payload removal, marker removal, and
  root removal are each followed by a directory `fsync`; completion commits last.
  At most one deletion is pending per slot. A pending intent is resumed with the
  same operation ID and authorization before any new generation, marker
  admission, or provenance refresh: an absent root completes it, a matching
  marked root resumes deletion, and an empty markerless root is removed. A
  markerless nonempty root, a replaced root, or a different generation refuses
  without deletion. A failed step leaves the intent pending with a
  `cleanup.attempt-failed` event. A deleted generation reappearing refuses as
  contradictory history, and state preparation refuses to adopt it. An absent
  root with nothing pending reports
  `result: absent` without attributing it to an older operation. These
  barriers support process-death recovery; host power-loss durability is not
  claimed.
- **PROC-1..3 / PROC-CAP-1:** every spawned process belongs to a runtime-owned
  process group, cancellation reaches the whole group, and a long-lived process
  counts as started only after its registry process record exists. Admission
  fails when a service requires stronger containment than the host supports.
- Service containment evidence is refreshed on the execution thread at liveness
  checkpoints. Ready-service checks include descendant escape detection, and the
  startup grace period polls containment and cancellation. No background process
  scanner owns or updates this evidence.
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
  output and uses failed-start settlement. A settlement error takes precedence
  and retains the lifecycle failure as its cause. Readiness transfers the owned
  starting handle into an owned ready handle; neither transition detaches the child.
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
  stderr, both presented live. It emits no runtime JSON metadata to stdout.
  `summary` presents runtime diagnostics and the live output of executed tasks,
  preparation, and services on stderr, one `[label] ` (stdout) or
  `[label:err] ` (stderr) prefix per line; stdout stays empty. `both` adds the
  final structured result on stdout. `json` presents no live logs. Displayed
  bytes establish no result: callers must check the process status; accepted
  child exit codes still produce a successful run. Composite selections, missing/unknown/
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
  evidence. Repeated prepares and prepare/root
  overlap retain separate files even when their task IDs and step paths match.
- Live output is presented by one command-owned read-only helper, the hidden
  `__presenter` mode of the runtime binary, launched with null stdin, a cleared
  environment, the caller's stdout/stderr, and one private socket; it inherits
  no locks, secrets, database writers, or cancellation FIFO. The session owner
  never writes caller streams while it has duties: runtime progress lines are
  retained in the run's `diagnostics.log` source. Each process registers its
  source label, presentation (`selected`, `shown`, or `hidden` for replace-on-retry
  probe logs), and run-relative stdout/stderr paths before release. The helper
  discovers sources in short read-only transactions, tails them by run identity
  and offset with bounded per-stream queues, and treats temporary EOF as
  progress. Human rendering bounds unterminated lines to 8 KiB fragments. A
  full stream queue holds back only the sources routed to that stream; other
  sources and the other stream keep flowing.
- Finalization never waits for delivery: after teardown, retention, and
  finalization, the owner closes source registration, writes the run summary
  and footer, closes the diagnostic writer, and publishes the output seal only
  when the run summary and diagnostic writes succeeded and every source recorded
  a checked capture outcome (`complete`,
  `incomplete`, or `unknown`); then it releases the slot. The command then lets
  the helper drain the sealed sources with no default deadline. A termination
  signal received during the drain ends it; a signal that already canceled the
  session, or a remote `down`, does not truncate the retained final output. Delivery is
  command-local: a failed, interrupted, or unconfirmed (unsealed) delivery of a
  successful session fails the command with `OUTPUT_PROJECTION_FAILED` without
  rewriting any session record; a canceled session stays `CANCELED`. A slow
  healthy reader is never truncated; a stalled reader cannot delay settlement
  or slot release. Displayed safe prefixes never upgrade incomplete capture.
- Execution checkpoints poll completed capture workers without waiting for open
  streams. Worker failure stops further execution, including a task with no
  deadline. Checked results remain owned through shutdown; a successful poll
  alone grants no completed capture outcome.
- Task, probe, and service capture always uses pipes, including without
  secrets, so service writer closure is checked rather than inferred from the
  leader's exit. The redactor emits every byte whose interpretation cannot
  change and holds back only the suffix that is still a proper prefix of some
  secret; results equal whole-buffer longest-first replacement for every
  chunking.
- Bounded capture shutdown: After
  containment and reap attempts, both stream workers receive one absolute
  shutdown deadline 1,000 ms away. Workers check control and expiry before reads,
  poll for at most 10 ms, and read at most 8 KiB per iteration. Only actual EOF
  completes capture. Expiry discards the undecided redactor tail, closes both
  evidence writers, and retains safe prefix files with an `incomplete` capture
  outcome and without completed task evidence. Both workers are joined even when either fails. This bound excludes
  blocked regular-file writes/flushes and OS scheduling; it proves neither that
  an escaped process died nor that the runtime discovered every descendant.
- Incomplete capture reports `SECRET_LEAK_BLOCKED` with the fixed message
  `captured stdout did not reach EOF before shutdown deadline` (or `stderr`).
  This reports inability to prove completed capture, not an observed secret leak.
  Containment/reap failure remains primary; capture errors follow in stdout then
  stderr order, before subordinate task outcomes. Capture failure outranks task
  and projection outcomes. Terminal service cleanup uses the same bounded
  shutdown; service capture remains owned until its workers settle.
- Task and probe child completion share containment, reaping, and capture
  ordering. Tasks record their process after spawn and before completion; a
  recording failure still consumes the child through cleanup. Cancellation and
  timeout intent are recorded before any signal, and intent-recording failure
  cannot bypass containment, reaping, or capture shutdown.
- Runtime errors may carry a non-recursive `causes` array. Projection failures
  use typed redaction-safe `details.projections` entries, whose fields are
  rendered by `nix run .#docs -- api record output-schema/runtime-error-projection`.
  Captured bytes, secrets, and raw OS messages are never serialized.
  Containment, registry, ownership, and state failures take precedence over
  `OUTPUT_PROJECTION_FAILED`, which takes
  precedence over task outcomes. Post-admission failures use lifecycle,
  registry, state, or selection codes; `MANIFEST_ADMISSION` is never a late phase
  projection.
- Finalization retains the first error among equal-priority failures. An incoming
  lower/equal-priority error appends its existing causes, then its safe root cause.
  A higher-priority error becomes primary, retaining its own causes before the
  previous primary's causes and safe root cause. Finalization records its own
  cancellation observation at most once across its teardown checkpoints;
  distinct errors may still carry distinct cancellation causes. Late cancellation
  continues service teardown and terminal registry settlement before slot release.
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
Delivery diagnostic paths deliberately use lossy display strings. Task, manifest and
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

## Internal workload gate

The private `__workload-gate <fd>` mode dispatches before ordinary runtime
admission, signal ownership, or output projection. It accepts a connected Unix
stream descriptor numbered at least 3; invalid invocation exits 125 silently.
It is not an adopter app or a manifest command. The owner must bootstrap it with
an empty environment, safe cwd, intended workload stdio, and no inherited slot,
database, or unrelated control descriptors.

The sole execution request is `NXG1`, a four-byte unsigned big-endian payload
length, then at most 1,048,576 bytes of JSON, terminated by write-half closure.
A ten-second absolute startup deadline includes the frame and terminator.
The exact JSON fields are `executable`, `args`, `env`, and `cwd`; strings are
arrays of Unix bytes, arguments are arrays of byte strings, and environment is
an array of name/value pairs. Executable and cwd must be absolute and NUL-free;
arguments and environment must be NUL-free. Environment names must be nonempty,
unique, and contain no equals sign. Unknown fields, malformed or partial frames,
trailing bytes, and invalid values reject before applying cwd or environment.

A valid request replaces the launcher with the executable, preserving PID,
process group, and workload stdin. The startup socket closes across exec. The
launcher clears the signal mask and restores standard workload signal dispositions.
A failed launch exits 125 and attempts one nonblocking failure byte on the socket:
1 for framing/deadline, 2 for invalid request, 3 for setup, or 4 for exec.
It prints no request values. Socket EOF alone proves neither successful exec nor
workload success; the owner must retain child-exit observation and startup errors.

The owner primitive validates and bounds the request before child creation,
requires an absolute launcher executable and held slot authority, and starts the
bootstrap from `/` with an empty environment. Only the selected child's descriptor
flags change to pass the socket; the parent's descriptors remain close-on-exec.
Its consuming registration-and-release operation calls the registration callback
before sending bytes, checks cancellation during bounded delivery/response waits,
and returns the child with any registration, delivery, or exec failure. Abandoning
an inert pending launch returns the child for checked reaping without permission.

Task execution, including preparation tasks, verifies the inert launcher's process
group and start identity and commits its task process/event before sending the
request. Service startup establishes its monitor and commits process/endpoint/event
evidence before release. Registration failure closes the gate and contains/reaps
the inert child. A service exec failure after registration retains the process
record and uses owned failed-start settlement. Permission delivery observes
cancellation; task startup also observes already-started services.

Exec probes use the same gate, committing their role, service attribution, process
identity and event before permission. Each attempt retains separate capture files
and execution evidence. Probe outcomes never settle the enclosing session. Probe
permission delivery observes cancellation and service liveness.
