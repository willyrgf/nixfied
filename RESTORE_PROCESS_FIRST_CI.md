# Restore Deterministic, Process-First CI/Test Execution

## Summary

Reintroduce missing runtime contracts by making execution process-first across all surfaces (apps, services, tasks, workflows, CI, tests), isolated by default for CI/test paths, restoring machine-readable summaries, implementing real multi-slot isolation, enforcing strict argument validation, and replacing workflow `setup/teardown` with composable `preRun/postRun` task lists.

User decisions already locked:
- Isolation is mandatory by default for CI/test, but must be overridable by config.
- Workflow lifecycle semantics move from `setup/teardown` to `preRun/postRun`.
- `--bg` must be reimplemented for true background execution, not no-op.
- CI DAG surface stays canonical as `units + needs` (with `stages` as sugar).
- Argument strictness is strict-by-default.
- Orchestrator is the universal process backend and run control plane for app/service/task/workflow/test/CI execution.

MFM bug-fix subset to preserve in this restore:
- Registry append must remain lock-protected and sequence-monotonic.
- Registry replay must remain deterministic via `sort_by(.seq)`.
- Env sandbox PATH must remain hermetic (no `/usr/bin:/bin` fallback).
- Parallel worker override and cancellation behavior must stay contract-tested.

## Restore Order

1. Process-first orchestration foundation (all execution surfaces route through orchestrator; foreground/background is policy-driven).
2. Ephemeral-by-default policy for CI/test/test-isolation (config-overridable).
3. Restore `summary.json` artifact writing.
4. Replace `test-isolation` placeholder with real matrix orchestration.
5. Migrate workflow lifecycle from `setup/teardown` to `preRun/postRun`.
6. Enforce strict arg validation on typed surfaces.
7. Update tests, help snapshots, docs.

## Deferred Scope (Explicit)

- Runtime env pass-through expansion (`CI_ARTIFACTS_BASE`, `SERVICE_*`) is not part of this restore.
- Re-open only if orchestrator/service execution cannot satisfy required behavior without those env policy additions.

## Implementation Plan

### 1) Process-first orchestrator

- Add `nixfied/runner/orchestrator.nix` as the single entrypoint for run lifecycle.
- Route all execution through orchestrator (not only CI/test), including:
  - `run-task`
  - `run-workflow`
  - `run-workflow-parallel`
  - generated app surfaces from `model.views.apps`
- service execution surfaces (`start/stop/restart/status`-adjacent flows) as run entities
- Orchestrator responsibilities:
  - allocate run id
  - track process state (`queued/running/passed/failed/canceled`)
  - track pid/pgid and mode (`fg`/`bg`)
  - apply default mode policy per surface and honor explicit `--bg`/`--fg` overrides
  - expose run inventory for introspection (`list`, per-run status)
  - support controlled shutdown (`stop <run-id>`, `stop-all`) with deterministic process-group cleanup
  - allocate/prepare artifacts dir
  - allocate/prepare ephemeral root when enabled
  - export runtime context env to executor
- Keep executor focused on task/workflow semantics; orchestrator owns process lifecycle.
- Design invariant: if something executes, it has an orchestrator run record and can be observed/stopped centrally.

### 2) Ephemeral-by-default execution policy

- Add workflow/task execution policy options:
  - `nixfied.workflows.<id>.execution.ephemeral.enable` (bool)
  - optional project-level defaults where needed
- Set defaults to enabled for:
  - `workflow.ci.*`
  - `test` workflow surface
  - `test-isolation`
- Allow explicit config override to disable isolation for CI/test if user opts out.
- Hard fail on ephemeral setup failure (`ERROR:` + non-zero); never silently continue.

### 3) Restore `summary.json` contract

- When `workflow.artifacts.writeSummary = true`, write:
  - `"$CI_ARTIFACTS_DIR/summary.json"`
- Keep `--summary` as compact terminal line; it does not replace summary JSON.
- Summary JSON fields (stable contract):
  - `run_id`
  - `workflow_id`
  - `mode`
  - `exit_code`
  - `started_at`
  - `finished_at`
  - `duration_seconds`
  - `counts` (`passed`, `failed`, `canceled`)

### 4) Real multi-slot `test-isolation`

- Replace placeholder in `nixfied/modules/operations.nix` with real orchestration:
  - build slot/env matrix from model/runtime config
  - execute isolated CI per slot/env combination
  - capture per-combination logs/artifacts
  - aggregate and fail if any combination fails
- Remove current success-on-skip behavior.
- If isolation is disabled/misconfigured, fail hard with clear `ERROR:` output.

### 5) Workflow lifecycle migration (`setup/teardown` -> `preRun/postRun`)

- Schema changes in `nixfied/modules/workflows.nix`:
  - remove `setup`
  - remove `teardown`
  - add `preRun.tasks :: [taskId]`
  - add `postRun.tasks :: [taskId]`
  - add `postRun.alwaysRun :: bool` (default `true`)
- Compiler changes in `nixfied/compiler/compile-workflows.nix`:
  - carry new fields into compiled model
- Executor flow:
  - run `preRun.tasks` in order
  - run main workflow plan
  - run `postRun.tasks` in ordered list semantics
  - honor `postRun.alwaysRun`
- Keep task-level `preHooks/postHooks` as independent lower-level lifecycle.
- Keep phase lists appendable for user modules (`lib.mkBefore`/`lib.mkAfter`).
- Intentional divergence from legacy MFM draft:
  - do not keep `lifecycle.setup` / `lifecycle.teardown` phases in public API.
  - use only `preRun/postRun` at workflow lifecycle level.

### 6) Strict arg validation by default

- In `nixfied/project/module.nix` (`mkCommandTask`), set typed arg parser unknown handling to strict.
- Executor/workflow parsing rules:
  - accept known flags (`--mode`, mode aliases, `--summary`, `--bg`)
  - reject unknown flags before `--`
  - pass through args after `--` only
- Expected behavior:
  - `nix run .#ci -- --wat` must fail with usage error.

### 7) Tests, snapshots, docs

- Update/add framework checks to assert new contracts:
  - strict arg validation
  - `summary.json` writing
  - real `--bg` path
  - orchestrator run inventory and stop controls
  - real `test-isolation` orchestration
  - workflow pre/post semantics
- Update help snapshot if exposed usage changes.
- Concrete contract/smoke updates:
  - extend `tests/framework/registry-events-contract.nix`:
    - assert `.events-lock` path usage
    - assert `.seq` write path usage and monotonic-seq plumbing markers
  - extend `tests/framework/registry-replay.nix`:
    - assert replay determinism with out-of-order input via `sort_by(.seq)`
  - extend `tests/framework/env-sandbox-contract.nix`:
    - assert `run_in_sandbox_runtime()` marker
    - assert `base_path` excludes `/usr/bin:/bin`
    - keep `env -i PATH=...` contract assertion
  - extend `tests/framework/executor-contract.nix`:
    - assert lifecycle phase handlers for `preRun/postRun`
    - assert worker cap override handling + invalid override warnings
    - assert cancellation reason markers (`dependency-not-passed`, `fail-fast`, `fail-fast-running`)
  - add `tests/framework/orchestrator-lifecycle-contract.nix`
  - add `tests/framework/orchestrator-stop-controls-smoke.nix`
  - add `tests/framework/workflow-lifecycle-smoke.nix`
  - add `tests/framework/parallel-worker-cap-smoke.nix`
- Keep output prefix contract unchanged:
  - `INFO:`
  - `WARN:`
  - `ERROR:`
  - `OK:`
  - `SKIP:`

## Orchestrator Design Appendix (v1)

### A) Scope and Intent

- Orchestrator is the process lifecycle authority for every execution surface.
- Existing behavior owned by executor (task/workflow semantics, DAG scheduling, fail-fast, hook execution) remains in executor.
- Existing app surface contract (`nix run .#<app>`) remains stable; orchestration is an internal routing change, not a UX reset.

### B) Integrate Existing Runner Layers

- Current runner chain:
  - `nixfied/runner/default.nix` builds apps via `dispatcher.nix`.
  - `nixfied/runner/dispatcher.nix` routes to `nixfied-executor`.
  - `nixfied/runner/executor.nix` owns semantic execution.
- Target runner chain:
  - `default.nix` builds apps via `dispatcher.nix` (unchanged at interface boundary).
  - `dispatcher.nix` routes to `nixfied-orchestrator` instead of directly to `nixfied-executor`.
  - `orchestrator.nix` manages run lifecycle/process control and calls executor for semantic run execution.
  - `executor.nix` remains semantic engine for task/workflow execution logic.
- Integration invariant:
  - dispatcher and executor are both retained; orchestrator composes them rather than replacing either.
- Internal executor dual-runner preservation:
  - serial and parallel workflow runners in executor remain canonical for workflow semantics.
  - orchestrator chooses process mode (`fg`/`bg`) and delegates semantic mode (serial/parallel) to executor/model policy.

### C) Run State Model

- Run entity fields (minimum contract):
  - `run_id`, `surface`, `target_id`, `mode`, `pid`, `pgid`, `state`, `started_at`, `finished_at`, `exit_code`, `artifacts_dir`, `ephemeral_root`.
- State set:
  - `queued`, `running`, `passed`, `failed`, `canceled`.
- Terminal states:
  - `passed`, `failed`, `canceled`.
- Invariants:
  - every started run reaches one terminal state exactly once.
  - every executable invocation has one run record.
  - `pid/pgid` is recorded for all running processes.

### D) Lifecycle and Transitions

- `queued -> running` when child process is spawned and pid/pgid is known.
- `running -> passed` when child exits 0.
- `running -> failed` when child exits non-zero.
- `running -> canceled` when stop policy terminates process group.
- `queued -> canceled` is allowed for pre-start cancellation.
- Invalid transitions (for example `passed -> running`) are rejected as contract violations.

### E) Control Surfaces

- Required control capabilities:
  - run in foreground (`fg`) and background (`bg`)
  - list active and recent runs
  - inspect one run state/details
  - stop one run
  - stop all runs
- Proposed operator surfaces (names can be finalized during implementation):
  - `run-task` / `run-workflow` / `run-workflow-parallel` route through orchestrator
  - `runs` (list/status)
  - `stop-run <run-id>`
  - `stop-all-runs`
- Stop semantics:
  - send `TERM` to process group, wait grace window, escalate to `KILL` if still alive.
  - cancellation state and reason are persisted.

### F) Failure Semantics

- Spawn/setup failure before child start:
  - record failed run with reason and non-zero exit.
- Registry write failure:
  - fail hard (`ERROR:` + non-zero); never continue with untracked execution.
- Background launch failure:
  - do not report detached success; return failure synchronously.
- Orchestrator/executor boundary failures:
  - orchestrator is source of truth for process state.
  - executor non-zero exits map deterministically to run terminal failure.

### G) Compatibility and Rollout Notes

- Keep dispatcher usage and help contracts stable while rerouting internals.
- Keep executor CLI subcommands stable (`run-task`, `run-workflow`) during migration.
- Phase migration safety:
  - first integrate orchestrator as wrapper with minimal semantic changes.
  - then move lifecycle ownership details (run id/process tracking) to orchestrator where needed.
  - preserve deterministic registry ordering and existing cancellation reason contracts.

## Implementation Checklist (File-by-File)

1. `nixfied/runner/orchestrator.nix`
- Create binary (`nixfied-orchestrator`) with:
  - strict argument parsing for orchestration flags
  - fg/bg process spawn and pid/pgid tracking
  - run record create/update/finalize
  - list/status/stop/stop-all control handlers
  - executor invocation bridge for `run-task`/`run-workflow`

2. `nixfied/runner/dispatcher.nix`
- Replace direct executor calls with orchestrator calls for:
  - `run-task`
  - `run-workflow`
  - `run-workflow-parallel`
  - generated task app surfaces
- Preserve usage errors and stable command interface.

3. `nixfied/runner/default.nix`
- Wire orchestrator into runner assembly while keeping `mkApps` interface unchanged.

4. `nixfied/runner/executor.nix`
- Keep semantic responsibilities:
  - task execution
  - workflow serial/parallel scheduling
  - hooks and fail-fast logic
- Adapt boundary contract to accept orchestrator-provided run context inputs.
- Remove duplicated lifecycle ownership only when orchestrator parity is verified.

5. `nixfied/runner/env-sandbox.nix`
- Ensure orchestrator-injected runtime context remains compatible with sandbox execution.
- Preserve hermetic PATH and current env contract guarantees.

6. `tests/framework/default.nix`
- Register new orchestrator contract/smoke suites in the framework test index.

7. `tests/framework/orchestrator-lifecycle-contract.nix`
- Assert valid state transitions, terminal-state uniqueness, and run record completeness.

8. `tests/framework/orchestrator-stop-controls-smoke.nix`
- Assert `stop-run` and `stop-all` process-group behavior and cancellation recording.

9. `tests/framework/executor-contract.nix`
- Keep executor semantic guarantees intact under orchestrator routing.
- Assert serial/parallel behavior is unchanged except for process lifecycle ownership.

10. `tests/framework/snapshots/help.txt`
- Update if any new user-facing control surfaces are exposed.

## Concrete Issue/Task Breakdown (Execution Backlog)

Execution rule:
- Complete issues in strict dependency order.
- Each issue is done only when its "Done when" and "Validation" bullets both pass.
- Deferred scope reminder: `CI_ARTIFACTS_BASE` and `SERVICE_*` are excluded from this backlog.
- Land work as direct commits on `dev` (no PR packaging required for this restoration).
- Breaking changes are allowed on `dev`, but must be documented in this file and reflected in tests/snapshots.
- Strict commit sequence: `C1..C12` maps 1:1 to `RPF-01..RPF-12` and is landed in order (no parallel track splitting).

### RPF-00: Contract Baseline Lock (pre-work)

- Scope:
  - `tests/framework/registry-events-contract.nix`
  - `tests/framework/registry-replay.nix`
  - `tests/framework/env-sandbox-contract.nix`
  - `tests/framework/executor-contract.nix`
- Tasks:
  - Confirm baseline coverage for lock-protected append, monotonic `.seq`, replay ordering, hermetic PATH, cancellation markers.
  - Record any missing assertions as TODO items in this document before implementation starts.
- Done when:
  - Baseline contracts are explicitly listed and acknowledged as non-regression gates for the remaining issues.
- Validation:
  - `nix run .#framework::test`

### C1 / RPF-01: Add Orchestrator Entrypoint and Route Dispatcher Through It

- Depends on:
  - `RPF-00` (pre-commit gate)
- Scope:
  - `nixfied/runner/orchestrator.nix` (new)
  - `nixfied/runner/dispatcher.nix`
  - `nixfied/runner/default.nix`
- Tasks:
  - Create `nixfied-orchestrator` with pass-through support for `run-task` and `run-workflow`.
  - Reroute `run-task`, `run-workflow`, `run-workflow-parallel`, and generated app surfaces to orchestrator.
  - Preserve current usage/error surfaces and exit-code behavior for invalid invocation.
- Done when:
  - All execution entrypoints go dispatcher -> orchestrator -> executor.
  - Any user-facing app surface change is documented and corresponding snapshots/tests are updated.
- Validation:
  - `nix run .#help`
  - `nix run .#run-task -- <task-id>` (sample known task)
  - `nix run .#run-workflow -- <workflow-id>` (sample known workflow)

### C2 / RPF-02: Move Run Lifecycle Ownership to Orchestrator

- Depends on:
  - `C1`
- Scope:
  - `nixfied/runner/orchestrator.nix`
  - `nixfied/runner/executor.nix`
  - `nixfied/registry/events.nix`
- Tasks:
  - Make orchestrator allocate `run_id` and own lifecycle state transitions.
  - Ensure pid/pgid and run mode are tracked for running processes.
  - Refactor executor boundary so semantic execution runs under orchestrator-provided run context.
- Done when:
  - Every execution has exactly one orchestrator run record and terminal state.
  - Executor no longer acts as competing source of truth for run lifecycle.
- Validation:
  - `nix run .#ci -- --summary`
  - `nix run .#framework::test`

### C3 / RPF-03: Implement Foreground/Background Process Policy

- Depends on:
  - `C2`
- Scope:
  - `nixfied/runner/orchestrator.nix`
  - `nixfied/runner/dispatcher.nix`
- Tasks:
  - Implement mode policy (`fg` default with explicit `--bg`, optional `--fg` override).
  - Ensure background mode detaches correctly and still registers lifecycle events.
  - Ensure process-group ownership is deterministic for later stop operations.
- Done when:
  - `--bg` is functional (not no-op), with tracked pid/pgid and terminal state updates.
- Validation:
  - `nix run .#ci -- --bg`
  - `nix run .#ci -- --summary`

### C4 / RPF-04: Add Orchestrator Control Surfaces (list/status/stop)

- Depends on:
  - `C3`
- Scope:
  - `nixfied/runner/orchestrator.nix`
  - `nixfied/runner/dispatcher.nix`
  - `tests/framework/orchestrator-stop-controls-smoke.nix` (new)
- Tasks:
  - Expose run inventory command(s) for active/recent runs.
  - Expose `stop-run <run-id>` and `stop-all-runs`.
  - Implement TERM -> grace -> KILL process-group termination policy and cancellation recording.
- Done when:
  - Running fg/bg work can be inspected and forcibly stopped from orchestrator surfaces.
- Validation:
  - orchestrator list/status command
  - orchestrator stop command on live background run
  - `nix run .#framework::test`

### C5 / RPF-05: Enforce Ephemeral-by-Default Policy for CI/Test Paths

- Depends on:
  - `C4`
- Scope:
  - `nixfied/modules/workflows.nix`
  - `nixfied/compiler/compile-workflows.nix`
  - `nixfied/runner/orchestrator.nix`
  - `nixfied/project/module.nix`
- Tasks:
  - Add/propagate workflow execution ephemeral policy fields.
  - Default CI/test/test-isolation paths to ephemeral enabled.
  - Fail hard on ephemeral setup failures; do not continue in shared roots.
- Done when:
  - CI/test paths are isolated by default and explicitly overridable by config.
- Validation:
  - `nix run .#ci -- --summary`
  - `nix run .#test-isolation`

### C6 / RPF-06: Restore `summary.json` Artifact Contract

- Depends on:
  - `C5`
- Scope:
  - `nixfied/runner/orchestrator.nix`
  - `nixfied/runner/executor.nix`
- Tasks:
  - Write `"$CI_ARTIFACTS_DIR/summary.json"` when `workflow.artifacts.writeSummary = true`.
  - Keep `--summary` terminal output as compact line contract.
  - Ensure summary fields match contract in this document.
- Done when:
  - Summary file exists with stable schema after qualifying workflow runs.
- Validation:
  - `nix run .#ci -- --summary`
  - assert summary JSON file presence/content in framework test

### C7 / RPF-07: Replace `test-isolation` Placeholder with Real Matrix Runner

- Depends on:
  - `C6`
- Scope:
  - `nixfied/modules/operations.nix`
  - `nixfied/runner/orchestrator.nix`
  - `tests/framework/` (new/extended isolation smoke test)
- Tasks:
  - Build slot/env matrix from model/runtime config.
  - Execute isolated CI per matrix cell, aggregate artifacts/results, fail on any cell failure.
  - Remove SKIP-success placeholder behavior.
- Done when:
  - `test-isolation` runs real orchestration or hard-fails with `ERROR:`.
- Validation:
  - `nix run .#test-isolation`
  - `nix run .#framework::test`

### C8 / RPF-08: Workflow Lifecycle Migration (`setup/teardown` -> `preRun/postRun`)

- Depends on:
  - `C7`
- Scope:
  - `nixfied/modules/workflows.nix`
  - `nixfied/compiler/compile-workflows.nix`
  - `nixfied/schemas/workflow-contract.json`
  - `nixfied/runner/executor.nix`
  - `nixfied/project/module.nix` (workflow declarations/migrations)
- Tasks:
  - Replace workflow schema fields with `preRun.tasks`, `postRun.tasks`, `postRun.alwaysRun`.
  - Update compiler and executor sequencing semantics.
  - Keep task-level `preHooks/postHooks` independent and intact.
- Done when:
  - Workflow lifecycle execution is deterministic with new fields only.
- Validation:
  - workflow lifecycle smoke test
  - `nix run .#framework::test`
  - `nix flake check`

### C9 / RPF-09: Enforce Strict Argument Validation on Typed Surfaces

- Depends on:
  - `C8`
- Scope:
  - `nixfied/project/module.nix`
  - `nixfied/runner/executor.nix`
  - `nixfied/runner/orchestrator.nix`
- Tasks:
  - Set typed arg parser behavior to reject unknown flags before `--`.
  - Keep pass-through behavior only after `--`.
  - Ensure known flags include mode aliases, `--summary`, and orchestration mode flags.
- Done when:
  - Unknown typed flags fail with usage error and non-zero exit.
- Validation:
  - `nix run .#ci -- --wat` (must fail)
  - `nix run .#ci -- --summary`

### C10 / RPF-10: Expand Framework Contract and Smoke Tests

- Depends on:
  - `C9`
- Scope:
  - `tests/framework/default.nix`
  - `tests/framework/orchestrator-lifecycle-contract.nix` (new)
  - `tests/framework/orchestrator-stop-controls-smoke.nix` (new)
  - `tests/framework/workflow-lifecycle-smoke.nix` (new)
  - `tests/framework/parallel-worker-cap-smoke.nix` (new)
  - existing contract tests listed in this document
- Tasks:
  - Register and implement all new suites.
  - Extend existing contract tests for replay ordering, lock paths, env sandbox guarantees, and cancellation reasons.
  - Keep tests deterministic and grep-friendly.
- Done when:
  - Framework suite covers all restored contracts in this plan.
- Validation:
  - `nix run .#framework::test`

### C11 / RPF-11: Snapshots, Docs, and Public Surface Verification

- Depends on:
  - `C10`
- Scope:
  - `tests/framework/snapshots/help.txt`
  - `RESTORE_PROCESS_FIRST_CI.md`
  - any surfaced help/docs generators impacted by new commands
- Tasks:
  - Update help snapshot for any new exposed control surfaces.
  - Verify docs and command contracts remain ASCII and stable.
  - Confirm deferred-scope items are not accidentally implemented.
- Done when:
  - Help snapshot and documentation match the final exposed surface.
- Validation:
  - `nix run .#help`
  - `nix run .#framework::test`

### C12 / RPF-12: Final Integration Gate (Release Readiness)

- Depends on:
  - `C11`
- Scope:
  - Whole repo gate run for this restoration.
- Tasks:
  - Execute full validation matrix and record outcomes.
  - Confirm no regressions in preserved MFM bug-fix subset.
  - Confirm commit boundaries and final rollout notes remain accurate.
- Done when:
  - All required commands pass with expected semantics and no open blocker remains.
- Validation:
  - `nix run .#help`
  - `nix run .#ci -- --summary`
  - `nix run .#ci -- --wat` (must fail)
  - `nix run .#ci -- --bg`
  - orchestrator list/status surface
  - orchestrator stop/stop-all surface
  - `nix run .#test-isolation`
  - `nix run .#framework::test`
  - `nix flake check`

## Sequencing Guidance

- Land commits strictly as `C1` -> `C12`.
- Do not split work into parallel tracks for this restoration.

## Blocker Rules

- Any regression in lock-protected registry writes, replay determinism, hermetic PATH, or cancellation reason contracts blocks integration.
- Command-surface breaks (`nix run .#<app>`) are allowed on `dev`, but only with explicit doc updates and updated help/tests.
- Any non-deterministic framework test behavior blocks integration until stabilized.

## Public API and Contract Changes

- Workflow schema:
  - removed: `setup`, `teardown`
  - added: `preRun.tasks`, `postRun.tasks`, `postRun.alwaysRun`
- CLI/runtime:
  - `--bg` becomes functional background mode
  - unknown args error by default on typed surfaces
- Artifacts:
  - `summary.json` restored in `CI_ARTIFACTS_DIR` when enabled
- Execution policy:
  - CI/test/test-isolation isolated by default, overridable by config
- Dispatcher:
  - `run-workflow-parallel <workflow-id> [-- ...]` must remain exposed and routed through orchestration.
- Orchestrator control:
  - running processes are queryable and stoppable via orchestrator-backed control surfaces.

## Test and Validation Matrix

Run and verify:
- `nix run .#help`
- `nix run .#ci -- --summary`
- `nix run .#ci -- --wat` (must fail)
- `nix run .#ci -- --bg` (must detach and register process state)
- orchestrator status/list surface shows fg/bg runs and current state
- orchestrator stop/stop-all surface terminates tracked process groups and records cancellation state
- `nix run .#test-isolation` (must run real orchestration or fail hard)
- `nix run .#framework::test`
- `nix flake check`

## Acceptance Criteria

- `summary.json` is generated again at expected location.
- CI/test runs are isolated by default and avoid run collisions.
- `test-isolation` no longer returns placeholder success.
- Workflow `preRun/postRun` semantics are executed deterministically.
- Unknown args are rejected by default.
- `--bg` is true background execution with process tracking.
- All execution surfaces are represented in orchestrator state and can be inspected/stopped centrally.
- Replay and event ordering remain deterministic under concurrent writes.
- Parallel override warnings and cancellation-reason semantics are stable and tested.

## Assumptions and Defaults

- Isolation defaults to enabled for CI/test/test-isolation.
- Users can disable isolation explicitly through config.
- `postRun.alwaysRun` defaults to `true`.
- Canonical CI DAG API remains `units + needs`; `stages` remains sugar.
- Log prefix contract remains stable and ASCII-only.

## Commit Breakdown (Strict C1-C12 Sequence on `dev`)

1. `C1`: add orchestrator entrypoint and reroute dispatcher surfaces.
Suggested message: `route dispatcher surfaces through orchestrator entrypoint`
2. `C2`: move run lifecycle ownership to orchestrator and adjust executor boundary.
Suggested message: `move run lifecycle ownership from executor to orchestrator`
3. `C3`: implement fg/bg process policy with deterministic process-group tracking.
Suggested message: `implement orchestrator foreground and background process policy`
4. `C4`: add orchestrator control surfaces for list/status/stop operations.
Suggested message: `add orchestrator run inventory and stop controls`
5. `C5`: enforce ephemeral-by-default policy for CI/test/test-isolation.
Suggested message: `default ci and test execution paths to ephemeral isolation`
6. `C6`: restore `summary.json` artifact contract.
Suggested message: `restore workflow summary json artifact contract`
7. `C7`: replace `test-isolation` placeholder with real matrix orchestration.
Suggested message: `implement real multi-slot test isolation orchestration`
8. `C8`: migrate workflow lifecycle schema/executor to `preRun/postRun`.
Suggested message: `migrate workflow lifecycle from setup teardown to preRun postRun`
9. `C9`: enforce strict typed arg validation (`unknown-before---` fails).
Suggested message: `enforce strict typed argument validation on runtime surfaces`
10. `C10`: implement and register framework contract/smoke suite expansion.
Suggested message: `expand framework contracts for orchestrator lifecycle and controls`
11. `C11`: update snapshots/docs/public surface notes.
Suggested message: `update help snapshots and docs for process-first contracts`
12. `C12`: execute final integration gate and close restoration.
Suggested message: `run final integration gate for process-first restoration`
