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
   tests, and workspace/all-target Clippy pass on Linux. Applicable small
   subtractions are implemented as described below.
   Explicit lock-root injection now replaces ambient thread-local state and fd
   duplication/restoration. The endpoint owner supplies a validated descriptor;
   a private acquisition loop receives the validated lock directory. Fixed-root
   production traversal and the original acquisition phase are retained. Named
   directory permission policies replace the magic sticky-bit/mode argument.
   Existing mode, symlink/nonregular, nonblocking, close-on-exec, partial-release,
   stable-inode, and real-start-before-prepare proofs remain; an unsafe lock child
   also proves that endpoint-less acquisition bypasses directory access.
   Clippy, 23 endpoint unit tests, 11 endpoint integration tests, and 75 service
   tests pass (`/tmp/nixfied-explicit-lock-final-tests.log`). Architect review
   found no defect and identified a redundant root validation, now removed;
   the final narrow review check is recorded in
   `/tmp/nixfied-explicit-lock-review-tests.log`.
   Conditional listener representation review is complete: retain IpAddr plus
   optional IPv6-only evidence. Linux accepts that attribute on IPv4 records and
   preserves it in diagnostic JSON; V4(address) would discard accepted evidence,
   while a lossless V4-with-optional-mode enum adds complexity. A literal parser-to-
   JSON test pins IPv4 missing/false/true mode evidence; the existing IPv6 missing-
   evidence rejection and dual-stack classification tests remain. Architect review
   agrees with retention (`/tmp/nixfied-listener-representation-tests.log`).
   Signal installation now shares one native install/save helper. Cancellation
   and SIGPIPE diagnostic mappings, rollback, and reverse restoration remain
   explicit. The health convenience wrapper is gone; health callers supply the
   cancellation token directly. Both stop methods remain intentionally: main uses
   uncancellable cleanup after failure and cancellation-aware normal completion.
   Readiness already requires a token; the dependent-task wrapper was removed in
   step 11. Slot placement validation reuses the already-checked policy and iterates
   its inclusive range directly, deleting the repeated validation and temporary
   vector. The sole validation caller preserves rejection order.
   Identity hashing replaces the unreachable empty-string fallback with expect
   at its private owner. Architect review verified all concrete inputs serialize
   strings/string-keyed maps/sequences/enums/booleans/integers; no fallible paths or
   custom error serializer is admitted. to_string and field order remain unchanged,
   so there is no reachable rejection-boundary or ABI change. Independent literal
   endpoint/state component digests pin JSON order and length/domain framing.
   Clippy, manifest tests, the hash proof, 73 service tests, both CLI signal tests,
   and 22 output tests pass (`/tmp/nixfied-smaller-subtractions-tests.log`,
   `/tmp/nixfied-signal-helper-tests.log`). Full fixture-backed floor is running
   (session `24863`, `/tmp/nixfied-refactor-consolidated-floor.log`).
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
12. Registry decoding/event context: complete.
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
   review found no correctness gaps. Full cross-layer CI passed (session `99879`,
   exit 0, `/tmp/nixfied-registry-owner-ci.log`), including downstream
   Nix/install/upgrade fixtures; macOS remains unverified.
13. Shared redaction scanner/safe projections: implemented.
   Whole-buffer and streaming redaction share one private scanner with an eligible
   match-start limit and actual consumed count. Longest-first matching, binary
   bytes, EOF-only tail flushing, and bounded capture shutdown remain unchanged.
   Literal overlapping-pattern output is checked at every pair of chunk splits,
   including empty chunks and an empty redactor. Summary/footer diagnostics now
   use the output owner's narrow constructor; exact safe-field tests retain its
   distinct interrupted/not-found mapping from replay. Native error spelling has
   one owner; the all-variants no-op cause match is deleted. Replay's partial-write,
   per-stream, and native/lossy-path policies remain intact.
   Clippy, 22 output tests, all 131 runtime unit tests, and the full fixture-backed
   floor passed (`/tmp/nixfied-redaction-sharing-tests.log`,
   `/tmp/nixfied-redaction-unit-tests.log`, `/tmp/nixfied-redaction-floor.log`).
   Architect review found no defects. Final cross-layer CI remains required.
14. Fixture consolidation and audit proof mapping: implemented; final verification below.
   RuntimeFixture now lives in tests/common and writes raw scenario bytes without
   validating or repairing them. Its command method returns ordinary Command;
   output tests and both real CLI signal/cancellation tests share it. Tests keep
   their explicit command environment and raw corruption paths.
   One CLI child now proves exact tool-root PATH, declared variables, and absence
   of a per-command parent canary. Deleted the separate PATH run and unsafe global
   environment mutation. Leaf output fixtures and the environment/secret proofs
   no longer bind ports for unused service metadata; service-dependent output
   fixtures retain real available-port selection and lifecycle coverage.
   Deleted raw_len readback beside the exact literal wire assertion and the
   positive redaction-token tree search beside exact redacted stdout; whole-tree
   secret absence remains. Merged slot-identity-only checks into the executed
   two-slot isolation proof, which compares actual instance IDs and stored address
   hashes while retaining control/cleanup isolation checks.
   Clippy, all 22 output tests, 73 service tests, and the literal local-record wire
   test pass (`/tmp/nixfied-cli-fixture-tests.log`,
   `/tmp/nixfied-fixture-consolidation-tests.log`,
   `/tmp/nixfied-leaf-fixture-tests.log`). The augmented two-slot proof is recorded
   in `/tmp/nixfied-slot-fixture-proof.log`.
   ServiceFixture now owns only the admitted scenario, placement, registry, and
   workspace lifetime. Deleted the mutable raw Manifest copy, seven readmit sites,
   and the duplicate typed argv mutator. Task timeout/argv and containment deltas
   are configured in raw JSON before admission. The cwd race prepares its directory
   before the sole admission, then replaces it with a symlink after admission.
   The live-owner collision test separately admits its second scenario instead of
   overwriting the owner's fixture. All fixture document reads borrow admission's
   immutable validated manifest.
   Service start, endpoint-less start, and prepare start are fixture methods;
   67 repeated manifest/admission/placement/registry argument groups and three
   forwarding helpers are removed. One shared run-recording/start setup remains
   for independently selected slot and upgrade scenarios.
   Clippy, 73 service tests, and ten upgrade tests pass after the method cutover
   (`/tmp/nixfied-service-start-method-tests.log`); final endpoint-less/prepare
   method checks are in `/tmp/nixfied-service-fixture-final-tests.log`.
   Approximate-audit retirement is complete. DEVELOPMENT maps each removed
   fixture-token/history blacklist, error/exit sentinel/list, and fixed-count
   assertion to its owning structural, freshness, raw-wire, literal ABI, or
   behavioral proof. The mapping explicitly limits decoder negatives to the
   represented policies; it does not claim every record has a separate unknown-
   field test. AGENTS now routes new fields through this replacement coverage.
   Exact whole-inventory/native-owner routing, per-record equality, explicit
   local-record inventory, independent literal wire tests, and required-field
   maintenance exercises remain. Added missing-field and same-count renamed-field
   declaration rejection vectors. Removed the default Error::source readback;
   literal error JSON remains and also pins ExitClass::Ok spelling.
   Architect review found no coverage gap blocking retirement. Clippy, manifest
   tests, error unit tests, and output_structure tests pass
   (`/tmp/nixfied-audit-replacement-tests.log`). The hermetic rust-workspace source
   check passed, including generation/reference freshness and maintenance exercises
   (`/tmp/nixfied-audit-source-check.log`). Current structure/coverage Nix evaluation
   also passed after the new field-rejection vectors. Final full CI remains due.
   Registry identity setup is now a table retaining project/environment/ABI/
   toolchain inputs, exact field/path diagnostics and error classes; each refusal
   also reopens the unchanged original identity. The stored slot-one/slot-zero
   mismatch proof remains separate. Cleanup tables retain both standard and purge
   unmarked/mismatched-marker cases and nondeletion assertions, plus protected and
   persistent standard refusals. Both successful purge variants now assert the
   original intent/terminal row, purge event flags, and deleted state.
   Clippy, 15 registry tests, and 23 state tests pass
   (`/tmp/nixfied-registry-fixture-tables.log`); fewer test functions contain all
   original scenario inputs and modes. Five additional composite/control CLI
   scenarios now use RuntimeFixture; lifecycle reuses common runtime_binary.
   The control identity-mismatch proof no longer acquires an unused TCP port.
   Environment-selected state-root tests and commands exercising distinct explicit
   state bases retain their purposeful command setup; endpoint's environment-root
   builder remains separate from the shared fixture's explicit --state-base policy.
   Clippy and all 73 service tests pass (`/tmp/nixfied-cli-fixture-adoption.log`).
   Architect's implementation-scope audit found no further production omissions;
   conditional/deferred investigations remain as specified by the RFC.

   Full fixture floor session `24863` failed once because the shutdown-signal test
   did not observe its initial service marker. Its assertion discarded diagnostics,
   so the cause is unproven. The test now signals/reaps its child on a missing
   marker and reports status/stdout/stderr without changing its deadlines or success
   assertions. Two subsequent complete service suites passed, including that case
   (`/tmp/nixfied-shutdown-diagnostic-suite.log`,
   `/tmp/nixfied-cli-fixture-adoption.log`). The complete fixture-backed floor is
   running again as session `2926` (`/tmp/nixfied-refactor-final-floor.log`).
   This is additional evidence gathering, not a claimed root-cause fix.

The implementation audit, measurements, complete Linux fixture floor, and public
package builds are recorded below. Full CI is still running; macOS execution
remains unavailable. No full acceptance or completion claim is made.

## Final implementation audit

Audited implementation revision: `c324538` (baseline `d6b5632`). This is an
implementation/proof map, not a claim that unavailable platform checks passed.
The ordered delivery record above uses commit order; the table below uses the
RFC's design section numbers.

| RFC requirement | Current owner and inspected proof |
| --- | --- |
| 1. Closed admission | `ValidatedManifest` has a private field and fallible construction; loaded/run/control fields are private. `lower` requires the validated document. Admission tests cover origin-before-parse, host/graph ordering, provenance, secret/source/closure refusal, control independence, and `output.rs::unused_graph_and_template_faults_reject_before_state_or_child_effects` asserts MANIFEST_ADMISSION with absent state and child markers. |
| 2. One Rust graph interpretation | `execution/plan.rs` owns flattening, combined service edges, cycle/union/capacity proof, logical planning and slot binding. Lowering calls those same proofs; runtime plans borrow admitted data. Independent Nix derivation vectors and literal Rust goldens remain. |
| 3. Resolved invocations/templates | `ResolvedInvocation`, checked cwd and parsed templates are constructed during lowering; runtime consumes them with opaque inserted values. Shared template vectors, raw admission negatives and execution tests cover literal run[0], argument/env parsing, secret restrictions, operation/endpoint scope and temporal cwd confinement. |
| 4. Ownership | Acquired/started owned and borrowed alternatives carry different resources. Ready/standing transitions consume ownership; failed transitions settle owned resources. Service tests cover borrowing, persistent survival, until-idle leases, readiness/health rollback, failed standing and exact error precedence. |
| 5. Bounded children/capture | Task/probe completion shares the bounded-child owner; all bounded capture uses pipes, an absolute deadline and EOF-only completion. Both workers join; incomplete evidence cannot produce replay. Unit and CLI tests cover empty/secret redactors, escaped idle/continuous writers, partial failure and persistent transfer. Linux evidence exists; macOS execution remains missing. |
| 6. Node/evidence owner | `execute_node` and `RunEvidence` own terminal records once. Monotonic attempted occurrences are separate from terminal indices; logs/summaries use exclusive creation. Repeated prepare/root and later-failure tests pin distinct literal child output; collision tests pin refusal before child start and preservation after terminal evidence. |
| 7. Registry readers/context | Registry owns typed endpoint/lifetime/status decoding, the actionable escaped-process predicate and borrowed event/context access. Controls consume native observations and ps alone projects wire records. Corruption, multiplicity, alternate IPv6 spelling, rollback, contention, reuse, lease and pre-signal identity tests remain. No new persistent observation cache; effect boundaries retain fresh identity and transaction-local ownership checks. |
| 8. Direct placement | State placement joins checked single components; the template language is deleted. Both-slot paths, narrowed component negatives, root/nested symlink confinement and registry-survival cleanup tests pin the boundary. |
| 9. Possible outcomes | Endpoint refinement returns errors directly; cleanup terminal state excludes intent. Validated endpoint lock roots are explicitly supplied. Unsafe targets, nonblocking/CLOEXEC/partial-release and real-start-before-prepare tests remain. Listener representation is deliberately retained because IPv4 diagnostic mode evidence is accepted and must survive. |
| 10. Failure/cancellation owner | RuntimeError owns causes; finalization records its cancellation observation once. Literal priority/flattening/cause-order tests and cancellation during blocked replay prove ordering and continued cleanup. |
| 11. Atomic initialization | Registry creation classifies, creates schema, writes identity and version in one immediate transaction. Tests cover concurrent initialization, injected write/version failures, process exit before commit, reopen and rejection without migration. |
| 12. Redaction/projection reuse | One scanner serves whole-buffer and streaming output. Output owns the narrow summary/footer constructor; replay retains distinct mappings and partial-progress handling. Literal binary/overlap/all-split tests, safe diagnostics and real broken-pipe output tests remain. |
| 13. Fixtures/proof repair | CLI setup is shared where semantics coincide; admitted service fixtures own no mutable raw shadow. Start operations are fixture methods. Timing/scope/version repairs, merged environment/slot proofs, identity/cleanup tables and removed readback assertions are recorded above. Raw SQL/JSON corruption remains explicit. |
| 14. Audit retirement | DEVELOPMENT maps retired approximate audits to exact inventory/record checks, freshness, independent ABI/wire and behavioral proofs. AGENTS routing changed atomically. Missing/renamed field vectors, native-owner/local-record routing and maintenance exercises remain. |
| Smaller subtractions | Shared sigaction setup retains rollback/reverse restoration and caller diagnostics. Redundant slot validation/vector and health wrapper are removed. Identity hashing keeps direct serialization and literal component hashes, with no reachable new error boundary. Both real stop policies remain. |
| ABI delivery | Descriptor additions and literal ABI snapshot are coupled to producers, consumers, docs and fixtures in coherent commits. Current ABI is `nixfied-runtime-abi:1-b0becb63c23d`; no numeric version change or old-byte compatibility path was introduced. |

## Final measurement and change-site accounting

Same physical-line classification as the baseline, measured from tracked Rust at
`c324538`; comments/blanks included. These are not percentage targets.

| Category | Baseline | Final | Delta |
| --- | ---: | ---: | ---: |
| Authored production | 18,756 | 18,821 | +65 |
| Embedded test modules | 3,778 | 4,692 | +914 |
| Integration/common tests and manifest fixtures | 12,099 | 12,410 | +311 |
| Generated Rust | 817 | 817 | 0 |
| Private test child | 643 | 686 | +43 |
| Total | 36,093 | 37,426 | +1,333 |

The bounded-child/capture correctness commit `210aedd` contributes +306 authored
production, +357 embedded test, +134 integration/fixture and +26 test-child lines.
This is a net commit delta that includes sharing/deletion as well as the new
protocol, not a claim that every added line implements capture. Outside that
commit, authored production is net -241 lines. Ownership transition fixes and
occurrence-evidence correctness also add code; test growth is retained evidence,
not classified as savings. No generated formatting reduction is claimed.

Counted as 12 removed duplicate implementation groups (group definitions below,
not individual helper functions): graph interpretation; invocation resolution;
template matching/rendering and secret-reference scanning; admission store-root
observation; source confinement; bounded task/probe completion; root/prepare node
execution; stored endpoint decoding; actionable escaped-process SQL; process
identity encoding; whole-buffer/streaming redaction scanning; sigaction setup.
Each now has one named owner described above. Independent Nix derivation and
literal test oracles are deliberately not counted as duplication to remove.

Five removed redundant storage/representation groups: separate finalization cause storage;
prepare/root terminal evidence shuttles; process-record IDs duplicated by transition
context; owned versus borrowed event insertion records; and service fixture raw
manifest shadows. Private admission APIs additionally remove external mutation
of proof fields; owned/borrowed service alternatives remove incoherent correlated
resource states. These representation changes are distinct from LOC movement.

Moved code includes RuntimeFixture/command construction into tests/common,
summary/footer projection construction into output, and lifecycle setup into
fixture methods; movement itself is not credited as deletion. Removed logic and
storage are listed separately above. Test setup sharing reduces coordinated test
edits while retaining raw adversarial boundary paths.

No new semantic seam, registry, daemon, compatibility path, mutable cache or
external authority was added. Eight newly explicit ABI behavior coordinates
(substitution-grammar, relational-admission, placement-components, probe-settlement,
service-ownership, task-evidence, bounded-capture, finalization) require descriptor
and existing native coverage routing entries. Their behavior changes also require
the existing ABI snapshot/docs/proof cutover; those coordinated obligations are
reported as added contract documentation, not hidden as refactor savings.

## Verification receipts and remaining evidence

- Linux fixture-backed floor: session `2926`, exit 0,
  `/tmp/nixfied-refactor-final-floor.log`. The realized Postgres recovery test ran
  and passed; it was not silently skipped.
- Public CLI, installer, and optimized runtime builds: session `35327`, exit 0,
  `/tmp/nixfied-refactor-final-packages.log`.
- Initial final CI: session `84150`, exit 30. Source checks and the floor passed;
  runtime gate slot 1 reported PORT_UNVERIFIABLE at 34980 (bind occupied without
  observable listener). The exact collision source was not established. The
  gate-only slot override moved from 34880 to 31880, below this host's ephemeral
  range 32768–60999; both proposed windows were bindable before the rerun.
  Production endpoint refusal semantics are unchanged.
- Repaired final CI: session `96889`, still running when this draft was prepared.
  All 22 runtime-gate cases, including both slots, passed; downstream Nix/install/
  upgrade fixtures are still running. Log: `/tmp/nixfied-refactor-final-ci-repaired.log`.
- Earlier full floor session `24863` failed once on the shutdown test's initial
  service marker. Its failure path now reaps the child and reports diagnostics,
  without changing time limits or success assertions. Subsequent service suites
  and the complete floor passed. The original failure's cause remains unproven.
- macOS execution is unverified. The current host is aarch64-linux, with no
  extra execution platform or configured remote builder (`/etc/nix/machines`
  absent). The user has been asked for an available macOS host/runner. The hosted
  macOS workflow now runs service/output suites alongside unit/endpoint tests
  (commit `5f586a4`); YAML and shell syntax have been checked, not macOS execution.

Implementation evidence is not full RFC acceptance while the required macOS
process/endpoint/capture proof is missing. No completion claim is made here.
