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
   Remaining in this step: one store-root observation per admission.
7. Graph, invocation, template, and placement ABI cutover: pending.
8. Owned/borrowed service resources: pending.
9. Bounded capture and child completion ABI cutover: pending.
10. Terminal/persistent capture integration: pending.
11. Borrowed plans, node runner, and occurrence evidence ABI cutover: pending.
12. Registry decoding/event context: pending.
13. Shared redaction scanner/safe projections: pending.
14. Fixture consolidation and audit proof mapping: pending.

No completion claim: final acceptance audit, final measurements, full fixture-backed
test floor, CI/downstream gate, macOS process/endpoint/capture evidence, and release
package checks remain outstanding.
