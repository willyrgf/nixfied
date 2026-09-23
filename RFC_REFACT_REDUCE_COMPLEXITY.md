# RFC: reduce runtime complexity through explicit ownership

Status: implemented design, with a qualified reduction outcome. This document
preserves the accepted design and plan; current behavior is owned by the contract.
At completion (`9ba9a07`), authored Rust production was only 126 lines smaller
than the implementation baseline, while the repository was 2,361 lines larger
than the branch base. Ownership and correctness improved, but substantial
maintained-code reduction was not achieved. See the corrected
[outcome and evidence](RFC_REFACT_REDUCE_COMPLEXITY_PROGRESS.md).

Baseline: `0570aa3`, reviewed on 2026-09-23. The audit covered all 26 Rust files with at least 400 lines, plus their adjacent owners and tests. Those files contain 30,165 of the 36,093 physical Rust lines under `runtime/`. The total includes 22,754 authored source lines with embedded tests, 11,879 integration-test/helper lines, 817 generated lines, and 643 test-child lines. These counts include comments and whitespace and are a baseline, not a reduction quota.

## Purpose and governing constraints

Reduce maintained runtime code by deleting duplicate interpretations, impossible states, and competing owners of evidence. File splitting alone is not a reduction. Preserve independent proofs at the Nix/Rust, wire, SQLite, filesystem, and OS boundaries.

The governing authorities are [CONTRACT.md](docs/CONTRACT.md), [DERIVATION_SPEC.md](docs/DERIVATION_SPEC.md), [ARCHITECTURE.md](docs/ARCHITECTURE.md), [DEVELOPMENT.md](docs/DEVELOPMENT.md), and the authored [capability descriptor](runtime/crates/nixfied-manifest/capability.txt). They take precedence over this proposal until its explicitly identified semantic changes are accepted and updated atomically.

The reviewed architecture already has useful foundations: a closed executable-task enum, immutable execution-manifest accessors, typed identifiers and loopback hosts, process termination primitives, a replay ticket, shared manifest fixtures, and structural declarations. Extend these owners. Do not replace them with a typestate framework, generic executor, transition engine, scenario DSL, invocation registry, arena, runtime cache, or new required artifact.

Most large runtime tests exercise facts that Rust cannot prove: process identity, concurrent state transitions, damaged SQLite, accepted wire bytes, redaction, ordering, and filesystem confinement. Delete tests only after naming the type guarantee or remaining independent behavioral proof that replaces them.

## Decisions at a glance

| Existing problem | Chosen owner and representation | What disappears |
| --- | --- | --- |
| Mutable loaded/admitted proof data | Private loaded document; separate control and run admissions | Rechecking already-proven identities and missing-source run branches |
| Duplicate graph meaning | One runtime logical graph derivation, then slot binding | Raw service-union derivation and connectsTo-only cycle algorithm |
| Repeated invocation resolution | One resolution pass producing executable payload and local binding evidence | Per-check executable/tool scans; late template parsing |
| Flag-driven service ownership | Borrowed service or owned service; concrete starting/ready transition | Borrowed child access, correlated child/guard flags |
| Task/probe child mechanics | One bounded owned-child operation with caller cancellation hook | Duplicate spawn/wait/contain/reap loops |
| Repeated leaf orchestration and evidence copies | One node runner and one evidence collection | Prepare/root execution loops and mutable output mirrors |
| Consumers decoding SQLite | Typed registry readers and explicit transactions | Duplicate endpoint decoding, actionable predicates, event context copies |
| Fixed layout interpreted as templates | Direct validated path composition | Template constants, variable environment, interpreter |
| Overbroad outcomes | Error-returning classifier and terminal outcome enum | Impossible success/status branches |
| Split failure/cancellation ownership | One primary error with flattened causes; record-once observation | Secondary cause accumulator and duplicate cancellation |
| Nonatomic schema construction | Immediate transaction owning classify/create/version/commit | Version gap and new-database self-checks |
| Duplicate redaction/output mechanics | One byte scanner; existing safe output constructors | Duplicate matcher and projection formatting |
| Fixture state coupling | Existing cohesive fixtures, configure before lowering | Mutable raw-plus-derived fixtures and forwarding wrappers |
| Approximate inventories | Existing declarations plus independent wire/ABI proofs | Token-search inventories and redundant enum sentinels |

## Review disposition and engineering boundary

All 14 review conclusions are accepted with the refinements below. Admission constructor closure preserves current rejection order until the separately identified graph/template/placement ABI cutover. Sections 2–4 now specify temporary capacity summaries, raw invocation traversal/nested grammar, and health-failure settlement. Section 5 replaces the invalid group-cleanup-implies-EOF premise with required bounded capture. Section 6 fixes confirmed repeated-prepare file truncation and removes evidence shuttles. Sections 7–14 retain query-specific checks, exact lexical rejection, listener/startup evidence, cause order, transactional classification, distinct diagnostic mappings, adversarial fixtures, and independent wire/ABI proofs.

This authorizes a concrete engineering plan, not a claim that these are all mechanical changes. The nonblocking capture protocol adds necessary correctness code and must be measured separately from simplification. The 1,000 ms drain policy and occurrence-based path spelling are explicit proposed public-behavior decisions; implementing them requires the atomic descriptor/docs/test updates below, not a new design decision by the engineer. There are no unresolved central design choices in the accepted work; the separately named platform investigations remain deferred.

## Target flow and proof ownership

```text
raw bytes + provenance
    -> origin check
    -> decode + exact identity + structural checks
    -> target/host check
    -> run/check: source resolution, secret resolution
       control: secret-reference validation only
    -> closure checks
    -> relational lowering + all-graph proof + carried-fact comparison
    -> all-task/all-slot capacity proof
    -> private control admission, or run admission with required source/secrets
    -> selected borrowed logical plan + slot binding
    -> host placement/registry/state effects
    -> acquisition, task execution, replay, teardown, final projections
```

No executable admission is published midway through this sequence. Helpers may construct private local candidates but cannot return them as admitted execution data. `check` uses run admission: its source, secret, and closure guarantees remain the same as `run`, without starting a child. `ps`, `down`, and `clean` use control admission and remain usable outside a moved/deleted workspace without fetching secret values.

The sequence retains target, source, secret, and closure host-check ordering. Moving graph checks, consolidating invocation resolution, and changing placement validation can change which rejection wins for inputs with multiple faults; the semantic cutover below explicitly defines and tests those differences. Raw origin rejection remains before parsing even for malformed non-store JSON; raw-byte hashing and error provenance remain intact. Source/secret/closure failures are never converted into generic parsing failures.

## 1. Close the admission boundary

Sources: [manifest_loader.rs](runtime/crates/nixfied-runtime/src/manifest_loader.rs), [admission/mod.rs](runtime/crates/nixfied-runtime/src/admission/mod.rs), [validation.rs](runtime/crates/nixfied-manifest/src/validation.rs), [execution/types.rs](runtime/crates/nixfied-runtime/src/execution/types.rs).

**Root cause.** `LoadedManifest` and `Admission` expose mutable proof fields. `lower(&Manifest)` documents a prerequisite that its signature does not enforce. Run/control selection is a boolean paired with an optional source and empty secrets. Callers defensively reconstruct phase completion.

**Design.** Keep the generated wire DTO as a wire DTO. Add a private-field owning `ValidatedManifest(Manifest)` wrapper in the manifest crate, constructed through `TryFrom<Manifest>`; it exposes only immutable access. Replace the single-implementation `Validate` trait with that concrete boundary. The loaded document owns this wrapper and private provenance fields, with no mutable manifest accessor or unchecked deserializer. Its constructor decodes and validates once. Tests traverse the same boundary rather than fabricating a proof token.

`lower` accepts `&ValidatedManifest`, not an arbitrary `Manifest`. It returns the existing immutable `ExecutionManifest` only after relational proofs finish. The wrapper moves the decoded DTO rather than cloning it. The runtime cannot mint an unchecked wrapper because its field/constructor is private to the manifest owner. No second manifest clone, self-referential structure, or revalidation-on-access is needed.

Use private-field `ControlAdmission` for common admitted metadata/execution data, and `RunAdmission { common: ControlAdmission, source: AdmittedSource, secrets: ResolvedSecrets }`. Common here means source-independent admitted facts, not a control command that must run first. Constructors share concrete private helper functions while preserving the phase sequence above. Run consumers require `&RunAdmission`; recovery consumers require `&ControlAdmission`. Replace `require_source()` with infallible access on run admission. Borrow a small shared run context where existing service/task callers need the same source, secret, and identity facts.

The intended API boundaries are concrete (names are illustrative; fields remain private):

```rust
// manifest crate: structural proof only; no host or runtime behavior
impl TryFrom<Manifest> for ValidatedManifest { /* checked construction */ }

// runtime: each entry point owns loading, origin, and its admission sequence
fn admit_run(path: &Path, context: &AdmissionContext) -> RuntimeResult<RunAdmission>;
fn admit_control(path: &Path, context: &AdmissionContext) -> RuntimeResult<ControlAdmission>;
fn lower(document: &ValidatedManifest) -> RuntimeResult<ExecutionManifest>;
```

An admission object owns the loaded document/provenance it needs; callers do not pass a second independently mutable manifest alongside it. Keep these APIs in existing owners rather than adding a shared service container.

Convert correlated task, probe, and secret wire alternatives once at their owning boundary. Retain wire presence/default/null behavior; generated structural records need not become a general sum-type code-generation feature. Runtime task/probe alternatives already exist and should be reused. A native validated secret-source alternative belongs to secret admission. Remove duplicated codebase and ABI checks only when all callers require the closed checked input. Filesystem existence, executability, canonical confinement, and process identity remain temporal checks.

Own raw origin checking in the load/admit entry point, removing the second origin check and its duplicate helper. Attach provenance once around the admission operation instead of passing the loaded document through each helper solely to attach diagnostics. For source paths, acquire the live or immutable root separately, then reuse one confined-directory resolver. Return the canonical path already proved rather than a boolean that forces another canonicalization; canonicalize the store root once within that admission. Preserve declared-executable containment separately from canonical store containment so valid buildEnv symlinks remain accepted. These are local observations, never a cache promising that a mutable path remains safe later.

**Invariant / rejection / proof.** Only checked immutable data reaches lowering; only run admission can supply child execution context. Wire structural errors remain `MANIFEST_INVALID`, exact identity/version errors remain `RUNTIME_ABI_MISMATCH`; host and relational owners retain their own codes. Prove malformed raw bytes and origin precedence, exact identity mapping, source-free controls, secret/source failures, constructor-to-execution transformations, and invalid-admission no-child/no-state behavior. Ordinary compiler visibility/type checking replaces tests of impossible borrowed/control misuse; it does not replace raw wire negatives.

## 2. One Rust graph interpretation, separate from slot binding

Sources: [lower.rs](runtime/crates/nixfied-runtime/src/execution/lower.rs), [plan.rs](runtime/crates/nixfied-runtime/src/execution/plan.rs), [derive-facts-vectors.nix](nix/checks/derive-facts-vectors.nix).

**Root cause.** Lowering verifies a second Rust interpretation of the graph rather than proving the one execution uses. Planning mixes flattening, union derivation, startup order, and port allocation, then repeats everything for every task/slot.

**Design.** The lowering owner privately constructs a candidate of locally resolved tasks/services. One concrete logical-graph module performs:

1. Resolve references for every declared task and service, not just selected/reachable ones.
2. Prove all task-reference and sibling-DAG cycles, including unused components.
3. Flatten task occurrences with their step paths and subtree dependencies; repeated occurrences remain distinct.
4. Build service edges from connectsTo plus every prepare task's leaf requirements.
5. Prove the combined service graph acyclic across all services.
6. Derive each task's canonical servicesRequired closure, stable startup order, flattened nodes, and total endpoint demand; compare carried facts against these independently computed results.
7. Check every task against every slot window. Logical derivation is once per task per admission; only capacity/port arithmetic depends on slot. Preserve deterministic slot/task failure traversal for capacity errors.

A private candidate is not an `ExecutionManifest`; the graph functions must not assume admission to establish admission. Build edges only after reference/task proofs. Delete `derive_services_required` and `leaf_requires` duplicates in lowering, and the separate connectsTo-only membership/cycle implementation in structural validation. Retain startup ordering because it computes a needed order. Reuse one combined edge definition.

Candidate and admitted representations share the same owned storage, moved into the admitted wrapper after proof; they do not maintain parallel graphs. Keep these functions within the existing execution owner. A new generic graph trait or reusable graph engine is unnecessary.

After admission, selection creates an ephemeral `LogicalPlan<'a>` borrowing immutable tasks/services from `ExecutionManifest`, using the same graph implementation. It may derive the selected task again; that is reuse of one interpretation, not a second mutable authority. Admission retains only temporary per-task endpoint-demand summaries until the deterministic all-slot capacity pass completes, then drops them. Flatten one task at a time and discard its nodes after deriving facts; never retain all flattened plans. The selected execution plan alone retains its required flattened occurrences. `bind_slot(plan, window)` supplies ports. Allocate ports in canonical service-ID and endpoint-ID order, separately from dependency-first startup order; endpoint-less services consume no ports.

The owning admission remains in the caller's stack scope while the selected plan and run session borrow it. Never put the plan and its borrowed owner into one self-referential session. Private node handles or references are local to that plan; manifest IDs remain serialization identities, not global arena indices. Selection and binding finish before host placement effects. The selected plan also contains preflattened prepare occurrences for each selected service. If two services name the same prepare task, retain both executions; static plan preparation does not deduplicate service lifecycle work.

**Invariant / rejection / proof.** Every published graph is reference-complete, acyclic, derivation-consistent, and feasible for every declared slot. Lowering owns relational rejection before effects. Retain independent Nix and Rust V1–V10 literal goldens; do not calculate expected facts using the new Rust implementation. Keep unused task/service cycles, mixed prepare/connectsTo cycles, diamonds, repeated occurrences, endpoint-less dependencies, multi-endpoint contiguous allocation, narrow secondary slots, and no-child rejection. Add a relational proof that the executed plan uses the expected services/startup/node order.

**Intentional ABI change.** Today pure connectsTo errors occur inside structural validation and map to `MANIFEST_INVALID`, before source/secret/closure checks. The chosen design moves all graph relationship errors to lowering: `MANIFEST_ADMISSION`, after those checks. Record both the new code and multi-fault precedence in contract fixtures and rotate runtime ABI in the same cutover. Do not silently remove the old validator in an earlier ABI-neutral commit.

## 3. Resolve each invocation once; render authored templates once

Sources: [lower.rs](runtime/crates/nixfied-runtime/src/execution/lower.rs), [process.rs](runtime/crates/nixfied-runtime/src/service/process.rs), [admission/secrets.rs](runtime/crates/nixfied-runtime/src/admission/secrets.rs), [ADAPTERS.md](docs/ADAPTERS.md).

**Root cause.** Reference validation discards the executable/tool choice and repeatedly scans opaque strings. Execution reparses text that admission supposedly understood. Probe deadlines have two owners.

**Design.** A single lowering pass per inline invocation resolves declared tools, first-tool-wins executable selection, carried executable agreement, codebase/cwd, and selected closure effects. It emits the resolved invocation and adds its operation to a local closure-binding map; compare carried operationBindings after resolution. No invocation IDs or registry are introduced. Use the selected closure directly for service-start effect checks.

Keep one ordinary raw-DTO traversal of task/start/ready/health invocation positions for shared pre-lowering admission consumers. It yields borrowed wire invocations, not resolved invocations: secret and closure checks run before lowering and cannot depend on its products. Closure executable checks collect invoked-tool membership once instead of rescanning all invocations for each closure. Secret declaration membership uses the existing map. The local operation-binding accumulator exists only during lowering and cannot become another runtime registry.

Represent substitution with a private `Template` composed of literal pieces and recognized endpoint/state/secret references. Parse only the supported Nixfied forms. Arbitrary child syntax such as `${HOME}` remains literal; malformed recognized Nixfied forms and forbidden secret positions reject at the existing secret/lowering phases. A common tokenizer supplies early secret-reference validation and later contextual endpoint resolution, so there is one grammar, not independent scanners. Early secret validation need not construct or store full executable templates.

The tokenizer scans authored text left to right, recognizing exact `${port}`, `${host}`, `${stateDir}`, `${port:<name>}`, `${host:<name>}`, and `${secret:<id>}` occurrences. Named payloads must be nonempty and contain neither braces nor a nested `${`; recognized openers without a closing brace reject. Unknown child forms stay literal, but do not hide recognized occurrences within them: `${HOME:-${port}}` preserves `${HOME:-` and the final `}` while substituting its inner `${port}`. `${port:${HOME}}` rejects as a malformed recognized form. `${portfoo}` remains literal. This lexical rule deliberately does not implement shell expansion or balanced-shell-expression parsing. Secret references anywhere in authored argument text remain forbidden, including inside unknown child forms. One tokenizer reports typed malformed-reference categories; early secret checking rejects only secret errors, leaving endpoint/state errors to contextual lowering so phase precedence remains explicit. Add identical literal acceptance/rejection vectors for the Nix validator and Rust parser, including nested forms and unknown unclosed forms.

The lowered template holds resolved endpoint selectors and declared secret IDs. Render once against the selected slot, state root, and admitted secrets. **Inserted values are opaque bytes/text: never parse or substitute them again.** This intentionally removes order-dependent recursive substitutions. A secret containing `${secret:B}` remains those bytes even if B is also referenced elsewhere in the original template. A host path containing `${port:...}` is inserted literally, not reinterpreted as a new reference. Secret material stays out of persistent plans/manifest/error text; templates store references, not values.

Use a private validated relative-cwd value for lexical safety. Current canonicalization, directory existence, and source-root confinement remain at execution because the filesystem may change. Reuse the existing endpoint representation where it has identical meaning instead of copying a `ResolvedEndpoint` type.

Make `ResolvedInvocation` the timeout-free command payload. `ExecTask` owns the task timeout; `ExecProbe` and `TcpProbe` each own their attempt timeout. Delete the probe lowering assignment that copies its deadline into the invocation. Service start does not gain a new kill-after deadline: its current lifecycle does not consume `start.exec.timeout`. Retain authored invocation timeout bytes in wire validation and service identity inputs where they already participate. Removing an unused native copy must not change reuse identity or invent a new operational timeout policy.

**Invariant / rejection / proof.** Every executable invocation has one resolved executable/tool interpretation; every runtime reference is admitted in its direct scope. Lowering owns endpoint scope, first authored task dependency semantics, endpoint-less rejection, tool/effect checks, and binding comparison; secret admission owns declared references and secret acquisition. Preserve first-tool-wins, named/own/bare endpoint scopes, endpoint-less first dependency, unknown tools, effects, binding vectors, literal child syntax, and real probe timeout behavior. Replace late substitution-rejection tests only once private templates prevent that illegal construction.

**Intentional ABI change.** Opaque inserted values differ from today's chained `String::replace`. Pair the template cutover with the graph semantic ABI cutover, explicit substitution documentation, and cross-layer literal vectors. Nix validation must accept/reject the same authored grammar; Nix does not resolve secret values. Include order-sensitive secrets, unclosed supported forms, unknown child forms, and inserted placeholder-looking path bytes. Do not claim this change is byte-identical.

The consolidated relational phase has an explicit rejection order: reference and operation-ID coherence; local invocation resolution and endpoint/effect coherence; task/sibling cycles; combined service cycles; carried operationBindings; carried servicesRequired; then slot capacity. Within invocation traversal, retain canonical service order with start/ready/health positions, followed by canonical task order; tool selection retains authored order. Within capacity checks retain slot order and the existing leaf-then-composite task order. Characterize multiple-fault inputs against this schedule, including local invocation failures that previously occurred after carried-fact checks. This change in rejection precedence is part of the semantic cutover, not an incidental refactor.

## 4. Represent service ownership and transitions directly

Source: [process.rs](runtime/crates/nixfied-runtime/src/service/process.rs).

**Root cause.** `borrowed`, optional child, monitor, startup guards, and relays represent mutually exclusive resource ownership and lifecycle stages in one product record. `stand` clears fields so Drop will not terminate an intentionally persistent process.

**Design.** Acquisition returns common service identity with `Borrowed(BorrowedService)` or `Owned(StartingService)`. The borrowed payload contains lease/reuse evidence and no child handle. The starting payload owns child resources and endpoint startup guards; even endpoint-less startup uses an empty guard collection, not an ambiguous missing guard. Successful `ready(self, registry, cancellation)` commits the ready transition before releasing guards and returns `ReadyService`. Initial health checking still follows readiness and precedes adding the service to the successful session collection. `ReadyService` retains owned failure-settlement capability: a health failure must use the same explicit failed-start settlement as a readiness failure, even though guards have already been released. Snapshot the failed service's identity/endpoint/process output before consuming ownership; carry that evidence to finalization with `failedService` and the original lifecycle failure, preserving settlement-error precedence. Failed readiness explicitly settles and contains using the still-owned starting payload.

The session stores ready owned services or borrowed services. Child-specific operations live on owned payloads. Borrower finalization only releases its lease; it never signals, reaps, or joins the owner's child. Owned stop/cancel contain and settle with existing policies. `stand(self, registry)` consumes ready ownership after the standing transition succeeds. Transfer to persistent operation must explicitly relinquish child teardown ownership and preserve the existing long-lived redaction relay behavior; never join still-open persistent-service pipes. Monitor shutdown, relay detachment/ownership transfer, and child-handle release are named parts of this one transition. On failed standing commit retain ownership long enough to attempt normal failure cleanup.

Use concrete starting/ready structs only where they remove real guard/child branches; do not add generic phase markers for every registry status. Drop remains best-effort containment protection for owned resources, not an infallible registry transition. Private optional fields used solely for Rust move-out in Drop are acceptable if inaccessible as domain states; avoid reintroducing public booleans.

**Invariant / rejection / proof.** Exactly one local owner can terminate a child; a borrower cannot acquire that capability. Readiness lock release follows committed ownership evidence. Acquisition/ready/stand own temporal validation. Retain borrowing, persistent survival, until-idle, failed-start cleanup, initial health failure after committed readiness (including failed-service summary and cleanup-error precedence), cancellation-after-prepare, guarded ready rollback, lease mismatch, and fresh process identity tests. Compilation replaces tests of borrowed child access; OS liveness is never compiler-proven.

## 5. One bounded-child implementation and safe relay completion

Sources: [task.rs](runtime/crates/nixfied-runtime/src/service/task.rs), [process.rs](runtime/crates/nixfied-runtime/src/service/process.rs), [redaction.rs](runtime/crates/nixfied-runtime/src/redaction.rs).

**Root cause.** Task and probe features each own hermetic command construction, process-group setup, polling, cancellation, termination, reaping, and relay ordering. Their error paths have diverged.

**Design.** Extract a concrete owned bounded-child operation around existing `BoundedExec`, outcomes, and `terminate_and_reap`; share command configuration with service start where policies actually match. Keep service start as a long-lived lifecycle owner, not another bounded task. The shared operation configures env-clear, declared environment plus runtime PATH, stdin policy, cwd, group creation, output capture, and polling.

Expose two concrete stages: spawning returns an owned child/capture handle; completion consumes it. Between them, the task caller records the process before reporting it started. If recording fails, the same handle follows unrecorded-child cleanup rather than entering ordinary task completion. Probes proceed directly to completion. Command-owned pipe writers are dropped before waiting for relay EOF. This preserves the recording barrier without adding a registry dependency to the child primitive or duplicating its cleanup.

Task policy supplies one synchronous `before_termination(reason)` callback for cancellation/timeout registry intent. It runs **before any signal**. A callback error becomes an execution error but cannot bypass containment and reaping. Probes supply no registry intent; retry/attempt policy stays in readiness. Task success-code interpretation and evidence remain in task execution. A concrete function taking a closure is sufficient; no executor trait or async runtime.

On every exit/error path: observe the outcome, attempt process-group containment, attempt reap even if containment fails, then shut down capture. `terminate_and_reap` proves only original-group cleanup: a child can call `setsid`, reparent, and retain a writer even when that function succeeds. Neither a process inventory nor an empty original group proves EOF. Do not mint a pipe-closure token from those observations, and do not make worker join conditional on a supposed complete descendant inventory.

**Chosen capture protocol (required, not deferred).** The existing capture owner keeps one worker per stream, with a nonblocking pipe read end and bounded polling. All bounded task/probe output uses pipes, including an empty redactor; the current no-secret direct-file fast path would otherwise let an escaped writer mutate supposedly final evidence. Persistent service output retains its separate standing transfer policy. Each bounded reader owns its file writer; children receive only pipe write ends. Close unused descriptors on spawn/error, set close-on-exec on internal pipe descriptors, and drop the configured `Command` writers before shutdown.

Each worker has a private standard-library control channel, checked at every loop iteration; a single shutdown message supplies its deadline, and disconnected control means abort/incomplete. The owner retains these senders through spawn and cleanup. After the containment/reap attempt, publish one absolute monotonic deadline to both workers: **1,000 ms from capture-shutdown initiation**, owned by the bounded-child completion operation, not reset per stream or by progress. Workers poll for at most 10 ms (or the remaining deadline), read at most one 8 KiB chunk per iteration, and check shutdown/deadline before each subsequent read, including when a pipe remains continuously readable. EOF finalizes the redactor tail and flushes normally. At deadline without EOF, close the read end, discard the undecided redactor tail (flushing a partial secret prefix could leak it), flush only already-redacted bytes, close the evidence writer, and return `Incomplete`. A simultaneous deadline/available-data case checks expiry first; it must not report complete merely because some bytes were drained. The bound excludes arbitrarily blocked regular-file writes/flushes or OS scheduling, and does not claim an escaped process was terminated.

Completion joins **both** workers and collects stdout then stderr results even after the first error/panic. Represent complete versus incomplete capture privately; only two successful EOF completions can issue `CompletedEvidence` or a replay ticket. On incomplete capture retain safe prefix files for diagnosis, publish no completed task evidence/replay, and continue finalization; preserve unresolved process ownership whenever containment failed. Original-group cleanup success is not upgraded to complete-tree proof, nor does capture timeout itself prove escape. No detached bounded relay may continue modifying these files after completion returns. This is a capture guarantee only: original-group containment still cannot promise to discover or kill every escaped descendant. Preserve that limitation explicitly; do not reinterpret incomplete EOF as a reliable process inventory. Construction/spawn/recording failures use this same shutdown path for whichever workers were created. The service terminal cleanup paths must use the same bounded relay shutdown mechanics; persistent `stand` explicitly transfers/detaches long-lived relay ownership instead.

**Diagnostic decision.** A stream that misses EOF emits existing `SECRET_LEAK_BLOCKED` with fixed safe message `captured stdout did not reach EOF before shutdown deadline` (or `stderr`); include neither child bytes nor raw OS text. This code describes failure to establish safe completed capture, not proof that a secret actually leaked. Preserve existing capture I/O/panic mappings and distinct unredacted file-creation `STATE_UNWRITABLE` mapping despite the empty-redactor pipe change. Capture failure is infrastructure and outranks task/projection outcomes; containment/reap error remains primary when also present, with capture failures appended in stdout/stderr order followed by subordinate task outcome using the existing cause-flattening rule. Specify exact JSON goldens for these combinations. No new public error code or manifest option is introduced; the new deadline, failure cases, diagnostic messages, and capture-path behavior are an explicit runtime ABI change.

**Invariant / rejection / proof.** Every spawned bounded child receives containment/reap attempts; intent precedes signals; actual EOF alone proves completed capture; no surviving writer can force an unbounded pipe-read wait. Independent test children must cover an ordinary group descendant, a handshaken `setsid` escape holding an idle writer, and an escaped continuously writing child, with and without secrets. Assert successful original-group cleanup does not bypass the capture deadline, no replay/complete evidence on timeout, closed/stable prefix files after return, secret prefixes absent at abort, and test-owned survivor cleanup. Use an external test timeout as a backstop, not a sleep as synchronization. Cover one worker failing while the other drains, both errors in deterministic order, construction/spawn/recording failures, containment failure, cancellation intent failure, normal large-output EOF, and redacted EOF tails. Run Linux and macOS cases. Persistent standing-service survival and ongoing redacted output remain separate proofs.

## 6. Execute leaves once and own evidence once

Sources: [main.rs](runtime/crates/nixfied-runtime/src/main.rs), [task.rs](runtime/crates/nixfied-runtime/src/service/task.rs), [output.rs](runtime/crates/nixfied-runtime/src/output.rs).

**Root cause.** Prepare/root callers duplicate leaf execution while `TaskRun`, selected task output, node output, and aggregate output become competing mutable representations. IDs are resolved and composites flattened again after admission.

**Design.** One concrete `execute_node` takes a resolved leaf occurrence, run context, registry, dependency handles, cancellation, and evidence mode. Prepare/root drivers select occurrences and policy only. The selected borrowed plan supplies task references and occurrence step paths before effects; no execution-time manifest lookup or re-flattening.

Prepare occurrences always use capture-only evidence. Only the directly selected leaf receives replay selection; neither a child task's manifest default nor a prepare task's default changes that decision. Split registry, evidence collection, and immutable started-service borrows at the call site instead of hiding the run session behind interior mutability to satisfy callbacks.

Keep `TaskExecutionError`'s before-terminal versus after-terminal evidence distinction. Record each terminal `TaskRun` once in a session-owned vector. Its private occurrence index identifies that execution, not task ID or globally unique step path. Separate service prepares may legitimately repeat the same task and wire stepPath; retain both records and do not invent a new wire prefix. Selected task and node projections store references or private indices into this collection. Prepare runners borrow the session evidence collection directly and append each terminal result before continuing or returning an error. Their return becomes `RuntimeResult<()>`; delete evidence-bearing `PrepareTaskError`, `ServiceStart.prepare_runs`, and the `ServiceStartError` evidence shuttle once all consumers are converted. A before-terminal failure appends no fabricated record; after-terminal failure appends its completed record once before propagating the error. Prepare runs therefore survive both later prepare failure and later startup/root failure. The direct-selected replay ticket stays move-only and opens both evidence files before terminal registry transitions/cleanup.

**Confirmed storage defect and decision.** `service/task.rs` currently derives both log paths from `node_id`, and `redaction.rs` opens them with `File::create`. Two services preparing the same task, or a prepare followed by that task as root, can overwrite earlier evidence despite distinct in-memory records. Allocate a monotonically increasing session-local occurrence number before creating files, distinct from the terminal-vector index (failed attempts can leave gaps). Use `task.<occurrence>.stdout.log` and `task.<occurrence>.stderr.log` under the existing run logs directory, starting at zero; use exclusive creation, never truncate or retry a conflicting name. Allocate once for each attempted node execution, including prepares; no task-ID deduplication, random identifier service, new registry, or wire field is needed. Preserve public `stepPath`/task identity and ordering. Log-path values in summaries/registry command evidence intentionally change and belong to the ABI cutover. An existing file rejects with the current redacted/unredacted file-creation code and does not get overwritten. Check repeated prepares and prepare/root overlap with different literal output per occurrence, assert distinct paths and unchanged earlier bytes, then inject later failure and verify all completed records/files remain attributable. Do not reuse terminal collection length as the file allocator.

At finalization, borrowed views read the canonical evidence; if current generated output records require owned values, construct them once at the serialization boundary. Do not add custom serializer machinery solely to remove one final clone. No mutable serialized mirror is maintained during execution. Preserve the existing public run-task/run-node fields, null/omission behavior, task ordering, direct-leaf primary selection, composite last-task projection, and task-output defaults.

**Invariant / rejection / proof.** A task occurrence has at most one terminal evidence owner; every emitted projection refers to that outcome. The node runner rejects dependency/lifecycle failure in its proper phase; evidence remains available after later failures. Keep prepare-on-later-failure, repeated task occurrences, before/after-terminal faults, exact output goldens, nonzero accepted exit, defaults, and replay-before-teardown tests. Raw serde/output tests remain because types do not prove emitted bytes.

## 7. Registry owns decoding; transactions own decisions

Sources: [service/registry.rs](runtime/crates/nixfied-runtime/src/service/registry.rs), [registry/events.rs](runtime/crates/nixfied-runtime/src/registry/events.rs), [control.rs](runtime/crates/nixfied-runtime/src/control.rs), [service/mod.rs](runtime/crates/nixfied-runtime/src/service/mod.rs).

**Root cause.** Readers expose storage strings and consumers independently rebuild endpoint/process meaning. Transaction context is copied repeatedly. Reconciliation returns presentation records that `down` then interprets as policy.

**Design.** Add small native readers over `&Connection` that also accept a transaction through its existing connection access. One endpoint row decoder validates status, service-instance prefix, endpoint identity, loopback address, and port representation. Reservation, activation, reuse, and control consume these typed rows. Keep exact complete-set equality and owner/lease comparisons at the transition that needs them.

Own the active-process OR escaped-with-open-port predicate once. Preserve caller-specific multiplicity and error precedence: a snapshot corruption check and a reservation conflict check need not become the same query/result simply to share SQL. Select the smallest shared row shape; do not hydrate all data everywhere.

Use the existing stored-process-identity owner for a shared serializer. Preserve distinct task/service wire shapes, including absence versus presence of tracked-process evidence. Remove duplicated run/service IDs from in-memory transition payloads when the transition context already owns them; continue comparing actual stored rows against that context.

A small borrowing transaction context supplies connection, immutable registry identity, and redactor without cloning. Transition functions retain explicit deferred/immediate mode and visible mutation order. Reuse `append_event` for event-only writes and one borrowed event insertion record. Do not introduce callbacks describing a generic state machine. Mutation and redacted event append still commit together.

Split reconciliation into a domain operation returning typed current observations and a `ps` projection. `down` consumes actionable typed observations, never public status strings. A returned observation is evidence at a moment, not authority: refresh OS identity immediately before signals and re-read mutable ownership facts within the relevant immediate transaction before writes/CAS.

**Invariant / rejection / proof.** Untrusted stored representations are decoded once per read; durable decisions are made against current transaction-local facts. Registry decoding rejects corruption, transitions reject stale ownership/leases, and OS observers reject unverifiable identity. Keep malformed rows, extra/missing/duplicate endpoints, multiplicity, exact reuse, escaped retained ports, rollback, event redaction/order, writer contention, PID reuse, and fresh pre-signal checks. Reusing code never authorizes reusing stale snapshots across race boundaries.

## 8. Delete the fixed placement language

Source: [state/placement.rs](runtime/crates/nixfied-runtime/src/state/placement.rs).

**Root cause.** Runtime-owned constant layout is still interpreted as configurable templates; logs/artifacts independently reconstruct the run path.

**Design.** Validate external project/environment/run values as nonempty single normal path components, excluding `/` anywhere (including trailing `/` and repeated `/`), embedded NUL, `.`/`..`, absolute paths, and `${` syntax. Check original bytes before `Path::components`, whose normalization can conceal `x/` or `x/.`; then require exactly one normal component equal to the original value. On supported Unix hosts a backslash is an ordinary filename byte, not a separator; do not silently narrow that unrelated case. Reject lexical defects before filesystem effects; then join directly:

```text
slot_relative = project / environment / slot
state_root    = state_base / slot_relative
registry_dir  = state_base / registry / slot_relative
run_dir       = state_root / runs / run_id
logs_dir      = run_dir / logs
artifacts_dir = run_dir / artifacts
summary_path  = run_dir / summary.json
```

Registry remains outside the tree deleted by slot cleanup. Delete `TemplateVars`, expansion, fixed template constants, and generic template interpretation. **This is an intentional additional ABI change.** Today validation is applied to the entire interpolated path: nested relative project/run values and later template variables embedded in earlier substitutions can be accepted. The target chooses ordinary single components and rejects those inputs before effects. Record that narrowed boundary in the contract and exact negative fixtures; do not label it a neutral replacement. Slot paths for currently supported ordinary identifiers remain unchanged. This layout boundary is unrelated to authored child invocation templates.

**Invariant / rejection / proof.** For accepted single-component identifiers, pure placement derives the same confined layout before effects; host materialization retains canonical/symlink confinement. Placement owns lexical rejection, filesystem owners retain race-sensitive checks. Keep both-slot exact paths, registry survival through clean, adversarial traversal, root symlink, and nested symlink tests. Add explicit old-versus-new cases for nested/absolute/template-looking components to the semantic cutover. No new path hierarchy framework.

## 9. Return only outcomes an operation can produce

Sources: [process.rs](runtime/crates/nixfied-runtime/src/service/process.rs), [state/cleanup.rs](runtime/crates/nixfied-runtime/src/state/cleanup.rs), [endpoint.rs](runtime/crates/nixfied-runtime/src/service/endpoint.rs).

**Root cause.** Broad `Result`/product types encode nonexistent outcomes, creating fallback errors and repeated optional checks.

**Design.** Endpoint error refinement returns `RuntimeError` directly; its observed registry/classification failures are alternative errors, not a successful unit result. Delete the caller's fabricated `RegistryCorrupt` impossible-success branch. Cleanup terminal recording accepts `Deleted` or `Failed { safe_reason }`, never `Intent` plus unrelated optional reason. Parse persisted cleanup status at the row boundary. Use `V4(address)` versus `V6 { address, v6_only }` for observed listeners if this removes real optional-mode branching; IPv6-only evidence must remain mandatory for IPv6 classification.

Remove ambient test lock-root injection from endpoint acquisition. Production opens and validates its fixed lock directory; a private acquisition function takes that owned/borrowed directory descriptor explicitly, and tests supply their validated temporary descriptor. Preserve ownership, mode, symlink/nonregular rejection, nonblocking locks, close-on-exec, partial-set release, and the real-start-before-prepare test. Replace magic directory-mode switches with named concrete policy cases where necessary; do not introduce a pluggable lock provider. The platform listener parsers and full holder correlation remain their existing owners.

**Invariant / rejection / proof.** These private types exclude false success, terminal intent, and missing IPv6 evidence. Native classifiers/DB decoders reject only genuinely unknown data. Preserve exact endpoint error precedence, terminal event/status projection, wildcard/dual-stack behavior, and corrupt-row tests. Do not add enum-construction/getter tests for compiler-proven alternatives.

## 10. One failure owner and record-once cancellation

Source: [main.rs](runtime/crates/nixfied-runtime/src/main.rs).

**Root cause.** `FailureAccumulator` keeps pending causes outside `RuntimeError`, while cancellation observation is repeated with a flag that is not updated after replay. The same observation can be appended again after cleanup.

**Design.** Keep only `Option<RuntimeError>` as the accumulator state. The exact merge rule is:

- Empty: take the incoming error unchanged.
- Incoming priority less than or equal to primary: retain primary; append incoming flattened causes, then its safe root cause, to primary's causes.
- Incoming priority greater than primary: incoming becomes primary; its existing causes come first, then the old primary's accumulated causes, then the old primary's safe root cause.

This reproduces current final cause order, including equal-priority first-primary selection and cause-before-demoted-root order. Do not sort or deduplicate causes by code; repeated codes may describe distinct failures. Infrastructure remains above projection, which remains above task/cancellation/dependency outcomes. Preserve safe cause details and nonrecursive projection.

A local `record_cancellation_once` operation owns the observation flag and updates it when inserting the cancellation error. Seed it from an initial canceled outcome. Call it at the existing checkpoints so late cancellation still affects teardown/terminal status; it only deduplicates the finalizer's same cancellation observation, not all canceled causes globally.

**Invariant / rejection / proof.** One ordered diagnostic owner and at most one finalizer cancellation insertion. Finalization owns this merge after admission; phase-specific errors remain unchanged. Prove a small matrix of every priority relationship, nested incoming causes, equal-priority ordering, and exact JSON cause sequence. Add a controlled cancellation-during-replay/finalization test proving one insertion and continued cleanup. Treat this source-established defect as a focused correctness fix, not as already experimentally reproduced.

## 11. Make registry construction atomic under concurrent creation

Source: [registry/schema.rs](runtime/crates/nixfied-runtime/src/registry/schema.rs).

**Root cause.** Tables/meta commit before `user_version`, leaving a crash gap; pretransaction emptiness classification can also race another initializer. Rereading new writes does not provide atomicity.

**Design.** Apply connection-level WAL/foreign-key/busy-timeout configuration as required, then begin an immediate write transaction. Inside that transaction read version and user-table count exactly once (delete pretransaction classification) and choose exactly one case:

1. Empty and version zero: create current tables/meta, set `PRAGMA user_version` through the same transaction, commit once.
2. Existing current version: validate required shape and exact identity under the transaction, then finish without rewriting history.
3. Any incompatible/nonempty-unversioned state: reject without schema/data mutation.

The locked classification prevents a losing concurrent creator from attempting a second initialization after a stale empty read. Query each required table's columns once, then compare the required set in deterministic order. This is the current partial required-shape check, not a claim of exhaustive schema equivalence. Use ordinary `CREATE TABLE`/`CREATE INDEX` and direct metadata insert in the locked empty case; remove `IF NOT EXISTS`, insert-if-empty guards, and new-database rereads that merely prove this transaction's successful writes. Retain all validation of existing bytes, meta version, and identity.

**Invariant / rejection / proof.** A committed new database has matching schema, metadata, and user version, or no creation commits. Schema initialization owns rejection before registry operations. Keep reopen, current-version malformed shape, incompatible version, nonempty unversioned, exact identity, and history-preservation proofs. Add deterministic rollback/interrupted-creation coverage and concurrent initializers with the same and differing identities. No schema version bump is needed because shape/meaning is unchanged; do not repair old partial databases or add a migration.

## 12. Reuse existing redaction and projection primitives

Sources: [redaction.rs](runtime/crates/nixfied-runtime/src/redaction.rs), [output.rs](runtime/crates/nixfied-runtime/src/output.rs), [error.rs](runtime/crates/nixfied-runtime/src/error.rs).

**Root cause.** Whole-buffer and streaming redaction implement the same matcher twice; orchestration rebuilds diagnostics whose safe construction already belongs to output handling.

**Design.** One private byte scanner accepts the full available input and a limit on eligible match-start positions, and returns redacted bytes plus consumed input count. Whole-buffer redaction uses the complete length; streaming keeps the maximum-pattern tail and drains the returned count. A match starting before the limit may consume beyond it. Preserve longest-first matching and emit remaining pending bytes only through the same redactor at EOF.

Reuse a narrow safe projection-error constructor from output handling; centralize native enum wire spelling at its existing generated/native owner. Delete all-variants no-op matches. Use section 5's EOF/deadline shutdown protocol for terminal relays; never infer EOF from containment success. Share safe diagnostic constructors only where code, exit class, details, redaction, and cause policy coincide: summary/footer orchestration and replay/capture can intentionally map similar I/O failures differently. Preserve those caller mappings through explicit arguments or separate narrow constructors, not one universal projection mapper.

Retain the replay copy loop: it distinguishes read/write/flush errors, partial progress, interruptions, and write-zero in ways `std::io::copy` does not report. Retain independent stdout/stderr workers, replay ticket ownership, safe details constructors, JSON traversal, and intentional native-versus-lossy path serialization.

**Invariant / rejection / proof.** Persistent runtime bytes are redacted before writing, chunking does not change output, and diagnostics never expose raw OS/secret content. Redaction/output owners reject failed capture/projection at their actual phase. Keep overlapping patterns, every split position, binary streams, empty secrets/output, partial writes, one-stream failure while the other completes, and exact safe diagnostic tests. No tests for forwarding accessors.

## 13. Share cohesive fixtures; preserve adversarial inputs

Sources: [tests/common/mod.rs](runtime/crates/nixfied-runtime/tests/common/mod.rs), [tests/service.rs](runtime/crates/nixfied-runtime/tests/service.rs), [tests/output.rs](runtime/crates/nixfied-runtime/tests/output.rs), [manifest fixtures](runtime/crates/nixfied-manifest/src/fixtures.rs).

**Root cause.** Tests share temporary-directory primitives while repeating complete runtime invocations. Some fixtures expose mutable raw and lowered manifests that callers must manually keep synchronized.

**Design.** Move the existing runtime fixture/command setup into common and keep returning an ordinary composable `Command`. Put service start operations on the existing service fixture rather than adding four forwarding functions. Configure raw scenario deltas, then lower once into the fixture used for execution. Corruption tests retain an explicit raw-byte/SQL escape path; helpers must not auto-correct the defect under test. Reuse the existing manifest base where scenario semantics match. Leaf-only tests do not acquire listening ports. Keep the independent test child simple and independent of production ownership logic.

Integration fixtures write raw manifest bytes to a temporary path and use `admit_run` or `admit_control` with the existing explicit non-store test policy and fixture context. Pure lowering tests use `ValidatedManifest::try_from` followed by `lower`; they do not need host admission and cannot manufacture a `RunAdmission`. Do not add an unchecked production or test-only admission constructor to keep old fixture conveniences alive. Fixtures testing malformed structure call the raw boundary directly instead of trying to construct an invalid validated wrapper.

Specific proof repairs/removals:

| Current case | Required change and remaining proof |
| --- | --- |
| Endpoint-less named reference test has stale servicesRequired | Fix carried union first, then assert the exact intended scope rejection |
| Missing registry shape test uses obsolete version | Set current version so missing-shape code is actually reached |
| Two readiness escape tests detach before probing | Retain one preexisting-escape case; use a real in-probe handshake for the distinct timing claim |
| Slot identity-only test overlaps executed two-slot test | Consolidate into actual two-slot identity/isolation proof |
| Separate PATH and environment captures | One child proves exact PATH, declared variables, and absent parent canary; use per-command environment |
| Positive tree token search duplicates exact redacted stdout | Keep exact output plus whole-tree secret absence |
| Reading back assigned raw_len or default Error::source | Remove; compilation and adjacent wire/error tests cover claimed guarantee |
| first_candidate_port exists only for its test | Delete helper/test; retain actual window/placement behavior |
| Repeated registry identity/cleanup setup | Table common setup, keep every input, error class, mode, and nonmutation assertion |

**Invariant / rejection / proof.** A runtime fixture contains one coherent constructed scenario; expected answers remain literal and independent. Fixture construction rejects accidental setup errors, production boundaries still reject intentionally bad data. Run the affected existing integration suites, not a new suite testing helper accessors.

## 14. Retire approximate audits only after proof mapping

Sources: `runtime/crates/nixfied-runtime/tests/capability_coverage.rs` (retired; available at baseline `d6b5632`), [coverage.nix](nix/checks/coverage.nix), [structure.nix](nix/checks/structure.nix), [wire_presence.rs](runtime/crates/nixfied-manifest/tests/wire_presence.rs), [generated.nix](nix/checks/generated.nix).

**Root cause.** Manual inventories survived after declaration ownership moved elsewhere. Token occurrence in one fixture is weaker than per-record coverage and cannot establish omitted/new-field coverage.

**Design.** Before deletion, map each old assertion to per-record structural inventory coverage, generated freshness, independent ABI snapshot, raw presence/unknown-field/enum negatives, or a specific behavioral test. Record which decoder policies each raw test actually exercises: the existing lifecycle unknown-field case is representative, not proof that every record already has an independent negative. Pair policy-matrix cases with generated decoder freshness and exhaustive native lowering where applicable. Add missing independent boundary proof if needed. Then remove the approximate fixture-token inventory, historical field blacklists, and duplicate handwritten all-error/all-exit lists/exhaustive sentinels. Generated enum exhaustiveness is not independent wire correctness; retain the latter.

Update the `AGENTS.md` requirement naming `capability_coverage.rs` atomically with its replacement routing. Edit [manifest declarations](nix/meta/manifest.nix), [output declarations](nix/meta/outputs.nix), and other authored owners; regenerate checked-in outputs with `.#regenerate`. Never hand-edit generated files or replace golden answers with output calculated by the implementation under test.

**Invariant / rejection / proof.** Every claimed contract surface still has an owner and a falsifiable independent proof. Declaration checkers reject missing/ambiguous coverage before generation; runtime wire tests reject invalid bytes. Preserve the existing maintenance exercise adding a required field and observing generated definitions, native consumers, and reference output. Passing a shared renderer against its own expected output is insufficient.

## Smaller subtractions and deliberate retention

Apply these with their owning changes, only when total helper-plus-caller code decreases:

- Remove test-only production convenience wrappers for readiness, health, dependent tasks, and duplicate stop entry points; fixtures supply an ordinary cancellation token. Retain real signal/SIGPIPE tests.
- Share the low-level sigaction install/save helper while keeping rollback and reverse restoration explicit.
- Remove repeated slot-policy validation within the same validated call chain and avoid building a vector solely to obtain a set.
- Tighten service-identity serialization's silent fallback through its existing owner without inventing a new hash scheme. Preserve exact identity bytes; a changed failure boundary joins the explicit ABI review.
- Keep ready and health nominal roles, lifecycle event vocabulary, move-only replay evidence, and before/after-terminal errors when they encode actual distinctions. Tiny structurally similar records do not justify a generic lifecycle wrapper.

The following are **deferred investigations, not approved replacements or promised reductions**:

| Candidate | Evidence required before a separate decision |
| --- | --- |
| Replace custom cleanup walker with std library | Pinned Linux/macOS root-symlink refusal, nested unlink semantics, confinement races, classification, and crash evidence |
| One process inventory per observation pass | Completeness/error semantics, remembered reparented escapees, fresh pre-signal identity; no persistent cache |
| Remove macOS PID hints | Proof that enumeration remains complete under churn; current hints add PIDs, not merely ordering |
| Unify FFI buffer growth policies | Actual matching native API race/growth/failure contracts, not superficial similarity |

Keep host checks, liveness reobservation, immediate transaction rechecks, PID reuse protection, source confinement, ownership/cleanup gates, independent Nix derivation, redaction, and no-child admission proof. These are necessary complexity.

## ABI and persistence delivery

Most sections are internal representation changes and must preserve exact current bytes, defaults, omission/null behavior, status/error vocabulary, lifecycle event semantics, and diagnostic precedence. No numeric manifest or SQLite schema version changes are proposed merely for refactoring.

The **explicit semantic cutovers** cover:

1. Graph relationships move to the relational admission phase; consolidated invocation and graph checks use the explicit rejection schedule in section 3. This changes some error classes and multiple-fault precedence.
2. Authored template parsing has one grammar and inserted values are opaque, removing recursive replacement behavior.
3. Fixed placement identifiers become lexically checked single normal components; whole-path interpolation no longer admits nested or embedded-template identifier inputs.
4. Bounded capture always uses pipes and has an EOF deadline; incomplete capture emits the exact diagnostics from section 5 and cannot replay. This is not a mechanical join refactor.
5. Per-occurrence exclusive task evidence files replace stepPath-keyed truncation, changing projected paths while preserving task/step identity.

Before each affected implementation commit, update the normative contract/adapter language and authored capability descriptor with the applicable admission, substitution, capture, or evidence-path semantics, rotating its digest-derived runtimeAbi. Update the independent ABI snapshot, both Nix/Rust constant consumers, producers, consumers, exact diagnostics, fixtures, and affected Nix/Rust golden suites together. Update DERIVATION_SPEC only if its algorithm/observable derivation semantics change; consolidating a Rust implementation must preserve its current results. New manifest fields are not proposed. If implementation discovers another observable change, stop treating that portion as ABI-neutral and add it to the reviewed cutover.

Old manifests are rejected by exact ABI identity. Old registry identity remains rejected; do not translate rows, rewrite append-only history, introduce aliases, or add a compatibility reader. Operators needing to stop old standing services use the matching old runtime/manifest before adopting the new contract, consistent with existing exact-version recovery. The RFC introduces no migration mechanism.

Schema initialization atomicity and cancellation deduplication are separately reviewable correctness fixes to existing guarantees. Their tests must describe the changed faulty path; they are not silently bundled into a mechanical LOC patch. Any public diagnostic changes caused by those fixes must be recorded explicitly under the same ABI discipline.

## Dependency-ordered coherent commits

Each row is a logical commit or narrowly scoped series whose individual commits remain buildable and coherent. Subjects are lower case. Never land a new reader alongside a retained fallback or leave source consumers midway through a representation cutover.

| Order | Commit purpose | Dependencies and proof before proceeding |
| --- | --- | --- |
| 1 | repair misleading runtime proofs | Correct wrong-reason/timing fixtures; focused admission/registry/service cases establish baseline |
| 2 | remove impossible outcomes and dead ceremony | Error-returning classifier, no-op matches, test-only wrappers; focused behavior plus compile/lint |
| 3 | prepare placement boundary proofs | Exact current paths/confinement/cleanup tests and characterization of currently accepted compound components; enable narrowed-component negatives with the later ABI cutover |
| 4 | make registry initialization atomic | Immediate classify/create/version transaction; reopen/rollback/concurrent-creator proof |
| 5 | own finalization failures and cancellation once | Exact priority/cause matrix and controlled late cancellation |
| 6 | close loaded and admitted construction | All constructors/consumers cut over together; origin/phase/control/no-effects tests; keep old graph validator until step 7 |
| 7 | unify graph and invocation admission semantics | One atomic cross-layer ABI cutover for sections 2–3 and direct placement from section 8, descriptor/docs/fixtures/goldens included; no provisional compatibility paths |
| 8 | represent owned and borrowed service resources | Acquisition/ready/stand/finalization consumers together; lifecycle/borrowing/persistent tests |
| 9 | bound terminal capture and share child completion | Atomic section 5 protocol plus descriptor/ABI/docs/fixtures; task intent, no-secret pipes, EOF deadline, both workers, escaped-writer and platform proofs; no intermediate unconditional joins |
| 10 | prove terminal and persistent capture ownership | Integrate every service terminal error path with bounded shutdown while preserving standing transfer; may be part of step 9 where resource owners are coupled; no ABI-neutral claim |
| 11 | execute plan nodes with one evidence owner | Plan borrowing, direct prepare append, occurrence allocation/exclusive files and output projections together; path ABI/docs/fixtures and repeated-prepare failure proofs |
| 12 | centralize registry readers and event context | Typed decoding and callers together; transactional race/rollback/control tests |
| 13 | share redaction scanning and safe projection construction | Chunk/binary/partial-write/security proofs; no capture/replay framework |
| 14 | consolidate fixtures and retire superseded audits | Only after replacement guarantees exist; update AGENTS routing and generated coverage together |

Small fixture consolidations may accompany earlier owner changes rather than accumulate to step 14. Conditional platform investigations are not dependencies of the accepted reduction and must not delay completing it.

## Verification and acceptance

Start with the narrowest behavioral proof for each commit using the pinned fixture-backed environment. Examples from [DEVELOPMENT.md](docs/DEVELOPMENT.md):

```sh
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-manifest'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test admission'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test service'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test registry'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test state'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test output'
nix run .#test
nix run .#ci -- --dirty
```

Select filters/suites appropriate to each change; do not rerun everything after trivial edits. `.#test` is the complete fixture-backed Cargo floor, including the realized recovery fixture; raw Cargo is not equivalent. Cross-layer/ABI changes require `.#ci -- --dirty`, generated freshness, and both derivation suites. Output changes also require the downstream gate. Run rustfmt/Clippy as prescribed. Process/endpoint/capture changes require Linux and macOS proof; release/CLI/install builds remain separately reportable coverage.

For the baseline and final tree, measure authored production, embedded tests, integration/common tests, generated code, and private test-child code separately. Record moved versus deleted code, number of duplicate algorithms removed, redundant state owners removed, and newly required coordinated change sites. Do not count generated formatting changes as design savings or set a percentage target that rewards erased evidence.

Acceptance requires:

- One Rust implementation of each graph fact; independent Nix and literal oracles remain.
- Private proof boundaries are enforced by APIs, with no unchecked execution constructor or mutable admitted graph.
- Run/control and owned/borrowed alternatives cannot be confused at call sites.
- One bounded-child cleanup path, one terminal task-evidence owner, and registry-owned decoding; temporal checks remain at effects.
- No late manifest-admission errors; exact reviewed diagnostics and cause order; no duplicate finalizer cancellation.
- Complete output redaction and bounded EOF/deadline capture shutdown for every bounded child, including escaped writers after successful original-group cleanup; incomplete capture never replays and has no unbounded pipe-read wait. Persistent-service standing remains functional.
- Each removed test/audit has an identified replacement guarantee, and the remaining test actually reaches the claimed boundary.
- Every intentional ABI change lands atomically with documentation, descriptor, fixtures, and both sides; no fallback or migration.
- Net reduction in maintained duplication and change sites, with honest LOC accounting and explicit reporting of any added correctness code.

These are the original design acceptance criteria, not verification receipts.
The [outcome report](RFC_REFACT_REDUCE_COMPLEXITY_PROGRESS.md) records measured
results, the proof repair, and unverified platform coverage. Future reduction
work should separate behavior-preserving simplification from correctness changes,
budget production/test/documentation growth, and review the tradeoff when the
expected aggregate savings disappear.
