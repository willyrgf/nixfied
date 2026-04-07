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
- backed by a smaller set of non-workspace tests for compile guarantees, negative safety rules, and kernel-native semantics

The proof workspace should become the backbone of runtime and end-to-end coverage.

It should not become the entire test strategy.

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
- Preserve explicit guarantees for isolation, reproducibility, idempotence, stable CLI behavior, and process cleanup.

## Non-Goals

- Do not replace compile/eval proofs with end-to-end smoke.
- Do not force one scenario to prove every branch and every negative case.
- Do not make the proof workspace depend on heavyweight real services unless the service-specific behavior is itself the product guarantee.
- Do not treat every built-in service as equally important on every PR.

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

## What The Proof Workspace Should Not Be Asked To Prove

Some classes of tests should remain outside the proof workspace.

### Compile/Eval Proofs

Keep direct compile proofs for:

- model determinism
- state hash determinism
- service surface catalog publication
- introspection bundle/schema publication
- feature inventory publication
- contract rendering determinism

These are cheaper and clearer at compile/eval time.

### Negative Safety Rules

Keep direct targeted tests for:

- blocked runtime-owned env overrides
- blocked sensitive passthrough
- invalid workflow definitions
- invalid shell contract args/env
- invalid run-record payloads

A proof workspace is bad at negative combinatorics.

### Kernel-Native Semantics

Keep Rust-native tests for:

- run-id semantics
- summary composition
- validation rules
- workflow state-machine rules
- registry snapshot/replay logic
- run-record parsing and transition logic

If the semantics already live in the kernel, the strongest cheap proof should live there too.

### Service-Specific Exceptional Behavior

Do not force one proof workspace to exercise every built-in service in depth.

Service-specific tests should survive only where the service has framework-specific exceptional behavior, for example:

- PostgreSQL backup/restore or test-db lifecycle
- Helios readiness/sync gating or source-kind rules
- supervisor/process-compose behavior if it remains a public promise

Everything else should be covered by representative service orchestration inside the proof workspace.

## Capability Inventory Gaps To Fix First

The current `features` output is useful but incomplete as a test-planning tool.

It exposes major runtime features, but it does not yet model several important guarantees as first-class capabilities.

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

## Proposed Fixture Shape

The proof workspace should be tracked as a seed fixture and materialized into a temp git repo during tests.

Recommended layout:

```text
tests/proof-workspace/
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
  lib/
    materialize.nix
    assert.sh
```

Key rules:

- the seed stays small and readable
- tests copy it into `$TMPDIR`, initialize a real git repo, and run through public commands
- the fixture must be deterministic and self-contained
- the fixture must not depend on developer-local tools or secrets

## Service Strategy Inside The Proof Workspace

The proof workspace should use deterministic stub-backed services by default.

Why:

- faster
- more reproducible
- clearer failure modes
- lower CI variance

The workspace should still exercise real framework service orchestration:

- setup
- status
- ready
- health
- stop
- workflow pre/post phase integration

But it should not boot heavyweight real packages just to prove generic orchestration.

Recommended representative service mix:

- one plain HTTP/port-bound service
- one multi-step readiness service
- one long-running process that spawns a child process for stop/process-tree validation

Real service packages should be reserved for the few capabilities that truly depend on them.

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
- `nix run .#runs`

Assertions:

- expected commands exist and execute
- summary file shape is correct
- registry events are written
- services run in the expected order
- artifacts land in the expected per-run paths

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

Assertions:

- `pid` and `pgid` are recorded when expected
- cancellation reason is stable
- child processes do not leak
- wrapper process cleanup is not mistaken for full process-tree cleanup

### Scenario 3: Ephemeral Workspace Scenario

Purpose:

- prove ephemeral source behavior through a real workspace

Representative flow:

- run a workflow with default ephemeral behavior
- run again with worktree/untracked behavior enabled
- run again with env-file loading explicitly enabled

Assertions:

- tracked files are copied as expected
- untracked files are excluded by default and included only when configured
- host env file behavior matches config
- runtime directories, registry, and artifacts remain isolated

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

This does not mean one scenario replaces every current test file one-for-one.

It means the proof workspace should become the primary evidence for those behaviors, and overlapping narrower tests should be deleted aggressively.

## What Should Still Stay Outside

The following should survive as separate tests if they remain real guarantees:

- compile/eval publication proofs
- negative env/safety tests
- kernel-native tests
- service-specific exceptional behavior tests
- a few direct CLI contract checks where exact help/error wording is the product contract

## CI Implications

Recommended model:

### Every PR

- compile/eval proofs
- kernel native tests
- proof workspace happy path
- proof workspace interruption/process cleanup

### Path-Triggered PR Or Pre-Merge

- proof workspace ephemeral scenario
- proof workspace isolation scenario
- wrapper round-trip scenario
- service-specific exceptional scenarios

### Nightly Or Release

- broader slot/env matrix
- heavier real-service scenarios
- any surviving legacy regression probes

This is much leaner than running dozens of unrelated smokes on every PR while still skipping the most important user-facing runtime composition.

## Migration Plan

### Stage 1: Define Capability Ownership

- make the capability inventory explicit enough to plan coverage
- add missing capability entries for stop/summary/hooks/install/upgrade/machine-output
- mark which capabilities are expected to be proven by the proof workspace

### Stage 2: Create The Seed Workspace

- add `tests/proof-workspace/seed/`
- use only public framework surfaces
- keep it deterministic and readable
- add stub-backed services for representative orchestration patterns

### Stage 3: Add The First Two Proof Scenarios

- happy path
- interruption/process cleanup

These two scenarios should be enough to start deleting a meaningful subset of current runtime/e2e smokes.

### Stage 4: Add Ephemeral And Isolation Scenarios

- move fragmented ephemeral assertions into one coherent proof workspace story
- move fragmented isolation assertions into the public `test-isolation` path

### Stage 5: Add Wrapper Round-Trip

- prove install and upgrade using the same seed workspace

### Stage 6: Delete Aggressively

Delete old tests if:

- the proof workspace already covers the capability more directly
- the old test is proving implementation shape rather than behavior
- the old test exists only because the suite did not previously have a coherent workspace proof

## Decision Rules

When deciding whether to keep an old test after the proof workspace lands, ask:

1. Is this proving a real user-visible or framework-stability guarantee?
2. Is the proof workspace already proving that guarantee through a stronger boundary?
3. Could this be proven cheaper at compile/eval or kernel-native level?
4. Is this mostly an implementation-shape assertion?

If the answers are:

- no
- yes
- yes
- yes

then delete it.

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

## Open Questions

1. Which built-in services are true product surface versus examples or convenience integrations?
<!-- We want to use all services, at least the setup/startup process, checks and teardown process. -->
2. Which exact help/error strings are contractual enough to justify direct contract tests?
<!-- Cant imagine one -->
3. Should the proof workspace live under `tests/proof-workspace/` or another dedicated top-level folder?
<!-- I think a dedicated top-level folder is required, it also allows us to recreate a new repository from scratch using the top-level folder + the install process of the framework, the own project module.nix etc... -->
4. How much of the current `features` inventory should become test-planning metadata?
<!-- All features should come from metadata. Not really an test thing. -->
5. Which current tests should be the first deletion targets once the first two proof scenarios are green?
<!-- I want your help to describe exactly all tests that should be deleted (most of them) and the little unit test alike ones that should be maintained. -->

## Current Recommendation

Adopt the proof workspace model.

Do not adopt the "single app proves everything" model.

Use one canonical workspace fixture plus a small scenario suite as the primary runtime/e2e proof system for the framework.

Keep compile proofs, negative safety tests, and kernel-native tests outside that system.
