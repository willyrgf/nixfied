# Runtime refactor outcome and evidence

The accepted ownership and correctness design was implemented. It removed
specific duplicate algorithms and invalid resource states, but **did not achieve
a substantial maintained-code reduction**. At implementation completion
(`9ba9a07`), the branch was **+2,361 repository lines against `cedfb91`**, the
reviewed `origin/dev`. Authored Rust production decreased by 368 lines against
that branch base, but only **126 lines (0.7%)** during RFC implementation.

This is a correctness and ownership improvement with modest production savings
and substantial additional tests. Neither completing the RFC checklist nor
counting removed duplicate groups establishes an overall complexity reduction.
The RFC prescribed new capture, ownership and evidence behavior while accepting
reduced duplication without requiring fewer total maintained lines.

The original chronological receipts are preserved in Git:
`git show 9ba9a07:RFC_REFACT_REDUCE_COMPLEXITY_PROGRESS.md`.
They are historical evidence, including the accounting error corrected below,
not current acceptance criteria. The [RFC](RFC_REFACT_REDUCE_COMPLEXITY.md)
retains the accepted design; [CONTRACT.md](docs/CONTRACT.md) remains normative.

## Corrected implementation accounting

These tables describe the completed implementation at **`9ba9a07`**, before the
subsequent proof repair and report condensation. The RFC review baseline
`0570aa3` and implementation baseline `d6b5632` have identical Rust line counts.
The branch base is pinned to `cedfb91`, rather than a moving remote ref.

| Rust category | Branch base `cedfb91` | Implementation base `d6b5632` | Completed `9ba9a07` | Branch delta | Implementation delta |
| --- | ---: | ---: | ---: | ---: | ---: |
| Authored production | 18,621 | 18,379 | 18,253 | -368 | -126 |
| Unit tests | 4,075 | 4,155 | 5,260 | +1,185 | +1,105 |
| Integration/common tests and manifest fixtures | 12,309 | 12,099 | 12,410 | +101 | +311 |
| Generated Rust | 817 | 817 | 817 | 0 | 0 |
| Private test child | 653 | 643 | 686 | +33 | +43 |
| **Total Rust** | **36,475** | **36,093** | **37,426** | **+951** | **+1,333** |

| Repository category, `cedfb91` to `9ba9a07` | Net physical lines |
| --- | ---: |
| RFC and progress report | +1,044 |
| Other documentation and AGENTS | +194 |
| Rust | +951 |
| Nix, workflow and other fixtures | +172 |
| **Total** | **+2,361** |

Removing the two RFC/report files alone would still leave +1,317 lines at that
revision. Of the branch's 368 production lines removed, 242 were removed before
RFC implementation began. Documentation condensation is a separate maintenance
saving; it does not retroactively improve the implementation's code reduction.

Reproduce repository deltas with `git diff --numstat cedfb91 9ba9a07`.
For Rust, enumerate tracked `runtime/**/*.rs` with `git ls-tree -r --name-only`
at each pinned revision and read each file with `git show REV:PATH`. Count
physical lines, including comments and blanks. Classify generated paths first,
then the test-child crate, `tests/` paths and `fixtures.rs`, then standalone
`*_tests.rs` files. For remaining source, split at the first
`#[cfg(test)]` followed by `mod tests {`: the suffix is unit tests and the
prefix production. This procedure describes these audited trees, not a general
Rust parser; inspect new test-module layouts before reusing it.

The old procedure missed standalone `main_tests.rs` and `command_tests.rs`.
It therefore reported production as 18,756 -> 18,821 (+65) and embedded tests
as 3,778 -> 4,692 (+914). The corrected production and combined unit-test
categories above replace those figures; total Rust was already correct.

## What the additional code bought

Whole-commit Rust deltas against each commit's parent:

| Change | Production | Unit/integration tests and fixtures | Test child |
| --- | ---: | ---: | ---: |
| Graph/invocation cutover, `903d516` | -217 | +359 | 0 |
| Service ownership, `bf8cb1f` | +160 | +232 | 0 |
| Bounded capture, `210aedd` | +306 | +491 | +26 |
| Node/evidence ownership, `3281b38` | -134 | +256 | +17 |

Capture contributes +823 Rust lines. Outside that commit, implementation
production is -432 lines, not the previously reported -241. These are net
commit deltas, including sharing and deletion; they do not isolate pure feature
costs or make the other commits behavior-preserving refactors.

The inspected reductions comprise 12 duplicate implementation groups: graph
interpretation; invocation resolution; template/secret scanning; admission
store-root observation; source confinement; task/probe completion; root/prepare
node execution; stored endpoint decoding; actionable escaped-process SQL;
process identity encoding; redaction scanning; and sigaction setup.

Five redundant representation groups were removed: separate finalization causes;
prepare/root evidence shuttles; duplicated transition process IDs; owned/borrowed
event insertion records; and service-fixture raw manifest shadows. Private
admission and distinct service owners additionally restrict invalid construction.
Fixture/helper relocation itself is not counted as deletion. Independent Nix
implementations and literal test oracles remain intentional independent proofs.

Eight behavior coordinates were added to the existing ABI inventory:
substitution-grammar, relational-admission, placement-components,
probe-settlement, service-ownership, task-evidence, bounded-capture and
finalization. They carry ongoing descriptor/docs/test coordination costs.
The inventory of removed groups is qualitative evidence, not a measured net
complexity score. No new daemon, persistent cache or compatibility reader was added.

## Implementation and proof map

| RFC sections | Owner and retained proof |
| --- | --- |
| 1: admission | Private `ValidatedManifest`, `ControlAdmission` and `RunAdmission`; admission and output tests cover rejection order, recovery independence and no effects on invalid admission. |
| 2-3: graph/invocations | `execution/plan.rs`, lowering and `template.rs`; independent Nix/Rust vectors, raw negatives and execution tests cover carried facts, endpoint scope, opaque insertion and cwd confinement. |
| 4: services | Starting/ready owners and borrowed handles in `service/process.rs`; service tests cover guarded readiness, borrowing, health failure, failed standing and persistent survival. |
| 5: capture | Shared bounded-child completion and `redaction.rs`; worker/CLI tests cover EOF, shared deadlines, both-worker settlement, escaped writers and incomplete evidence refusal. See the repaired proof below. |
| 6: evidence | `execute_node` and `RunEvidence`; exclusive occurrence files and repeated-prepare/root tests preserve distinct output and completed evidence after later failure. |
| 7: registry | Typed readers/context in registry owners; corruption, rollback, contention, reuse, lease and pre-signal identity tests remain. |
| 8-9: placement/outcomes | Direct checked path composition and terminal/error alternatives; state/endpoint tests retain confinement, cleanup evidence, lock and listener diagnostics. |
| 10-11: finalization/initialization | RuntimeError owns causes; finalization observes cancellation once; immediate schema transactions publish identity/version atomically. Priority, blocked-replay cancellation, crash, rollback and concurrent-creator tests remain. |
| 12: redaction/projection | One byte scanner with distinct caller diagnostics; binary/overlap/all-split, partial-write and safe-error proofs remain. |
| 13-14: fixtures/audits | Coherent admitted fixtures and shared CLI setup; DEVELOPMENT maps deleted approximate audits to structural, freshness, raw-wire, ABI and behavioral proofs. |

The exact runtime ABI at implementation completion is
`nixfied-runtime-abi:1-b0becb63c23d`. The proof repair changes neither runtime
semantics nor the descriptor, numeric versions or compatibility policy.

## Capture proof repair

Review removed the production post-poll control check in a temporary copy and
found that `shutdown_arriving_during_poll_is_observed_before_eof` still passed:
it repeated a polling sequence instead of calling the production capture loop.
This was a coverage defect; the production checkpoint was present.

The replacement, `shutdown_at_poll_boundary_rejects_readable_eof`, calls the
same loop used by production through a private poll-operation parameter. Its
poll callback queues an expired shutdown after the initial control check,
closes the peer and calls real OS poll. Completion must be `Incomplete` even
though EOF is readable. The sender remains connected, so disconnection cannot
satisfy the assertion accidentally. Existing tests cover accepted EOF and
undecided-tail discard. Architect review approved this bounded testability change.

The repaired test passes normally and fails when only the production post-poll
check is deleted in an isolated copy. No scheduling delay or copied capture loop
is used. Verification receipts for this follow-up are recorded below.

Proof-repair commit `00db53a` adds 12 production lines and one unit-test line.
Rust therefore totals 37,439 lines: production 18,265, unit tests 5,261, with
the other categories unchanged. Production is now -356 against the branch base
and -114 against the implementation baseline. The additional private wrapper
buys a falsifiable boundary proof; it is not counted as code reduction.

## Verification and remaining limits

Historical implementation receipts, preserved in the original report:

- Linux `.#test` passed, including realized Postgres recovery; public CLI,
  installer and optimized runtime builds passed.
- Full `.#ci -- --dirty` passed for `c324538` plus the gate port adjustment
  committed as `716b403`, including all 22 runtime-gate cases and downstream
  install/upgrade fixtures. Later implementation commits changed workflow/docs.
- An earlier gate collision at port 34980 and one service-marker startup failure
  were not conclusively diagnosed. The gate port window moved below the host's
  ephemeral range; subsequent suites passed. Passing reruns do not establish
  either original cause.
- At review revision `9ba9a07`, a fresh fixture-backed `.#test` passed 378 tests,
  including realized Postgres recovery.

Follow-up verification for the source committed as `00db53a`:

- All nine redaction unit tests pass. In the isolated mutant, deleting only the
  post-poll checkpoint makes the new test fail at the `Incomplete` assertion.
- `cargo fmt --all -- --check` and workspace/all-target Clippy with warnings
  denied pass in the pinned development shell.
- `nix run .#test` passes all 378 tests, including realized Postgres recovery.
- Full cross-layer CI and public release/package builds were not rerun for this
  proof repair; production polling behavior and ABI semantics are unchanged.

The condensed report and RFC status correction change documentation only.

**macOS execution remains unverified.** The prior explicit deferral to hosted CI
is retained. The workflow includes runtime unit, endpoint, service and output
tests; configuring it is not execution evidence. Linux and macOS remain
supported targets. Historical full CI/package receipts do not certify later
source revisions or replace the outstanding macOS run.
