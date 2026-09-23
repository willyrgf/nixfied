# Runtime complexity implementation evidence

Objective: fully implement `RFC_REFACT_REDUCE_COMPLEXITY.md`. This record tracks
evidence, not a replacement scope or acceptance specification.

## Baseline

Implementation starts at `d6b5632`, with a clean worktree. Its Rust tree has the
same 36,093 physical lines as the RFC baseline `0570aa3`.

| Category | Baseline lines |
| --- | ---: |
| Authored production | 18,756 |
| Embedded test modules | 3,778 |
| Integration/common tests and manifest fixtures | 12,099 |
| Generated Rust | 817 |
| Private test child | 643 |

Measurement: enumerate tracked `runtime/**/*.rs` at the revision; count physical
lines including comments and blanks. Classify generated paths first, then the
test-child crate, then `tests/` paths and `fixtures.rs`. For remaining source
files, split at the first `#[cfg(test)]` followed by `mod tests`; the suffix is
embedded tests. This isolates the 220-line shared manifest fixture from authored
production, unlike the RFC's combined source count. Use the same procedure for
final accounting; separately report code movement, deleted duplicate algorithms,
removed state owners, coordinated change sites, and capture correctness additions.

## Ordered delivery

1. Proof repairs: endpoint-less named-reference fixtures now carry the correct
   service union and assert exact scope diagnostics. The missing registry shape
   fixture uses the current schema version and asserts the missing-column error.
   The readiness escape fixture now triggers detachment from a blocked exec probe;
   the distinct preexisting-escape test remains. This exposed a real last-attempt
   bug: readiness returned `READINESS_TIMEOUT` for the in-probe escape. Rechecking
   service liveness after the probe fixes that path. Contract and capability
   descriptor record the diagnostic change; the independent ABI snapshot rotates.
   Linux evidence: both named-reference tests, all 71 service tests, all 13
   registry tests, and all 32 manifest tests passed; workspace/all-target Clippy
   passed with warnings denied. `nix run .#ci -- --dirty` (session `90410`)
   finished: checks, the fixture-backed Cargo floor, and runtime gate passed;
   the Nix adoption gate failed with `PORT_CONFLICT` on 127.0.0.1:23080.
   Output is in `/tmp/nixfied-complexity-ci-step1.log`; failure diagnostics are
   in `/tmp/tmp.l3SWDPWwkw/direct-default.stderr`. The listener was Postgres PID
   508781 using `/home/willyrgf.linux/.local/state/nixfied/mfm/dev/0/pgdata`,
   outside this task's test state. It was preserved. Full gate remains outstanding.
   The first CI attempt rejected the missing native inventory coverage entry;
   that entry is now included. macOS remains unverified.
2. Impossible outcomes/dead ceremony: endpoint refinement now returns errors
   directly, deleting the fabricated success fallback. Cleanup terminal writes
   accept only `Deleted` or `Failed { safe_reason }`, and prior cleanup evidence
   stores a parsed status. All 24 state tests, 11 endpoint tests, six readiness
   tests, and workspace/all-target Clippy pass on Linux. Remaining: explicit
   lock-directory injection, applicable wrapper/ceremony subtractions, and the
   conditional listener representation review.
3. Placement characterization: exact slot-one registry/run/log/artifact/summary
   paths are pinned alongside existing slot-zero paths. Materialization rejects
   symlinks at each owned root and nested run path without writing through them.
   Literal cases record currently accepted nested project/run identifiers,
   embedded environment substitution, and slash-prefixed run identifiers;
   traversal, absolute project identifiers, and unresolved templates reject.
   Convert the accepted compound cases to negatives in step 7's ABI cutover.
   Deleted the test-only `first_candidate_port` production helper; actual
   service slot-window and planner allocation proofs remain. All 26 state tests
   pass on Linux, including cleanup registry survival and interrupted deletion.
4. Atomic registry initialization: one immediate transaction now owns database
   classification, existing shape/identity validation, creation, metadata,
   `user_version`, and commit. Deleted create-if-absent guards and new-write
   self-checks; required columns are read once per table. Schema version and
   diagnostics are unchanged. Independent proofs cover same/different-identity
   concurrent creators, metadata-insert rollback, denied version-write rollback,
   and process loss at SQLite's commit hook followed by reopen. All 18 registry
   tests, all 26 state tests, and workspace/all-target Clippy pass on Linux.
5. Finalization error/cancellation ownership: the primary error now owns every
   cause; the secondary accumulator is deleted. A nine-pair literal priority
   matrix covers equal-priority retention and existing/nested cause order.
   Finalizer cancellation observation updates its flag at every checkpoint.
   A real blocked-replay signal test proves exactly one cancellation diagnostic,
   complete replay, and no active service/lease/port rows after teardown. Removing
   the flag update made that test fail with two cancellation diagnostics, then
   restoring it passed. Contract, descriptor, native coverage, and independent
   ABI snapshot updated atomically. All 19 output tests, seven binary tests,
   32 manifest tests, Clippy, and formatting pass on Linux. Cross-layer `.#check`
   passed (session `77914`, log `/tmp/nixfied-complexity-finalization-check.log`). The full current-tree
   fixture floor and downstream gate remain required for final acceptance.
6. Closed loaded/admitted construction: first coherent boundary cutover complete.
   `ValidatedManifest` owns the DTO, has private construction and immutable access,
   replaces the single-implementation `Validate` trait, and is required by `lower`.
   `LoadedManifest` fields are private and parsing moves the checked document into
   it. Deleted the duplicate post-load ABI checker. Lowering fixtures use current
   identity; invalid leaf/window fixtures now exercise structural construction,
   while capacity rejection still exercises lowering. All 32 manifest tests,
   110 runtime unit tests, 31 admission tests, 71 service tests, and workspace
   Clippy pass on Linux. No public diagnostic or rejection-order change.
   Source/closure/target helpers now return phase errors without carrying a loaded
   document solely for diagnostics; the admission operation attaches provenance
   once. Live/immutable sources share the canonical confined-directory resolver,
   preserving lexical/root-check ordering and diagnostic text. Store containment
   returns the proved canonical path instead of forcing immutable sources to
   canonicalize twice. Secret and closure admission share one raw invocation
   traversal, and invoked-tool membership is collected once per closure pass.
   The follow-up passes all 31 admission tests, both secret unit tests, and Clippy.
   Secret descriptors now convert once into borrowed `EnvVar`/`File` alternatives
   before reference checking and value reads; resolution no longer reinterprets
   optional wire fields or uses presence `expect`s. Declaration membership uses
   the existing map. Eight malformed-source cases prove exact diagnostics and
   descriptor-before-reference/value precedence. All 31 admission tests, three
   context tests, three secret unit tests, and Clippy pass on Linux.
   Architect review selected lazy `InvocationRoot::{CurrentDirectory, Path}` in
   admission context. Resolution remains inside live-source admission after
   target/dirty-policy checks; controls and immutable sources never resolve it.
   All 33 admission tests and Clippy pass, including explicit-root deletion and
   immutable-source independence. The fixture-backed `.#test` floor at `5d26b54`
   passed (session `99052`, `/tmp/nixfied-complexity-pre-admission-floor.log`).
   A real-admission fixture trial exposed nonexistent state-fixture closures and
   fabricated service/upgrade metadata. The trial was removed; do not broaden the
   store root to `/` or add unchecked constructors to preserve those fixtures.
   Configure real declared executables before admission. Upgrade hash fixtures
   should use distinct valid raw JSON bytes and independently computed hashes.
   Lifecycle fixtures now declare Nix-built sleep and shell programs supplied by
   the dev shell and test wrapper; state/upgrade fixtures declare the realised
   test child. Declared basenames are retained for multicall dispatch, and the
   ready-file probe uses the shell builtin rather than an undeclared external
   `test`. All 71 service, 26 state, 10 upgrade tests and Clippy pass on Linux.
   Removed every fabricated admission literal from integration fixtures and the
   real-start endpoint unit proof. Fixtures now write raw JSON and run admission
   with an explicit workspace root. The executable-removal scenario alone injects
   its narrow temporary store root before removing the admitted program during
   prepare. Upgrade tests use compact/pretty valid bytes and independently hashed
   expectations instead of invented admission hashes. Changed service scenarios
   re-admit all bytes rather than replacing only the execution graph. All 71
   service, 26 state, 10 upgrade, and 11 endpoint unit tests pass; the owner
   attribution case passes after synchronizing its raw scenario; Clippy passes.
   Private `ControlAdmission` now owns the loaded document, immutable execution
   graph, and serialized provenance. `RunAdmission` adds required source/secrets;
   no boolean mode, optional source, public proof fields, or `require_source`
   remains. Path-based `admit_run`/`admit_control` own origin-before-parse ordering;
   the duplicate loaded-origin check and independently passed runtime manifests
   are deleted. State identity/cleanup derive facts from common admission. Source
   admission requires the structural wrapper and drops duplicate codebase checks.
   Architect review found the public task `RunContext` as a remaining bypass;
   it now privately borrows `RunAdmission` and derives source/secrets/hash from it.
   The public service-derived constructor is removed. Review confirmed closure.
   The obsolete control-source accessor misuse test is replaced by compiler
   visibility and the existing deleted-workspace control behavior test. Both raw
   constructors now prove malformed non-store origin precedence and provenance.
   All 110 runtime unit, 32 admission, 71 service, 26 state, 10 upgrade, three
   context tests and Clippy passed; `.#test` passed before the task-context
   follow-up, then 32 admission/71 service tests and Clippy passed after it.
   Logs: `/tmp/nixfied-private-admission-floor.log`, `/tmp/nixfied-run-context.log`.
   Step 6 complete: one local store-root observation now serves origin, immutable
   source, and closure checks. Declared-path containment remains separate from
   canonical store confinement. A missing-store multi-fault test preserves source,
   secret, then closure error precedence and raw provenance. All 33 admission
   tests and Clippy pass; the complete fixture-backed `.#test` floor passes on the
   final admission tree (`/tmp/nixfied-admission-complete-floor.log`).
7. Graph, invocation, template, and placement ABI cutover: complete.
   Normative contract, adapter grammar, capability semantics, and native coverage
   routing are drafted for the atomic change. Direct placement joins replace the
   fixed template interpreter; original component bytes reject slash/NUL/dot/
   template syntax while preserving Unix backslashes. All 26 state tests pass.
   A shared Rust tokenizer now drives early secret checking and detects known
   references inside unknown child syntax, including secrets nested in malformed
   endpoint forms. The independent Nix tokenizer is connected to compiler
   validation. Both pass the same 19 literal JSON grammar vectors; the Nix check
   is included in the source gate. These are authored literal expectations, not
   generated answers. New files are staged so Nix source filtering includes them.
   Logs: `/tmp/nixfied-step7-first.log`, `/tmp/nixfied-template-vectors.log`;
   direct Nix invocation-template check returned true.
   Private candidate storage now moves into `ExecutionManifest` after graph proof.
   The planner owns combined edges, all-task flattening/cycle checks, service union
   derivation, carried service checks and slot capacity; duplicate raw Rust graph
   walkers are deleted. Admission discards flattened occurrences per task and
   consumes temporary service unions into capacity summaries. Selected plans
   borrow task/service definitions, retain separate prepare occurrences per
   selected service, and bind ports canonically before placement/registry effects.
   Main no longer re-flattens or re-resolves planned definitions during execution.
   Invocation selection now traverses authored tools once and returns the chosen
   closure for effects; one local binding accumulator supplies derived comparison.
   The duplicate raw invocation traversal is removed. Task timeout belongs to
   `ExecTask`; probe timeout remains on its probe; endpoints reuse manifest Endpoint.
   Private lexical cwd rejects absolute/parent/NUL input during lowering; canonical
   filesystem confinement remains per spawn/attempt. The old lexical escape runtime
   test now proves a symlink changed after admission is rejected at execution.
   New independent proofs cover graph-vs-closure precedence, first-tool selection
   for effects/bindings, local errors before cycles/carried facts, and separate
   shared-prepare occurrences. Architect review found per-position service error
   ordering still deferred scope checks; start/ready/health now complete locally in
   order, with multifault proofs for invalid start scope versus ready executable,
   and invalid ready executable versus endpoint-less health TCP.
   All 68 execution tests, 34 admission tests, 71 service tests, 26 state tests,
   and workspace/all-target Clippy pass on Linux. Logs:
   `/tmp/nixfied-local-order.log`, `/tmp/nixfied-step7-graph-cwd.log`,
   `/tmp/nixfied-step7-service-state.log`, `/tmp/nixfied-graph-cleanup.log`.
   Parsed templates now hold only private checked pieces and resolved endpoint
   selectors/declared secret IDs. The renderer appends each piece once, keeping
   inserted secret/path values opaque. Removed the old endpoint and secret scanners
   and chained replacement implementation. Prepared probes now hold concrete
   rendered commands and one scheduling policy; attempts retain temporal cwd checks.
   Literal rendering proofs cover order-sensitive secrets, placeholder-looking
   state paths, and nested known references inside unknown child forms. End-to-end
   output proofs show inserted paths reach children literally and unused graph or
   template faults create neither state nor child markers. A real blocked probe
   test proves its own deadline both exceeds and truncates the authored invocation
   timeout as configured. Old scanner and late invalid-substitution proofs were
   replaced with parser rejection and rendered-behavior proofs.
   Architect review found Nix scanned run[0] as a template while Rust selected it
   literally. Nix now scans only argument tail/env, retaining secret prohibition
   throughout run. Realised Rust admission and Nix resolve/validate cases accept
   two placeholder-looking executable basenames but reject them in argument tails.
   Contract/adapter docs and descriptor record the literal executable position.
   ABI snapshot deliberately rotates to `nixfied-runtime-abi:1-90d13e80a7e8`.
   All 116 runtime unit, 35 admission, 71 service (before the new deadline case),
   21 output, 30 manifest tests and the new two-outcome deadline case pass; current
   all-target Clippy passes. Independent Nix template+validation check returns true.
   Logs: `/tmp/nixfied-opaque-templates.log`, `/tmp/nixfied-template-runtime.log`,
   `/tmp/nixfied-template-effects.log`, `/tmp/nixfied-literal-program-tests.log`,
   `/tmp/nixfied-probe-deadline-abi.log`, `/tmp/nixfied-nix-template-validation.log`.
   Cross-layer `nix run .#ci -- --dirty` passed (session `21511`, exit 0),
   log `/tmp/nixfied-step7-ci.log`: source/freshness/Nix and Rust derivation checks,
   the complete fixture-backed Cargo floor including recovery, all 22 runtime
   gate cases, and Nix compiler/install/upgrade integration. Linux proof is current;
   macOS and separately requested final release/CLI/install builds remain unverified.
8. Owned/borrowed service resources: complete.
   Acquisition is `Borrowed(BorrowedService)` or `Owned(StartingService)`;
   sessions accept only borrowed handles or ready owners. Borrowers retain only
   immutable evidence, with no child, monitor, guard, probe, source, or secret
   resources. Consuming readiness retains starting ownership on failure and
   releases guards only after the ready commit; failure cleanup and best-effort
   Drop remain inside guard ownership. Initial health failure snapshots service
   output before consuming ready ownership. Stop/cancel/stand consume handles;
   standing commit failure explicitly contains and settles, while success names
   monitor shutdown, relay detachment, and live child-handle release.
   Architect review found no correctness defect. Independent proofs cover
   competing startup before/after failed-ready settlement, failed standing with
   successful/failed settlement, failed health cleanup precedence, and real CLI
   health failure summary/evidence after committed readiness. The impossible
   pre-ready task dependency test is replaced by temporal registry revalidation
   after a real ready transition. All 73 service tests and workspace/all-target
   Clippy pass on Linux (`/tmp/nixfied-ownership-final-focused.log`). Contract,
   architecture, native descriptor coverage, and ABI snapshot updated together:
   `nixfied-runtime-abi:1-37edfe301f32`. Full current-tree `.#ci -- --dirty`
   passed (session `21135`, exit 0, `/tmp/nixfied-ownership-ci.log`), including
   source checks, fixture-backed Cargo, all 22 runtime cases, and Nix compiler/
   install/upgrade integration. macOS remains unverified. Bounded relay shutdown
   remains steps 9–10.
9. Bounded capture and child completion ABI cutover: complete.
   `OwnedBoundedChild` now owns task/probe spawn, polling, intent-before-signal,
   containment/reap, and capture completion. Tasks record between spawn and
   consuming completion; recording failure consumes the same owner through abort.
   The duplicate task spawn/wait/cleanup implementation is removed. Service start
   shares hermetic command configuration, while retaining long-lived ownership.
   Bounded capture always uses close-on-exec pipes, including empty redactors. Nonblocking
   workers check control/expiry before and after polling, read at most 8 KiB,
   poll at most 10 ms, and receive one shared 1,000 ms shutdown deadline before
   either is joined. EOF alone finalizes the redactor tail; incomplete capture
   discards it and closes files. Both streams are joined after errors/panics.
   Safe fixed incomplete messages survive cause projection; containment, stdout,
   stderr, and task outcome ordering has literal JSON goldens. No incomplete
   task evidence or replay is constructed. Architect review caught a pending
   control message after poll and an incidental cancellation-reason change; both
   are fixed, with a synchronized poll/EOF boundary proof and original event text.
   Independent test-child fixtures handshake a setsid escape holding idle or
   continuously written pipes. Four real CLI combinations (with/without secrets)
   prove bounded return, no replay/completed node, stable closed prefix files,
   undecided-tail secrecy, and a still-live test-owned survivor cleaned by tests.
   Additional proofs cover ordinary descendants, EOF tails/binary empty-redactor
   output, one failing worker while the other stays readable, partial creation,
   spawn failure, unrecorded abort/reap, and intent failure before signaling.
   Current focused proof: 124 runtime unit, 22 output, 73 service tests and Clippy
   pass (`/tmp/nixfied-capture-final-focused.log`). The fixture-backed `.#test`
   floor passed before final error-order fixes (`/tmp/nixfied-capture-floor.log`).
   Contract/guide/architecture and native descriptor coverage updated together;
   ABI snapshot is `nixfied-runtime-abi:1-aaed2238d5f9`. Full CI session `30624`
   failed at Postgres interruption recovery (`/tmp/nixfied-bounded-capture-ci.log`):
   `ps` returned no process rows. A focused rerun reproduced it. Review established
   that the test's TCP-only synchronization could interrupt before process-record
   commit; stale processes would still have been reported. The proof now waits
   for committed live service evidence before interruption. Separately restored
   long-lived no-secret service direct-file output; always-piped capture is the
   bounded task/probe policy, and should not change service orphan semantics.
   Focused recovery and Clippy pass (`/tmp/nixfied-service-output-policy.log`).
   Full current-tree CI passed (session `36838`, exit 0), including source
   checks, fixture-backed Cargo, runtime gates, and downstream Nix/install/upgrade
   integration (`/tmp/nixfied-bounded-capture-ci-repaired.log`).
10. Terminal/persistent capture integration is included in the atomic step 9 cut.
   Service failure, stop, cancellation, unrecorded get-pgid/recording failures,
   pre-spawn cancellation/spawn failure, and Drop all use bounded capture shutdown.
   Explicit cleanup no longer skips reap/capture after containment failure or
   silently discards capture errors. Registry escape settlement retains collected
   termination/reap evidence. Cancellation preserves containment/capture/intent/
   cancellation order; settlement failures retain their existing precedence.
   Persistent transfer sends a named control before detaching, so owner-channel
   disconnection cannot abort transferred relays. Ongoing redaction/EOF-tail
   behavior has a worker proof; existing persistent survival/borrow/release/down,
   until-idle, cancellation, identity/lease race, and escape service tests pass.
   macOS execution remains unverified; final acceptance must report that gap.
11. Node runner and occurrence evidence ABI cutover: complete.
   One concrete runner resolves authored dependencies and executes borrowed prepare
   and root plan nodes. The session owns one terminal TaskRun vector; root and
   selected projections retain private indices, with output constructed once at
   finalization. Prepare callbacks append directly and return unit results.
   Deleted PrepareTaskError, ServiceStart/ServiceStartError evidence shuttles,
   duplicate node result branches, and the test-only dependent-task wrapper.
   A separate monotonic attempted-occurrence allocator starts at zero and permits
   gaps before terminal evidence. Task logs and summaries use exclusive numeric
   occurrence filenames; probes retain their existing replaceable retry files.
   Summary collisions remain after-terminal and retain evidence/replay.
   Independent repeated-prepare/root overlap tests use distinct child output for
   all occurrences, including later prepare and root failures. Collision tests
   prove both stream errors with/without secrets, no child before successful log
   creation, preserved prior bytes, allocator gaps, and retained terminal records
   on summary failure. Architect review found no correctness defect.
   Clippy, 22 output tests, 74 service tests, and the collision/gap binary test
   pass (`/tmp/nixfied-evidence-suites.log`). The complete fixture-backed floor
   passed (session `63610`, `/tmp/nixfied-evidence-floor.log`). Contract, architecture,
   descriptor routing, and ABI snapshot updated together to
   `nixfied-runtime-abi:1-b0becb63c23d`. Full current-tree CI passed (session
   `30118`, exit 0, `/tmp/nixfied-evidence-ci.log`): source checks, fixture floor,
   all 22 runtime cases, and downstream Nix/install/upgrade fixtures. Committed
   as `3281b38`; macOS remains unverified.
12. Registry decoding/event context: implemented; full proof running.
   Registry lends disjoint connection, identity, and redactor references to
   service/control/cleanup transitions. Explicit deferred/immediate transaction
   modes, mutation order, temporal checks, redaction, and commit boundaries remain.
   One borrowed EventInsert replaces owned/borrowed event representations; lifecycle
   event-only writes reuse append_event. ProcessRecord no longer duplicates run
   and service IDs supplied by transition context; stored comparisons remain.
   StoredProcessIdentity owns shared encoding with task tracking omitted and
   service tracking present, including empty arrays. Literal wire tests preserve
   exact field order/presence. A trigger rejecting the second start event proves
   rollback of both event writes, service/process rows, and port ownership.
   Architect review found no correctness defects. Clippy, 40 service unit tests,
   18 registry, 74 service, 26 state, and 10 upgrade tests pass
   (`/tmp/nixfied-registry-context-complete.log`). Prepared in isolated worktree
   while step 11 CI ran; committed as `8f23846` and merged back. The temporary
   worktree was removed cleanly.
   Registry now owns shared open-endpoint decoding for start, activation, snapshots,
   and controls. Status/prefix/endpoint/loopback/port checks occur at that boundary;
   raw address spelling remains separate from parsed host so exact equality and
   event bytes stay unchanged. Duplicate process-layer endpoint parsing is deleted.
   One registry-owned active/escaped-with-owned-open-port SQL predicate serves
   query-specific readers without replacing multiplicity or first-conflict policy.
   Typed reconciliation observations replace public status-string interpretation;
   ps alone projects public records and down retains fresh rereads/pre-signal checks.
   Existing lifetime parsing now also closes control-row decoding.
   Independent tests cover malformed endpoint rows through all four consumers,
   no signaling/control event writes on refusal, corrupt lifetime refusal,
   snapshot multiplicity versus reservation precedence, exact alternate IPv6
   spelling, and complete ps/down output. The stale-port cleanup fixture had an
   accidental unprefixed key; repaired that positive fixture and retained separate
   malformed-key negatives. Clippy, eight registry unit proofs, 75 service,
   26 state, and 10 upgrade tests pass (`/tmp/nixfied-registry-boundary-tests.log`,
   `/tmp/nixfied-registry-relational-tests.log`). Full fixture-backed floor passed
   (session `71543`, `/tmp/nixfied-registry-owner-floor.log`). Final architect
   review found no correctness gaps; cross-layer CI remains required.
13. Shared redaction scanner/safe projections: pending.
14. Fixture consolidation and audit proof mapping: pending.

No completion claim: final acceptance audit, final measurements, full fixture-backed
test floor, CI/downstream gate, macOS process/endpoint/capture evidence, and release
package checks remain outstanding.
