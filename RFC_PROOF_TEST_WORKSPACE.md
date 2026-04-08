# RFC: Proof Test Workspace

Date: 2026-04-07

Status: Draft

Repository: `/Users/willyrgf/dev/random/nixfied`

## Summary

The framework should adopt one canonical proof workspace and drive it through a small set of scenario tests.

This is a stronger direction than the current pile of narrowly scoped smoke tests, but only if we are disciplined about what the proof workspace is supposed to prove.

The right target is not "one app that uses all features."

The right target is:

- one checked-in example workspace fixture
- using only public framework surfaces
- exercised by a few explicit proof scenarios
- backed by a much smaller set of non-workspace unit tests for compile metadata, local helper semantics, and kernel-native logic

The proof workspace should become the backbone of runtime and end-to-end coverage.

It should not become the entire test strategy.

If a test shells through public apps or manages runtime state, it does not qualify as one of those surviving non-workspace tests.

## Problem

The current suite is too fragmented.

Symptoms:

- too many tests build custom local fixtures or custom harness models
- too many tests assert implementation shape instead of framework guarantees
- PR coverage is heavy on internal compile/kernel checks and light on real user-facing runtime scenarios
- many tests are proving adjacent slices of the same behavior with slightly different fixtures
- the current shard taxonomy is ownership-shaped, not guarantee-shaped

The result is predictable:

- high maintenance cost
- weak confidence in cross-feature behavior
- poor signal density
- slow evolution because every runtime change touches many bespoke tests

## Thesis

The framework is a workspace/runtime product, not a library of isolated helper functions.

A large part of the framework's value lives in how the public surfaces work together:

- compiled model publication
- public apps
- runtime execution
- service lifecycle operations
- workflow phases
- summaries and run inventory
- registry and artifact placement
- ephemeral execution
- slot/env isolation
- stop behavior and process-group cleanup
- install and upgrade

That means a meaningful fraction of the suite should be organized around one coherent workspace that uses the framework like a user would.

This is the right place to prove:

- the framework can boot a real workspace
- the public commands compose
- the runtime honors the core isolation and lifecycle rules
- the framework survives a complete start -> run -> inspect -> stop path

## Hard Position

The "one app using all features" idea is directionally right but structurally wrong.

One app is too narrow because the framework surface is bigger than a single launcher.

The public surface includes:

- `nix run .#help`
- `nix run .#features`
- `nix run .#introspect`
- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#ready`
- `nix run .#health`
- `nix run .#test-isolation`
- `nix run .#run-task -- <task-id>`
- `nix run .#run-workflow -- <workflow-id>`
- `nix run .#run-workflow-parallel -- <workflow-id>`
- `nix run .#runs`
- `nix run .#stop-run -- <run-id>`
- `nix run .#stop-all-runs`
- `nix run .#framework::install`
- `nix run .#framework::upgrade`
- `nix run .#svc::<service>::<op>`

So the correct object under test is a proof workspace, not an app.

## Goals

- Replace a large share of current runtime/e2e test surface with a smaller number of higher-signal proofs.
- Exercise the framework through public interfaces, not private harness-only paths.
- Make cross-feature regressions obvious.
- Keep PR coverage fast and deterministic.
- Preserve explicit guarantees for isolation, reproducibility, idempotence, stable CLI behavior, and process cleanup without snapshotting exact help/error wording by default.

## Non-Goals

- Do not replace compile/eval proofs with end-to-end smoke.
- Do not force one scenario to prove every branch and every negative case.
- Do not make the proof workspace depend on heavyweight real services unless the service-specific behavior is itself the product guarantee.
- Do not require every PR to boot heavyweight real-package variants of every service. The proof workspace must still touch every built-in service through the generic lifecycle path.
- Do not keep standalone help or error wording contract tests unless a specific string is explicitly promoted to API.

## What The Proof Workspace Should Prove

The proof workspace should cover core runtime capability composition.

### 1. Public Surface Coherence

The workspace should prove that the generated public surfaces are usable together:

- `help`
- `features`
- `introspect`
- `validate-env`
- `ports`
- `check-ports`
- `run-task`
- `run-workflow`
- `runs`
- `stop-run`

This is a stronger proof than isolated command help assertions because it checks that the public surface is not only present but operational.

The proof here is command viability and coherent behavior, not exact help text or error text snapshots.

### 2. Runtime Isolation And Placement

The workspace should prove:

- slot/env-derived runtime roots
- stable registry placement
- stable artifact placement
- workspace-scoped state separation
- no leakage from host mutable dirs into managed runtime dirs

This should replace multiple current tests that probe these behaviors one slice at a time.

### 3. Workflow And Service Phase Semantics

The workspace should prove:

- workflow `preRun` and `postRun` phases
- service phase ordering
- service requirement scoping
- ready/health execution in the expected places
- summary generation for successful and interrupted runs

### 4. Ephemeral Behavior

The workspace should prove:

- ephemeral source materialization behavior
- include-untracked behavior when enabled
- env-file behavior when disabled and when explicitly enabled
- artifact and registry behavior inside ephemeral runs

### 5. Process And Stop Semantics

The workspace should prove:

- foreground run tracking
- `pid` and `pgid` visibility where appropriate
- interruption cleanup
- `stop-run` behavior
- child-process tree cleanup, not just wrapper-pid cleanup

This is important because process-group behavior is one of the framework's real value propositions and also one of the easiest places to regress silently.

### 6. Packaging Round-Trip

The workspace should prove:

- thin install produces a usable wrapper
- vendored install produces a usable wrapper
- upgrade preserves user-owned files and refreshes framework-owned files
- installed wrappers can run at least one canonical proof scenario

This is a framework product promise, not a side detail.

## What Still Lives Outside The Proof Workspace

Only unit-test-like checks should survive outside the proof workspace.

That means:

- pure compile/eval proofs
- pure normalization/serialization/helper proofs
- kernel-native Rust unit tests

Everything else should move into proof workspace scenarios.

This includes negative behavior.

The framework should not keep a second pile of integration-smoke tests just because the path is a failure path instead of a happy path.

### Compile/Eval Unit Proofs

Keep small deterministic proofs for:

- model determinism
- state hash determinism
- service surface catalog publication
- introspection bundle/schema publication
- feature inventory metadata publication
- contract rendering determinism

These are cheaper and clearer at compile/eval time.

### Runtime Helper And Normalization Unit Proofs

Keep small local tests for:

- shell contract runtime helpers
- service policy resolution
- service probe normalization
- vendored metadata rendering
- runtime-event helper policy/status derivation
- small compile-time normalization rules

These should prove one local invariant each.

They should not materialize a full workspace, orchestrate a long workflow, or depend on exact CLI wording.

### Kernel-Native Unit Tests

Keep Rust-native unit tests for:

- run-id semantics
- summary composition
- validation rules
- workflow state-machine rules
- registry snapshot/replay logic
- run-record parsing and transition logic

If the semantics already live in the kernel, the strongest cheap proof should live there too.

### Negative And Failure Paths Move Into Workspace Scenarios

The following kinds of behavior should be proven through the proof workspace, not through standalone Nix integration smokes:

- blocked runtime-owned env overrides
- blocked sensitive passthrough
- invalid mode or arg combinations on public commands
- interruption and cancellation behavior
- process-tree cleanup behavior
- install/upgrade misuse paths
- lifecycle ordering failures visible through public commands

The proof workspace should own public failure behavior because those failures are part of the framework product surface.

## Capability Inventory Gaps To Fix First

The current `features` output is useful but incomplete as a capability inventory.

All framework capabilities should come from metadata.

Tests consume that metadata.

Tests do not define the capability model.

This is product metadata first, not a second handwritten test taxonomy.

The current inventory exposes major runtime features, but it does not yet model several important guarantees as first-class capabilities.

Before the proof workspace becomes the backbone of the suite, add explicit capability entries for:

- machine-output behavior
- summary sidecars
- task hooks
- stop-run semantics
- process-group cleanup
- install semantics
- upgrade semantics
- workflow interruption semantics
- artifact placement semantics

Without those entries, the proof workspace would carry implicit coverage instead of explicit coverage.

### Coverage Mapping Contract (Required Before Deletions)

Capability metadata is necessary but not sufficient.

We also need an explicit capability -> scenario ownership map that CI can validate.

Recommended file:

- `proof-workspace/scenarios/coverage-map.nix`

Recommended shape:

```nix
{
  "runtime.workflow-interruption" = {
    layer = "proof-workspace";
    scenarios = [ "scenario-2-interruption-process-cleanup" ];
    replaces = [
      "tests/framework/orchestrator-signal-cleanup-smoke.nix"
      "tests/framework/orchestrator-stop-controls-smoke.nix"
    ];
  };
}
```

Required CI gates before deleting any overlapping tests:

- every `coverageRequired = true` capability is mapped to at least one proof scenario or a surviving unit/kernel test with explicit rationale
- every mapped proof scenario runs in at least one CI profile
- deletion PRs fail unless each deleted test has a mapped replacement capability and a currently green proving scenario

This keeps deletion behavior explicit and mechanically enforced instead of policy-only.

## Proposed Fixture Shape

The proof workspace should live in a dedicated top-level folder.

Recommended path:

- `proof-workspace/`

The proof workspace should be tracked as a seed fixture and materialized into a temp git repo during tests.

Recommended layout:

```text
proof-workspace/
  README.md
  seed/
    flake.nix
    README.md
    AGENTS.md
    docs/ARCHITECTURE.md
    docs/DETAILED.md
    docs/UPGRADE.md
    nixfied/project/
      conf.nix
      module.nix
      runtime.nix
      services.nix
      tasks.nix
      workflows.nix
    app/
    fixtures/
  scenarios/
  lib/
    materialize.nix
    assert.sh
```

This is intentionally not under `tests/`.

Reason:

- it is a repository-shaped artifact, not just a test helper
- it should be possible to recreate a fresh repository from it
- it should support install/bootstrap round trips
- it should carry its own `nixfied/project/` ownership clearly

Key rules:

- the seed stays small and readable
- tests copy it into `$TMPDIR`, initialize a real git repo, and run through public commands
- the fixture must be deterministic and self-contained
- the fixture must not depend on developer-local tools or secrets

### Bootstrap Modes

The proof workspace should support two bootstrap modes:

#### 1. Seed Copy Mode

- copy `proof-workspace/seed/` into a temp directory
- initialize a git repo and commit a baseline (`git init`, `git add .`, `git commit`)
- run proof scenarios directly

This is the fast path for most scenarios.

#### 2. Install Bootstrap Mode

- start from an empty temp directory
- run `framework::install` through a fully qualified app reference, for example `nix run github:willyrgf/nixfied#framework::install -- --vendor --target "$TMPDIR/proof-wrapper"`
- materialize the proof workspace project files into the installed wrapper
- run scenarios from the wrapper root (`cd "$TMPDIR/proof-wrapper"`)
- run the same proof scenarios

This is the path that proves repository recreation and wrapper viability.

The RFC should treat these command forms as canonical to avoid ambiguity across test environments.

## Service Strategy Inside The Proof Workspace

All built-in services are product surface at least at the generic lifecycle level.

In the current repository, that means at minimum:

- `helios`
- `minio`
- `nginx`
- `postgres`
- `reth`
- supervisor-backed lifecycle execution where it participates in service orchestration

That means the proof workspace must exercise all built-in services through:

- setup/startup
- ready checks
- health checks
- stop/teardown
- workflow phase integration where relevant

The proof workspace should use deterministic and lightweight implementations where possible.

Why:

- faster
- more reproducible
- clearer failure modes
- lower CI variance

But "lightweight" must not mean "bypass the framework lifecycle."

The proof workspace still needs to drive the actual framework-owned setup/start/check/stop paths for every shipped service.

Deep service-specific branches beyond the generic lifecycle path do not need to run on every PR.

Examples:

- PostgreSQL backup/restore
- Helios sync gating details
- supervisor/process-compose edge behavior

Those can be covered by dedicated proof-workspace scenario variants or by unit tests when the logic is local enough.

### Runtime Budget And Tiering

To keep this maintainable, scenario scope and runtime cost must be explicit.

Recommended guardrails:

- Scenario 1 + 2 + 6 total PR budget target: <= 15 minutes
- Scenario 3 + 4 run on path-triggered pre-merge unless changed files force PR execution
- Scenario 5 defaults to pre-merge/nightly unless install/upgrade code paths changed
- heavier service-specific variants run nightly/release only unless the relevant service files changed

If a scenario regularly exceeds its budget, split or retier it instead of silently growing PR latency.

## Proposed Proof Scenarios

The proof workspace should be driven by a small scenario suite, not one mega-test.

### Scenario 1: Public Surface And Happy Path

Purpose:

- prove the workspace is valid
- prove the public apps are wired correctly
- prove services and workflows compose

Representative flow:

- `nix run .#help`
- `nix run .#features`
- `nix run .#introspect`
- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#ready`
- `nix run .#health`
- `nix run .#run-task -- <task-id>`
- `nix run .#run-workflow -- <workflow-id> --summary`
- `nix run .#run-workflow-parallel -- <workflow-id>`
- `nix run .#runs`
- `nix run .#svc::<service>::<op>` for representative service ops across all built-in services

Assertions:

- expected commands exist and execute
- summary file shape is correct
- registry events are written
- all built-in services prove setup/start/ready/health/stop/teardown viability
- services run in the expected order
- artifacts land in the expected per-run paths
- `run-workflow-parallel` executes with the expected workflow semantics
- direct command-surface smokes for `help`, `features`, `introspect`, and machine-readable summary output stop being necessary once this is green

### Scenario 2: Interruption And Process Cleanup

Purpose:

- prove stop behavior
- prove process-group cleanup
- prove interrupted runs are recorded correctly

Representative flow:

- start a workflow that launches a long-running process with a child process
- capture run id
- inspect `runs`
- issue `stop-run`
- assert the run reaches canceled state
- assert child process tree is gone
- assert stop reason and event details are correct
- start multiple long-running runs and issue `stop-all-runs`
- assert all tracked runs reach canceled state and process trees are gone

Assertions:

- `pid` and `pgid` are recorded when expected
- cancellation reason is stable
- child processes do not leak
- wrapper process cleanup is not mistaken for full process-tree cleanup

### Scenario 3: Ephemeral Workspace Scenario

Purpose:

- prove ephemeral source behavior through a real workspace

Representative flow:

- create a committed tracked fixture baseline in the temp git repo
- create explicit untracked sentinel files under known paths
- create an explicit env file fixture used only for this scenario
- run a workflow with default ephemeral behavior
- run again with worktree/untracked behavior enabled
- run again with env-file loading explicitly enabled

Assertions:

- tracked files are copied as expected
- untracked files are excluded by default and included only when configured
- host env file behavior matches config
- runtime directories, registry, and artifacts remain isolated
- scenario assertions are against explicit sentinel files, not inferred from incidental repo state

### Scenario 4: Isolation Matrix Scenario

Purpose:

- prove slot/env separation through the public isolation tool

Representative flow:

- `nix run .#test-isolation`

Assertions:

- per-cell logs and summaries exist
- slot/env values are propagated correctly
- registry and artifacts are isolated per cell
- no cell tramples another

### Scenario 5: Wrapper Round-Trip Scenario

Purpose:

- prove install and upgrade with the same canonical workspace

Representative flow:

- install thin wrapper
- run the happy-path scenario inside the installed wrapper
- install vendored wrapper
- run the happy-path scenario inside the vendored wrapper
- mutate user-owned files
- run upgrade
- assert preservation and overwrite rules

Assertions:

- wrappers remain runnable
- project-owned files are preserved when promised
- framework-owned files are refreshed when promised

### Scenario 6: Failure Path And Guardrail Scenario

Purpose:

- replace the current pile of negative integration tests
- prove public failures through the same repository-shaped workspace

Representative flow:

- invoke blocked runtime-owned env and sensitive passthrough cases through public commands
- invoke invalid mode and invalid arg combinations on public commands
- trigger cancellation and guardrail paths in a real run
- trigger install/upgrade misuse paths through public commands where relevant

Assertions:

- failure happens at the public boundary, not only in a private helper
- failure leaves no leaked processes behind
- failure leaves no corrupted registry or artifact state behind
- the failure class is stable enough to diagnose without depending on exact wording
- failure assertions use stable fields and codes, not prose matching

Failure contract requirements for this scenario:

- text mode: non-zero exit + `ERROR:` prefixed diagnostic surface
- machine/json mode (where supported): stable structured fields such as failure `code`, `stage`, and relevant target identifiers
- usage/precondition/validation/runtime-interruption classes are asserted by class and code, not by exact sentence wording

## What The Proof Workspace Can Replace

It should be able to replace large parts of the current runtime/e2e sprawl, especially tests that are really fragments of one bigger guarantee:

- CLI routing smoke for public runtime flows
- workflow phase ordering smoke
- service phase integration smoke
- runtime dir/artifact/registry isolation smoke
- many ephemeral behavior smokes
- stop/interruption/process cleanup smoke
- ready/health orchestration matrix smoke
- wrapper runnability smoke
- public negative-path and guardrail smokes

This does not mean one scenario replaces every current test file one-for-one.

It means the proof workspace should become the primary evidence for those behaviors, and overlapping narrower tests should be deleted aggressively.

## CI Implications

Recommended model:

### Every PR

- compile/eval proofs
- kernel native tests
- proof workspace happy path
- proof workspace interruption/process cleanup
- proof workspace failure-path/guardrail scenario
- coverage-map validation gate (capability -> scenario ownership + deletion eligibility checks)

### Path-Triggered PR Or Pre-Merge

- proof workspace ephemeral scenario
- proof workspace isolation scenario
- wrapper round-trip scenario
- service-specific exceptional scenario variants that are too heavy for every PR
- over-budget scenario reruns promoted from PR tier when changed paths require stronger confidence

### Nightly Or Release

- broader slot/env matrix
- heavier real-service scenarios
- any surviving legacy regression probes

This is much leaner than running dozens of unrelated smokes on every PR while still skipping the most important user-facing runtime composition.

## Migration Plan

### Current Status And Remaining Checklist (Updated 2026-04-08)

Current green state:

- [x] `nix run .#test -- --mode feature-proof --summary`
- [x] `nix run .#test -- --mode ci --summary`
- [x] `nix run .#test -- --mode full --summary`
- [x] Capability metadata, `proof-workspace/scenarios/coverage-map.nix`, historical green evidence, and CI deletion gating are in place.
- [x] Scenarios 1 through 6 exist and are scheduled in the intended profiles.

Still to do before this RFC can be treated as fully implemented:

- [x] Make `proof-workspace/seed/` a self-contained checked-in canonical workspace instead of relying on bootstrap to copy live `nixfied/project`, `nixfied/modules`, and `nixfied/framework` trees from the source repository.
- [x] Make Scenario 1 actually prove the capabilities it currently claims:
  - invoke representative `svc::<service>::<op>` public surfaces
  - exercise built-in service lifecycle viability for `helios`, `minio`, `nginx`, `postgres`, and `reth`
  - prove non-placeholder `ready`/`health` behavior where service-operation coverage is claimed
  - add a real task-hook proof instead of relying on `task.test.isolation.unit`
- [x] Add real machine-output coverage before treating `machine-output-app-smoke.nix` as fully replaced:
  - exercise at least one machine-output app happy path
  - exercise at least one structured machine-output failure path with stable fields/codes
- [x] Extend Scenario 6 so the negative-path migration is real rather than partial:
  - blocked runtime-owned env overrides
  - blocked sensitive passthrough
  - install/upgrade misuse paths
  - machine/json assertions over stable `code`, `stage`, and target identifiers where supported
- [x] Extend Scenario 5 so thin and vendored wrappers rerun at least one canonical happy-path proof flow, not just `validate-env` plus a single `run-task`.
- [ ] Finish deleting the remaining legacy integration-shaped checks that are still scheduled outside proof-workspace in `full`:
  - `caller-pwd-remote-projectroot-smoke.nix`
  - `disabled-service-no-package-resolution-smoke.nix`
  - `logging-injection-smoke.nix`
  - `nginx-site-management-smoke.nix`
  - `nix-client-env-smoke.nix`
  - `postgres-backup-restore-smoke.nix`
  - `postgres-config-artifacts-smoke.nix`
  - `ready-helios-sync-gate-smoke.nix`
  - `selected-source-only-resolution-smoke.nix`
  - `service-probe-overrides-smoke.nix`
  - `slot-env-runtime-smoke.nix`
  - `supervisor-lifecycle-smoke.nix`
  - `vendored-metadata-packaged-source-smoke.nix`
- [ ] Rewrite the remaining broad or presentation-coupled tests into smaller local proofs:
  - `features-surface-contract.nix`
  - `package-output-contract.nix`
  - `shell-contract-runtime-smoke.nix`
  - `workflow-validation-errors.nix`
  - `run-id-semantic-inputs-contract.nix`

### Stage 1: Define Capability Ownership

- make the capability inventory explicit enough to plan coverage
- add missing capability entries for stop/summary/hooks/install/upgrade/machine-output
- mark which capabilities are expected to be proven by the proof workspace
- add `proof-workspace/scenarios/coverage-map.nix`
- add CI validation that blocks deletions without mapped, green replacement coverage

### Stage 2: Create The Seed Workspace

- add `proof-workspace/seed/`
- use only public framework surfaces
- keep it deterministic and readable
- make every built-in service visible through the generic lifecycle path

### Stage 3: Add The First Two Proof Scenarios

- happy path
- interruption/process cleanup

These two scenarios should be enough to start deleting a meaningful subset of current runtime/e2e smokes, but only for capabilities already mapped and green in CI.

This is the first real cutover point. Do not wait for every later scenario before deleting the obvious runtime duplication.

### Stage 4: Add The Failure-Path Scenario

- move negative integration coverage into the proof workspace
- stop keeping separate negative-path smokes for public runtime behavior

### Stage 5: Add Ephemeral And Isolation Scenarios

- move fragmented ephemeral assertions into one coherent proof workspace story
- move fragmented isolation assertions into the public `test-isolation` path

### Stage 6: Add Wrapper Round-Trip

- prove install and upgrade using the same seed workspace

### Stage 7: Delete Aggressively

Delete old tests if:

- the proof workspace already covers the capability more directly
- the old test is proving implementation shape rather than behavior
- the old test exists only because the suite did not previously have a coherent workspace proof
- the coverage map explicitly marks a replacement and CI proves that replacement path is currently green

## Decision Rules

Use a shape-based routing rule, not a vague "useful test" rule.

A test is allowed to survive outside the proof workspace only if all of the following are true:

1. It evaluates compiler output, helper logic, or kernel logic in isolation.
2. It proves one local invariant with one local reason for failure.
3. It does not require a materialized repository, `git init`, installed wrapper, or workspace marker.
4. It does not invoke a public app as the thing under test.
5. It does not depend on runtime roots, registry side effects, artifact placement, service lifecycle, process cleanup, install/upgrade, slot/env isolation, or ephemeral execution.
6. It does not primarily verify exact help or error wording.
7. It is materially cheaper and clearer than proving the same thing through the proof workspace.

If a test does any of the following, it belongs in the proof workspace instead:

- shells through `nix run .#...`
- proves command composition across multiple public surfaces
- proves ready/health/service setup/start/stop/teardown
- proves registry/artifact/runtime-root placement
- proves isolation, ephemeral behavior, interruption, or cleanup
- proves install/upgrade or wrapper runnability
- proves public failure behavior

It should be:

- deleted
- replaced by a proof-workspace scenario
- or rewritten into a smaller unit test

## Risks

### Risk 1: The Proof Workspace Becomes A New Monolith

Mitigation:

- keep the seed workspace small
- keep scenarios separated by purpose
- do not stuff every branch and every negative case into one scenario

### Risk 2: Coverage Becomes Implicit Again

Mitigation:

- explicitly map capabilities to proof scenarios
- keep the capability inventory as the source of truth

### Risk 3: Stub Services Hide Real Integration Problems

Mitigation:

- keep a small number of service-specific exceptional tests where real package/runtime behavior matters

### Risk 4: Install/Upgrade Drift From The Proof Workspace

Mitigation:

- use the same seed workspace for wrapper round-trip tests instead of separate custom fixtures

## Resolved Decisions

1. All built-in services are framework product surface at least for setup/startup, checks, and teardown. In the current repo that means `helios`, `minio`, `nginx`, `postgres`, `reth`, plus supervisor-backed lifecycle execution where relevant. The proof workspace must touch all of them at that level.
2. No standalone exact help/error string contract tests are justified by default. The contract is behavior and surface viability, not wording. If a string ever becomes contractual, that should be an explicit policy decision, not a testing accident.
3. The proof workspace should live in a dedicated top-level folder, not under `tests/`.
4. All framework capabilities should come from metadata. Tests consume that metadata; they do not define it.
5. The current-suite keep/delete plan is explicit below.
6. Coverage-driven deletion is gated by a checked-in capability-to-scenario map and CI validation; "scenario exists" is not enough.

## Current Suite Triage

### Keep As Unit-Style Anchors

These are the current tests that fit the intended surviving shape, either as-is or with only minor shrinkage:

```text
tests/framework/contract-render-snapshot.nix
tests/framework/cross-machine-hash.nix
tests/framework/docs-guidance-contract.nix
tests/framework/helios-pinned-source-contract.nix
tests/framework/introspection-bundle-determinism.nix
tests/framework/kernel-native-tests.nix
tests/framework/model-hash.nix
tests/framework/nix-checks-deadnix-issues-fail-smoke.nix
tests/framework/nix-checks-nil-issues-fail-smoke.nix
tests/framework/nix-checks-parent-workflow-skip-smoke.nix
tests/framework/nix-checks-statix-issues-fail-smoke.nix
tests/framework/nix-checks-visible-output-smoke.nix
tests/framework/no-legacy-project-modules.nix
tests/framework/registry-replay.nix
tests/framework/run-record-validator-failure.nix
tests/framework/runtime-events-policy-smoke.nix
tests/framework/runtime-events-status-smoke.nix
tests/framework/scheduler-order.nix
tests/framework/service-policy-runtime-smoke.nix
tests/framework/service-probe-overrides-contract.nix
tests/framework/service-surface-catalog-contract.nix
tests/framework/vendored-metadata-contract.nix
```

Also keep the pure eval proof for the introspection schema shape.

Also keep a pure eval proof that capability metadata is complete and structurally valid. That is the replacement concept for the current `features` surface snapshot style of testing.

Some of these files still carry legacy `-smoke` names, but their shape is local/helper-level and that is what matters.

### Rewrite Into Smaller Unit Tests, Then Delete The Current File Form

These files are proving real things, but the current form is too broad or too presentation-coupled:

```text
tests/framework/compiler-validation.nix
tests/framework/excluded-service-evaluation.nix
tests/framework/features-surface-contract.nix
tests/framework/package-output-contract.nix
tests/framework/postgres-config-artifacts-contract.nix
tests/framework/operations-contract.nix
tests/framework/run-id-semantic-inputs-contract.nix
tests/framework/service-requirements-contract.nix
tests/framework/shell-contract-runtime-smoke.nix
tests/framework/workflow-validation-errors.nix
```

The goal is to split these into small local invariants, not keep the current monoliths.

In particular:

- `features-surface-contract.nix` should become a pure metadata proof over `model.features`, not a public CLI snapshot.
- `package-output-contract.nix` should become pure publication checks over apps/packages, not `--help` grep.
- `shell-contract-runtime-smoke.nix` should keep env/arg/exit semantics but stop asserting exact rendered error lines.
- `workflow-validation-errors.nix` should keep invalid-model evaluation failures and stop grepping source text for message literals.
- `operations-contract.nix` should keep compile-level operation wiring checks and drop presentation-coupled script text assertions.
- `run-id-semantic-inputs-contract.nix` should be split into smaller kernel/runtime invariants so run-id semantics are proven without broad integration harnessing.

### Delete Once The Proof Workspace Scenarios Are Green

These are the current tests that should be removed from the suite once the proof workspace covers the corresponding behavior:

```text
tests/framework/artifacts-root-override-isolation-smoke.nix
tests/framework/artifacts-run-isolation-smoke.nix
tests/framework/caller-pwd-remote-projectroot-smoke.nix
tests/framework/ci-mode-matrix-smoke.nix
tests/framework/disabled-service-no-package-resolution-smoke.nix
tests/framework/discovery-command-surfaces-smoke.nix
tests/framework/env-loader-strict-smoke.nix
tests/framework/ephemeral-copy-budget-smoke.nix
tests/framework/ephemeral-execution-smoke.nix
tests/framework/ephemeral-retention-smoke.nix
tests/framework/ephemeral-runtime-behavior-smoke.nix
tests/framework/ephemeral-runtime-env-isolation-smoke.nix
tests/framework/framework-install-filter-smoke.nix
tests/framework/framework-install-thin-smoke.nix
tests/framework/framework-install-vendor-smoke.nix
tests/framework/framework-selfhost-contract.nix
tests/framework/framework-template-install-upgrade-help-smoke.nix
tests/framework/framework-upgrade-preserve-smoke.nix
tests/framework/introspect-contract.nix
tests/framework/isolation-nested-run-id-smoke.nix
tests/framework/local-override-introspect-contract.nix
tests/framework/logging-injection-smoke.nix
tests/framework/nginx-site-management-smoke.nix
tests/framework/nix-ci-workflow-contract.nix
tests/framework/nix-client-env-smoke.nix
tests/framework/orchestrator-arg-forwarding-smoke.nix
tests/framework/orchestrator-signal-cleanup-smoke.nix
tests/framework/orchestrator-stop-controls-smoke.nix
tests/framework/parallel-runner-process-tree-smoke.nix
tests/framework/parallel-runner-smoke.nix
tests/framework/parallel-worker-cap-invalid-smoke.nix
tests/framework/parallel-worker-cap-smoke.nix
tests/framework/postgres-backup-restore-smoke.nix
tests/framework/postgres-config-artifacts-smoke.nix
tests/framework/postgres-kernel-probe-lifecycle-smoke.nix
tests/framework/project-config-boundary.nix
tests/framework/ready-health-matrix-smoke.nix
tests/framework/ready-health-shutdown-smoke.nix
tests/framework/ready-helios-sync-gate-smoke.nix
tests/framework/registry-detail-derivation-smoke.nix
tests/framework/registry-events-runtime-contract.nix
tests/framework/registry-lock-recovery-smoke.nix
tests/framework/run-id-active-collision-suffix-smoke.nix
tests/framework/run-id-noise-stability-smoke.nix
tests/framework/run-record-atomicity-smoke.nix
tests/framework/runtime-env-isolation-smoke.nix
tests/framework/selected-source-only-resolution-smoke.nix
tests/framework/service-dir-isolation-smoke.nix
tests/framework/service-lifecycle-matrix-smoke.nix
tests/framework/service-op-composition-contract.nix
tests/framework/service-probe-overrides-smoke.nix
tests/framework/service-set-behavior-contract.nix
tests/framework/slot-env-runtime-smoke.nix
tests/framework/summary-json-smoke.nix
tests/framework/supervisor-lifecycle-smoke.nix
tests/framework/task-hooks-smoke.nix
tests/framework/test-mode-cli-contract-smoke.nix
tests/framework/vendored-metadata-packaged-source-smoke.nix
tests/framework/workflow-lifecycle-smoke.nix
tests/framework/workflow-mode-derived-smoke.nix
tests/framework/workflow-probe-scope-smoke.nix
tests/framework/workspace-registry-isolation-smoke.nix
```

This delete list is intentionally large. Most of the current suite is integration-shaped and should not survive once the proof workspace exists.

### First Deletion Block Once Scenarios 1 And 2 Are Green

These are the earliest high-confidence deletion targets after the happy-path and interruption scenarios exist:

```text
tests/framework/ci-mode-matrix-smoke.nix
tests/framework/discovery-command-surfaces-smoke.nix
tests/framework/introspect-contract.nix
tests/framework/local-override-introspect-contract.nix
tests/framework/orchestrator-arg-forwarding-smoke.nix
tests/framework/orchestrator-signal-cleanup-smoke.nix
tests/framework/orchestrator-stop-controls-smoke.nix
tests/framework/parallel-runner-process-tree-smoke.nix
tests/framework/parallel-runner-smoke.nix
tests/framework/parallel-worker-cap-invalid-smoke.nix
tests/framework/parallel-worker-cap-smoke.nix
tests/framework/ready-health-matrix-smoke.nix
tests/framework/ready-health-shutdown-smoke.nix
tests/framework/service-lifecycle-matrix-smoke.nix
tests/framework/service-op-composition-contract.nix
tests/framework/service-set-behavior-contract.nix
tests/framework/summary-json-smoke.nix
tests/framework/task-hooks-smoke.nix
tests/framework/workflow-lifecycle-smoke.nix
tests/framework/workflow-mode-derived-smoke.nix
tests/framework/workflow-probe-scope-smoke.nix
```

These are still subject to the coverage-map gate. A file being in this block is not permission to delete it without mapped green replacement coverage.

### Defer From Early Cutover

Do not include the following in the first deletion block:

- `operations-contract.nix` until the narrower compile-level replacement exists
- `run-id-semantic-inputs-contract.nix` until run-id semantic invariants are migrated to smaller kernel/runtime proofs
- docs and `nix-checks` behavior contract tests that remain local/package-level invariants

### Later Deletion Blocks

- After Scenario 3 and 4: delete the fragmented ephemeral, env-loader, slot/env, registry/artifact isolation, and `test-isolation`-adjacent smokes.
- After Scenario 5: delete the install, vendor, thin-wrapper, self-host, and upgrade preservation smokes.
- After Scenario 6: delete the blocked-env, sensitive passthrough, invalid-cap, and other public negative-path smokes.

### Support Files To Delete Or Move With The Integration Suite

These are not standalone tests, but they exist only to support the integration-heavy suite that this RFC is replacing:

```text
tests/framework/launcher-disabled-nginx-override.nix
tests/framework/launcher-helios-task-module.nix
tests/framework/launcher-skip-helios-override.nix
tests/framework/poison-helios-source-override.nix
tests/framework/poison-package.nix
tests/framework/service-set-enabled-module.nix
tests/framework/lib/ci-probe-model.nix
tests/framework/lib/harness.nix
tests/framework/lib/runtime-fixture.nix
tests/framework/lib/test-probe-overrides.nix
```

`tests/framework/lib/shell-helpers.nix` should either be trimmed down for surviving unit tests or replaced by `proof-workspace/lib/assert.sh`.

`tests/framework/default.nix`, `tests/framework/framework-test-catalog.nix`, and `nixfied/framework/testing/catalog.nix` will need to be rebuilt around the new split:

- proof-workspace scenarios
- surviving unit tests
- profile selection based on capability metadata

## Current Recommendation

Adopt the proof workspace model.

Do not adopt the "single app proves everything" model.

Use one canonical workspace fixture plus a small scenario suite as the primary runtime/e2e proof system for the framework.

Keep only small compile/helper/kernel unit tests outside that system.

Do not preserve a separate class of standalone negative integration smokes.
