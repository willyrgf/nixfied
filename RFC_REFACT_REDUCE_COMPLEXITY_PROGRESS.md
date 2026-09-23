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
   passed with warnings denied. `nix run .#ci -- --dirty` is running as exec
   session `90410`, with output in `/tmp/nixfied-complexity-ci-step1.log`.
   The first CI attempt rejected the missing native inventory coverage entry;
   that entry is now included. Re-poll the current handle before deciding its
   outcome. macOS remains unverified.
2. Impossible outcomes/dead ceremony: endpoint refinement now returns errors
   directly, deleting the fabricated success fallback. Cleanup terminal writes
   accept only `Deleted` or `Failed { safe_reason }`, and prior cleanup evidence
   stores a parsed status. All 24 state tests, 11 endpoint tests, six readiness
   tests, and workspace/all-target Clippy pass on Linux. Remaining: explicit
   lock-directory injection, applicable wrapper/ceremony subtractions, and the
   conditional listener representation review.
3. Placement characterization: pending.
4. Atomic registry initialization: pending.
5. Finalization error/cancellation ownership: pending.
6. Closed loaded/admitted construction: pending.
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
