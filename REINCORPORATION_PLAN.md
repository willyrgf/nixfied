# Reincorporation Plan: Items 1, 2, and 3

## Summary

Incorporate the MFM changes back into framework core as three commits, one per item:

1. Registry concurrency + deterministic replay, plus hook/parallel model-schema plumbing.
2. Env sandbox hardening to preserve hermetic behavior (no host path fallback).
3. Executor lifecycle/parallel/hook engine hardening with a unified stackable lifecycle interface.

Lifecycle API is standardized to `lifecycle.<phase>` so users can append phase tasks directly, including `task.ops.ready` and `task.ops.health`.

## Commit 1: Registry + Model/Schema Plumbing

### Scope

- Keep/merge `nixfied/registry/events.nix`, `nixfied/registry/replay.nix`, `nixfied/modules/tasks.nix`, `nixfied/modules/workflows.nix`, `nixfied/compiler/compile-tasks.nix`, `nixfied/compiler/compile-views.nix`, `nixfied/schemas/task-contract.json`, `nixfied/schemas/workflow-contract.json`, and `nixfied/runner/dispatcher.nix`.
- Include runtime hook fields (`preHooks`, `postHooks`) in compiled tasks.
- Include workflow parallel config (`execution.parallel`) in module/schema.
- Keep `run-workflow-parallel` dispatcher surface and help output.
- Keep registry append locking and monotonic `.seq`.
- Keep replay sorted by `.seq`.

### Acceptance Criteria

- `run-workflow-parallel` appears in help snapshot.
- Task contract includes hook schema.
- Workflow contract includes execution fields.
- Replay output is deterministic for out-of-order event input.
- Registry append keeps valid NDJSON and increasing sequence values.

### Tests to Add or Adjust

- Extend `tests/framework/registry-events-contract.nix` with lock and sequence path assertions.
- Keep deterministic replay assertions in `tests/framework/registry-replay.nix`.
- Update `tests/framework/snapshots/help.txt` as needed for dispatcher surface.

### Commit Message

`incorporate registry ordering and hook/parallel model plumbing`

## Commit 2: Env Sandbox Hermetic Guard

### Scope

- Keep runtime-level sandbox helper support in `nixfied/runner/env-sandbox.nix` via `run_in_sandbox_runtime`.
- Preserve hermetic PATH behavior with Nix-only toolchain path.
- Explicitly avoid host fallback entries like `/usr/bin:/bin`.
- Keep `run_in_sandbox` delegating through task `.runtime`.

### Acceptance Criteria

- Hook runtime execution works through `run_in_sandbox_runtime`.
- PATH stays deterministic and hermetic.
- Existing task execution behavior remains stable.

### Tests to Add or Adjust

- Extend `tests/framework/env-sandbox-contract.nix` to assert:
- `run_in_sandbox_runtime()` exists.
- `base_path` excludes `/usr/bin:/bin`.
- Existing `env -i` PATH contract remains intact.

### Commit Message

`harden env sandbox runtime wrapper without host path fallback`

## Commit 3: Executor + Unified Stackable Lifecycle

### Scope

- Refactor workflow lifecycle public API to one consistent interface:
- `lifecycle.setup`
- `lifecycle.preRun`
- `lifecycle.postRun`
- `lifecycle.teardown`
- Each phase shape:
- `tasks = [ ... ]`
- `alwaysRun = <bool>`
- Hard cutover: remove legacy top-level `setup` and `teardown` interface from workflow module API.
- Keep execution order:
- `setup -> preRun -> main plan -> postRun -> teardown`
- Keep existing hook semantics:
- pre hooks run before main task command.
- post hooks run after main command attempt.
- non-shell runners with hooks fail explicitly.
- Keep parallel scheduler behavior:
- dependency graph scheduling.
- lock arbitration (exclusive; warn for `shared-aware`).
- fail-fast cancellation for running and pending units.
- worker caps via model `maxWorkers`, bounded by `NIXFIED_CI_MAX_WORKERS` and `CI_MAX_WORKERS`.
- `NIXFIED_WORKFLOW_PARALLEL` override support.

### Module and Schema Changes

- Update `nixfied/modules/workflows.nix`:
- Add `lifecycle` attrset with four typed phase submodules.
- Each phase exposes appendable `tasks` list and `alwaysRun` flag.
- Defaults:
- `setup.alwaysRun = false`
- `preRun.alwaysRun = false`
- `postRun.alwaysRun = true`
- `teardown.alwaysRun = true`
- Update `nixfied/schemas/workflow-contract.json`:
- Add `lifecycle` object with phase properties.
- Remove reliance on top-level `setup` and `teardown` in the public contract.
- Update `nixfied/compiler/compile-workflows.nix`:
- emit normalized `lifecycle` object in compiled model.
- stop emitting legacy top-level phase objects.

### Executor Refactor

- Update `nixfied/runner/executor.nix` phase handling to read from `.lifecycle.<phase>.tasks[]?`.
- Use `.lifecycle.<phase>.alwaysRun` uniformly for all phases.
- Keep generic phase runner and eliminate legacy-specific selectors.

### Project Migration

- Migrate workflow definitions in `nixfied/project/module.nix` to `lifecycle.<phase>`.
- Keep health and readiness checks opt-in and explicit by phase lists:
- `lifecycle.preRun.tasks = [ "task.ops.ready" ];`
- `lifecycle.postRun.tasks = [ "task.ops.health" ];`
- Preserve appendability so user modules can extend with `lib.mkAfter` or `lib.mkBefore`.

### Appendable User Extension Pattern

- Base workflow sets framework phase tasks.
- User modules append their own tasks, for example:
- `nixfied.workflows."<id>".lifecycle.preRun.tasks = lib.mkAfter [ "task.user.pre" ];`
- `nixfied.workflows."<id>".lifecycle.postRun.tasks = lib.mkAfter [ "task.user.post" ];`

### Acceptance Criteria

- Existing smoke tests pass:
- `tests/framework/parallel-runner-smoke.nix`
- `tests/framework/task-hooks-smoke.nix`
- New lifecycle tests pass:
- phase order validation (`setup -> preRun -> main -> postRun -> teardown`).
- `alwaysRun` behavior for `postRun` and `teardown` on failures.
- worker cap override behavior (`NIXFIED_CI_MAX_WORKERS=1`).
- invalid override warnings for worker count and workflow parallel env.
- cancellation reason coverage:
- `dependency-not-passed`
- `fail-fast`
- `fail-fast-running`
- Contract tests validate lifecycle selector usage in executor and compiled model.

### Tests to Add

- `tests/framework/workflow-lifecycle-smoke.nix`
- `tests/framework/parallel-worker-cap-smoke.nix`
- Optional `tests/framework/executor-env-overrides-contract.nix`
- Update `tests/framework/compiler-validation.nix` for `lifecycle` model shape.
- Update `tests/framework/executor-contract.nix` markers for lifecycle field paths.

### Commit Message

`unify workflow lifecycle phases and harden executor contracts`

## Public Interface Changes

- Dispatcher command:
- `run-workflow-parallel <workflow-id> [-- ...]`
- Task runtime schema:
- `runtime.preHooks.<id>`
- `runtime.postHooks.<id>`
- Workflow lifecycle schema:
- `lifecycle.setup`
- `lifecycle.preRun`
- `lifecycle.postRun`
- `lifecycle.teardown`
- Workflow execution schema:
- `execution.parallel`
- `execution.failFast`
- `execution.lockPolicy`
- `execution.emitRegistryEvents`
- Runtime env knobs:
- `NIXFIED_WORKFLOW_PARALLEL`
- `NIXFIED_CI_MAX_WORKERS`
- `CI_MAX_WORKERS`

## Rollout and Verification

- Apply commits in order `1 -> 2 -> 3`.
- After each commit run:
- `nix run .#framework::test -- --summary`
- `nix run .#help`
- Final verification:
- `nix flake check`
- `nix run .#ci -- --summary`
- `NIXFIED_CI_MAX_WORKERS=1 nix run .#ci -- --summary`

## Assumptions and Defaults

- Exactly one commit per item (three total).
- Hard cutover to lifecycle API is accepted.
- Health and readiness lifecycle tasks remain opt-in, not auto-injected.
- Formatting-only churn is excluded unless required by touched hunks.
