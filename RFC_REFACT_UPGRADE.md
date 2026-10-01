# Refactor upgrade around inspection and guarded repinning

Status: implementation-ready proposal, reviewed with the upgrade architect.
This RFC specifies the intended cutover; it does not describe already shipped
behavior. [CONTRACT.md](docs/CONTRACT.md#output-and-failure-contract) remains
normative until implementation updates UPGRADE-1 and its coupled surfaces.

## Decision

Give `upgrade` one narrow responsibility: report upstream changes relative to
the adopter's locked Nixfied source, then mechanically repin the Nixfied input
under an explicit checking policy.

Keep the existing command shape and add only `--force`:

```sh
nix run github:willyrgf/nixfied#upgrade -- --root . --plan
nix run github:willyrgf/nixfied#upgrade -- --root .
nix run github:willyrgf/nixfied#upgrade -- --root . --force
```

Within the framework checkout, `nix run .#upgrade` supplies the same command.
Adopter-generated apps do not export `upgrade`; an adopter's `.#upgrade` is not
a required entrypoint. Use the supplying framework reference and `--root` so
inspection does not depend on an adopter's generated apps evaluating.

| Mode | Responsibility | Candidate manifest evaluation | Project writes |
| --- | --- | --- | --- |
| `--plan` | Resolve identities and present source documentation and proposed pin/lock changes | Never run | None |
| Default apply | Resolve, report, evaluate, and attempt guarded repinning | Required; failure blocks writes | Pin/lock only |
| `--force` | Resolve, report, and attempt guarded repinning | Explicitly skipped | Pin/lock only |

Force changes the checking policy, not the write authority. It does not turn
resolution failures, ambiguous URL rewrites, concurrent edits, or failed file
operations into success.

## Problem and evidence

Today [upgrade.nix](nix/install/upgrade.nix) resolves a temporary candidate lock,
prints the source documentation diff, and calls `verify_candidate`. That helper
evaluates `manifest.drvPath` and exits 5 on failure before the complete change
summary and next steps. Plan uses the same fatal gate as apply. UPGRADE-1 and
the incompatible-plan case in [gate-nix.nix](nix/gate-nix.nix) require this
behavior, so the correction must change the contract and tests together.

The observed MFM2 upgrade from `6b3a70b` to `bae31c5` resolved and compared the
sources, then failed because the project still exported `model` through
`compileModel`. Updating that wiring in a temporary copy exposed missing Reth
endpoint probe commands and the old list form of `surface.verbs`. A project
that needs these edits still needs a useful upstream inspection.

The current gate also proves less than the phrase "candidate verification"
suggests. Evaluating a derivation is not building it, admitting its realised
manifest, starting services, or checking application data. Some breaking
changes fail this evaluation; others pass it and fail later. Upgrade must not
claim comprehensive compatibility in either case.

## Invariants and owners

1. Inspection is independent of candidate project evaluation. No plan or
   forced locked apply invokes the adopter's `manifest.drvPath` evaluation.
2. Checked apply cannot enter the write path after failed evaluation. Forced
   apply enters it only because the caller explicitly selected `--force`.
3. Resolution, reporting, evaluation when required, and writes use the same
   temporary candidate lock within one invocation. Do not resolve again between
   evaluation and application.
4. Plan changes none of `flake.nix`, `flake.lock`, or `nixfied.nix`. Apply owns
   only the requested Nixfied URL assignment and candidate lock update. Existing
   captured-file conflict checks and rollback behavior remain in force.
5. Source documentation is best-effort evidence. Unavailable documentation is
   distinct from identical documentation, and neither establishes compatibility.
6. Upgrade never edits declarations or performs runtime recovery, cleanup, or
   application-data migration. Runtime admission retains its own boundaries.

Nix owns input resolution, fetching, and evaluation. The Nix-packaged upgrade
tool owns mode dispatch, presentation, and guarded pin/lock application. The
adopter owns compile wiring, declarations, scripts, and application data. The
matching runtime owns admission, sessions, registry history, and state recovery.

Normalize parser flags once into the conceptual alternatives `Plan`,
`ApplyChecked`, and `ApplyForced`; the existing native implementation need not
introduce a Rust type or generated mode schema. Only the checked alternative
executes the evaluation gate. Both applying alternatives share one write path.
Do not retain a hidden "plan but requires preflight" alternative or add a
second forced transaction implementation.

## Command semantics and rejection boundaries

Add `force` as an idempotent flag, default `false`, in
[commands.nix](nix/meta/commands.nix). Its help text must say that it skips
candidate manifest evaluation when applying and retains write safeguards.
Preserve existing operand, repetition, help precedence, and error behavior.

`--plan` always wins over `--force`, independent of argument order. For example,
`--force --plan --force` is a normal nonmutating plan and reports evaluation as
not run because of plan. Do not introduce a combination error or force prompt.

Keep the existing `--root`, `--nixfied-url`, and `--no-lock` behavior.
`--no-lock` remains the separate mechanical URL-only path: it resolves no
candidate lock, writes no lock, and skips documentation comparison and
evaluation. `--force` has no additional effect there; retain the `--no-lock`
skip explanation. This RFC neither removes that mode nor merges it into force.

Locked plan, checked apply, and forced apply all require the existing lock to
identify the old source, an identifiable Nixfied input, and a resolvable
candidate. Inspecting input declarations is allowed; evaluating the adopter's
manifest or generated apps is not part of plan. A malformed flake/input graph
can still prevent resolution. Force does not repair malformed input syntax.

Preserve existing exit classes:

| Result | Exit status |
| --- | --- |
| Complete plan, including unavailable source documentation | `0` |
| Successful apply or candidate already matches | `0` |
| Native argument error | `2` |
| Missing/unsupported project wiring, ambiguous rewrite, or required old lock absent | `3` |
| Candidate lock resolution failure | `4` |
| Checked apply manifest evaluation failure | `5` |
| Captured-file conflict | `6` |
| Apply failure with the existing restored/unchanged outcome | `7` |
| Rollback failure requiring inspection | `8` |
| Existing caught apply interruption path | `130` |

Exit 5 is no longer a plan outcome. A Nix evaluation failure remains an
evaluation failure with its original diagnostic; do not classify arbitrary
stderr as an intentional breaking change or an environmental problem.
Before-write failure paths preserve project files. For partial-write failures,
report the existing rollback outcome accurately rather than claiming that no
write ever occurred. Preserve current signal behavior outside the caught
transaction window; this RFC introduces no new signal protocol.

## Reporting contract

Retain old/candidate type, original source, available revision, and NAR hash
identity blocks. Each invocation prominently identifies its actual candidate.
Retain the framed unified diff for source `README.md` and regular files under
`docs/` as the only stdout payload in lock-refresh mode. Status, warnings, next
steps, and Nix diagnostics stay on stderr. Help keeps its existing stdout behavior.

Keep the existing no-documentation-change and documentation-unavailable markers.
Documentation acquisition or comparison failure is advisory in all three modes
after candidate resolution succeeds. This is a source documentation comparison,
not an exhaustive inventory of semantic changes or a compatibility proof.

Replace the broad lock-refresh label `candidate verification` with the following
status lines, emitted once by the result reporter for the outcomes it handles:

| Outcome | Evaluation status |
| --- | --- |
| Plan | `candidate manifest evaluation: not run (--plan)` |
| Completed checked apply or checked no-op | `candidate manifest evaluation: passed` |
| Checked apply fails evaluation | `candidate manifest evaluation: failed` |
| Completed forced apply or forced no-op | `candidate manifest evaluation: skipped (--force)` |

Keep `would change`, `changed`, `unchanged`, `upgrade applied`, and declaration
preservation reporting. On failed evaluation, report the proposed changes as
blocked, not applied, and include next steps. Keep post-upgrade validation
explicitly not run; build and manifest-check remain recommended commands.

Plan next steps must say that a subsequent invocation resolves upstream again.
Do not say that rerunning without plan applies the exact previously inspected
candidate. Checked rejection should suggest editing declarations and replanning,
or explicitly forcing a repin and repairing afterward. Force must not be an
automatic retry after a failed checked apply.

Use one complete result reporter for the lock-refresh path. Derive descriptions
and per-file action words from the normalized mode and outcome; do not copy
plan, checked, forced, or rejection summaries into independent implementations.
Retain Nix diagnostics without adding an error-string parser, JSON report, or
new persistent result schema.

The common reporter handles plan, checked evaluation rejection, and completed
apply/no-op. Resolution, transaction, rollback, and interruption failures retain
their existing owning diagnostic handlers. Passed/skipped evaluation lines need
not appear when a transaction fails or is interrupted; no completed-result
report is emitted on those exits. Do not route rollback failures through a
success/no-op reporter or change rollback to satisfy presentation code.

## Implementation flow

Refactor the existing continuation; keep the transaction owner and existing
Nix commands. The ordered flow is:

1. Parse, identify the project, capture file identities, and stage an explicitly
   requested URL rewrite. Preserve the early `--no-lock` continuation.
2. Require the old lock and resolve one candidate lock into the existing
   temporary workspace with the existing targeted `nix flake update nixfied`.
3. Report locked identities and emit the best-effort documentation diff.
4. Calculate `flake_changed` and `lock_changed` before mode dispatch.
5. For plan, perform the existing captured-file unchanged check, render the
   complete plan with evaluation not run, and exit 0. Do not reach apply pause
   hooks, backups, replacement, or rollback setup.
6. For checked apply only, execute the existing `nix eval` command with the
   candidate reference lock and `manifest.drvPath`. Failure renders the complete
   rejected result and exits 5 before backups or writes. Forced apply skips this
   command entirely.
7. Checked and forced apply converge before the existing pre-apply pause and
   unchanged check. Reuse candidate identities, backups, guarded replacements,
   interruption handling, rollback, cleanup, and final identity checks.
8. Render the terminal apply result once. Both changed and no-change outcomes
   report the evaluation policy actually used.

Do not add builds, runtime admission, service startup, secret resolution, or
existing-state inspection to the gate. Extending evaluation into a migration or
deployment workflow defeats the narrow responsibility selected here.

## Required removals and cleanup

Prefer deletion and ordinary composition over new helpers or parallel paths.
The implementation must account for these concrete subtraction opportunities:

| Current code or expectation | Cutover action |
| --- | --- |
| `verify_candidate()` owns command execution, partial reporting, and fatal exit | Remove this helper and its unconditional call. Put the single evaluation command in checked apply; let the result reporter own terminal status. |
| `report_checked_summary()` duplicates mode-specific per-file branches | Replace it with one result reporter that also handles checked rejection and forced apply. Preserve file-specific information while sharing the action rendering. |
| Separate `old_available` / `candidate_available` flags mirror source-path presence | Remove the flags. Clear a source path on failed materialization and use nonempty successful paths to decide whether comparison is available. |
| Locked continuation tests whether the required original lock existed | Remove the impossible absent-original-lock cases described below. Retain admission and concurrent disappearance checks. |
| Broad verification wording and an exact-candidate rerun implication | Replace those statements with evaluation-scoped status and truthful per-invocation resolution guidance. |
| Tests require incompatible plan exit 5 and preflight status | Replace those expectations with successful inspection, no evaluation, complete summary, and unchanged-file proofs. Keep incompatible checked-apply rejection. |
| Known-gap text proposes `--skip-preflight` and default incompatible-plan rejection | Remove the superseded proposal when the feature lands. Record only genuinely deferred reporting/notes work and link this RFC if useful. |

The lock-refresh path is entered only after existing-lock admission; `--no-lock`
has already returned. Use the captured `lock_before` identity's existing `absent`
sentinel for admission, rather than retaining `lock_before_exists` as another
owner of that fact. In the admitted locked path:

- Determine lock changes by comparing candidate bytes with the original lock;
  remove the `lock_before_exists == 0` arm.
- Back up every changed lock without an additional existence-policy branch.
- Rollback of an applied lock restores that original backup. Delete the
  impossible "old lock absent, remove new lock" rollback arm.
- Remove the obsolete missing-old-lock identity-report alternative in this
  continuation. Preserve missing-source documentation reporting separately.

These deletions depend on admission remaining ahead of candidate preparation.
They must not remove checks for files disappearing or changing during execution,
or replace guarded rollback with unconditional copy/delete operations.

Retain the URL rewrite helpers, native parser, `file_identity`,
`assert_unchanged`, `atomic_replace`, atomic exchange helper, rollback ownership
flags, and race/signal test hooks. Do not prune a safety check merely because a
happy-path fixture does not exercise it. Keep the existing URL-only continuation;
its unrelated report simplification is not a prerequisite for this cutover.

The embedded Python atomic replacement and C exchange helper predate the current
Nix/Rust implementation-language rule. Do not add new Python/C logic or weaken
their behavior to reduce LOC. Replacing and deleting them requires an equivalent
Rust-owned implementation and platform proof in a separately scoped cleanup;
do not couple this small mode correction to a full installer rewrite. Likewise,
do not import runtime state machinery into installer tooling to share private
transaction code.

Report production-code LOC added and removed separately from tests/docs, plus
which helpers, branches, and duplicate facts disappeared. Seek a net reduction
in `upgrade.nix`; do not set a fabricated numeric target or remove necessary
proof to satisfy a quota.

## Atomic cutover surfaces

| Owner | Required change |
| --- | --- |
| `nix/install/upgrade.nix` | Parse force, normalize mode, move evaluation out of plan, share reporting/application, perform justified deletions. |
| `nix/meta/commands.nix` | Author force's token/default/help and correct upgrade/plan descriptions. |
| `nix/fixtures/upgrade-help.txt` | Update the independent expected help fixture. |
| `nix/checks/upgrade-syntax.nix` | Extend native parser checks for force repetition and combinations without changing existing parser policy. |
| `nix/checks/syntax.nix` | Keep the generated-help comparison valid; change vectors only if needed. |
| `nix/gate-nix.nix` | Update plan expectations, add forced application proofs, and preserve existing identity/diff/transaction coverage. |
| `flake.nix` | Update upgrade publication effects text to distinguish inspection, checked apply, and force. |
| `docs/CONTRACT.md` | Replace UPGRADE-1's shared fatal plan/apply preflight requirement with the three-mode contract. |
| `docs/GUIDE.md` | Document actual flags, exits, force boundaries, post-apply repair, and old-pin recovery ordering. |
| `docs/DEVELOPMENT.md` | Correct upgrade proof descriptions and plan/apply assumptions. |
| `docs/known-gaps.md` | Remove superseded design candidates and retain only undelivered gaps. |

Edit authored sources, not generated outputs. Upgrade's syntax is projected to
shell at package construction. [generated-files.nix](nix/meta/generated-files.nix)
projects only install commands to the Rust CLI and runtime commands to the
runtime; adding force must not produce unrelated Rust command changes.

Do not change `capability.txt`, the ABI snapshot, Nix/Rust ABI constants,
manifest declarations, compiler options, `docs/OPTIONS.md`, or the adopter app
inventory. This is a Nix-only command cutover, not a manifest/runtime ABI change.

Keep the frozen archives, archive/NAR provenance, `expected.diff`, and
`scope.expected.diff` under `nix/fixtures/upgrade-golden/` byte-identical. The
source documentation diff semantics are unchanged. Update current status and
exit assertions rather than rewriting historical sources to hide incompatibility.

## Required behavioral proof

Reuse the existing historical adopter and transaction fixtures. Exercise useful
outcomes and independent bytes rather than testing private mode variables.

| Scenario | Required observation |
| --- | --- |
| Adopter exports old package/API names, with valid input wiring | Plan exits 0, emits the expected source diff and complete summary, and leaves all three project files unchanged. |
| Candidate declarations fail evaluation | Plan still succeeds; checked apply exits 5, retains the Nix diagnostic, and changes no project file. |
| Explicit force on that same fixture | Candidate pin/lock is applied, declaration bytes are unchanged, evaluation is reported skipped, and no runtime state is created. |
| Output evaluation intentionally throws a distinguishable marker | Plan and force do not evaluate it; checked apply fails with the marker. This independently proves the bypass instead of inferring it from status text. |
| Valid candidate | Checked apply passes evaluation and applies; plan remains nonmutating. |
| Candidate bytes already match | Plan and both apply policies report unchanged; checked apply still requires evaluation and force still reports skipped. |
| Plan with force, in either order and with repetition | Behaves identically to plan, with no evaluation or writes. |
| Missing old lock or unresolved/unsupported candidate | Plan and force still reject without mutation. Existing `--no-lock` behavior remains valid. |
| Documentation identical/unavailable, and README/docs additions/deletions | Preserve distinct exact stdout markers and source diff bytes in plan and apply. |
| Unrelated inputs and project declarations | Pin/lock updates retain unrelated effective input identities and existing follows wiring; project-owned declarations are never rewritten. |
| Concurrent edits and interrupted application | Extend the existing guarded transaction cases to force so it cannot bypass conflict refusal or rollback; preserve diagnostic and temporary-file cleanup assertions. |

The throwing-output fixture's inputs must resolve without demanding its outputs;
the marker belongs in output/manifest evaluation, not input parsing. Use Nix/Rust
fixtures and the existing packaged test orchestration. No fake output containing
the same status words is a substitute for the real command behavior.

Run the narrowest proofs first: build the packaged upgrade, build the native
parser check, then run the canonical dirty-tree CI once on the final candidate.
The parser check can be built directly with:

```sh
nix build .#upgrade --no-link
nix build --impure --no-link --expr '
  let
    flake = builtins.getFlake ("git+file://" + builtins.getEnv "PWD");
    system = builtins.currentSystem;
    pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
  in import ./nix/checks/upgrade-syntax.nix { inherit pkgs; }
'
nix run .#ci -- --dirty
```

Follow [DEVELOPMENT.md](docs/DEVELOPMENT.md) for pinned checks and downstream
source selection. Do not substitute raw Cargo for fixture-backed CI. Report
unrun platform, release, and integration coverage; Linux does not prove Darwin
exchange/rollback behavior. The RFC authoring task itself needs link review and
`git diff --check`, not these implementation gates.

## Delivery and boundaries

Implement the mode/report/contract/test changes as one coherent logical commit,
including direct cleanup whose proofs are already part of that cutover. If the
dead-state and source-availability reductions warrant a second commit, it must
follow the coherent behavior cutover and independently preserve all guarantees.
Use lower-case subjects. Do not leave an intermediate commit where production,
help, contract, and tests disagree about plan or force.

Keep saved candidate locks, JSON output, exact historical note selection,
compatibility registries, migrations, automatic forced retry, full-source
snapshot transactions, and stronger crash durability outside this implementation.
There is no `--skip-preflight` alias and no new plan/apply subcommand tree.

Before changing declarations that could make old-pin controls unevaluable,
users must perform any required old-session inspection, shutdown, or recovery
with the matching old runtime, or preserve a working old checkout. Force only
repins; it does not make old data or registries acceptable to the new runtime.
Document this order without adding automatic recovery to upgrade.

## Material uncertainties

- **Candidate drift between invocations.** Each invocation independently resolves
  upstream. A later apply may select a different candidate than an earlier plan.
  Report actual identities and rerun semantics; do not imply an approved-plan
  token. Immutable source selection narrows drift but is not full-lock reuse.
- **Meaning of evaluation success.** The existing check covers only the values
  forced by manifest derivation evaluation. It is not comprehensive integration,
  host, runtime, or data validation. Keep the label scoped and the follow-up
  build/admission guidance explicit.
- **Write protection scope.** Current guards cover the three captured root files,
  not every imported module. File exchanges are individually atomic; multi-file
  visibility, uncatchable termination, and power-loss durability remain limited.
  Preserve existing checks without claiming a stronger transaction.
- **Platform and language cleanup.** The old replacement machinery has different
  Linux/Darwin exchange primitives and uses Python/C. Their removal needs a
  separate Rust implementation and both-platform behavioral proof. This RFC
  does not authorize a new fallback or a weaker replacement algorithm.

The mode policy itself is settled. These limitations do not prevent the narrow
cutover; they constrain its claims and identify independently scoped later work.
