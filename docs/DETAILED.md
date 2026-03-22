# Detailed Architecture

## Model Contract

`nixfiedModel` is compiled from typed modules and is the canonical source for generated command and view surfaces.

Top-level structure:

- `schema`
- `identity`
- `runtime`
- `serviceCatalog`
- `tasks`
- `workflows`
- `features`
- `views`
- `state`

Cheap canonical model:

- exported through `model`
- used by `stateHash`, introspection apps, selector derivation, and deterministic graph views
- intentionally excludes heavy runtime-only service normalization

Execution-only compiled surfaces:

- `compiled.services` carries heavy normalized service runtime details
- service app/hook env materialization is derived from `compiled.services` plus a selected service set
- `runtimeHash` fingerprints the heavy runtime layer separately from `stateHash`

## Compilation Passes

Deterministic pass order:

1. `resolve-modules`
2. `normalize-runtime`
3. `compile-service-catalog`
4. `compile-services`
5. `compile-tasks`
6. `compile-workflows`
7. `compile-features`
8. `compile-views`
9. `finalize-model`

The compiled state hash is `sha256(toCanonicalNix(model))`.
`runtimeHash` is a separate deterministic hash of the heavy compiled service runtime and is used by executor/orchestrator run-id seeding.

## Graph Exclusion

Pure graph exclusion is configured through `nixfied.graph.excludedServices`.

- Excluded services are gated during project service projection, before service config branches are selected.
- This is the mechanism to use when a service branch must not be evaluated at all.
- Dependent tasks and workflow units are pruned during compilation, and the filtered graph flows through features and generated views.
- Runtime `SKIP_<SERVICE>` flags are separate and only affect execution of an already-compiled graph.
- Public flake task/workflow launchers expose explicit compile-time selectors, for example `nix run .#ci -- --exclude-services helios --mode full --summary`.
- Launcher selector parsing is generic across public task apps and dispatcher surfaces (`run-task`, `run-workflow`, `run-workflow-parallel`).
- Generated service apps (`svc::<service>::<op>`) participate in the same launcher model and materialize only their own service runtime.
- Truthy `SKIP_<SERVICE>` env vars are folded into the launcher-selected exclusion set as compatibility sugar, but they are not compiler inputs by themselves.
- The canonical executed proof for that launcher path is `nix run .#framework::test -- --shard launcher-pruning`, which uses a poisoned Helios source override to verify that `SKIP_HELIOS=1` prevents Helios evaluation before selected-app compilation.

## Canonicalization Rules

- Canonical rendering is Nix-native (`toCanonicalNix`), not JSON-first.
- Attrset keys are recursively sorted.
- List ordering is preserved.
- Function values are rejected in canonicalized subtrees.

## Runtime and Dispatch

Execution is model-backed through dispatcher apps and orchestrator controls:

- `nix run .#run-task -- <task-id> [-- ...]`
- `nix run .#run-workflow -- <workflow-id> [-- ...]`
- `nix run .#run-workflow-parallel -- <workflow-id> [-- ...]`
- `nix run .#runs [-- <run-id>]`
- `nix run .#stop-run -- <run-id>`
- `nix run .#stop-all-runs`

Selector-aware launcher contract:

- Public task apps and dispatcher surfaces accept leading launcher options before normal app args.
- Public service apps do the same.
- `--exclude-services <csv>` is the canonical compile-time selector.
- `--launcher-help` shows launcher-specific help without invoking the selected app.
- Launcher parsing stops at the first non-launcher argument or `--`, and the remaining args are forwarded unchanged to the selected app.
- Public task/workflow surfaces are two-stage: resolve the selected app first, then execute a scoped dispatcher/orchestrator/executor runtime.
- The selected service set is derived from task requirements, recursive task deps, workflow unit requirements, workflow `preRun`/`postRun` tasks, `workflowRef` targets, workflow family/mode resolution, and explicit selectors.

Service operation consumption:

- The framework exports service operation apps such as `svc::postgres::status` from the compiled service graph.
- Task runtimes receive matching `SVC_<SERVICE>_<OP>` env vars for selected services only.
- Task runtimes receive per-service `NIXFIED_SERVICE_*` env vars only for selected services.
- Tasks without selected services do not receive ambient `SVC_*` or ambient `NIXFIED_SERVICE_*`.
- `NIXFIED_SERVICE_ROOT` remains available as a runtime-global root, even when no per-service env is exported.
- Prefer those generated surfaces over importing `framework/runtime/services/<service>/...` directly in project code; direct imports can retain excluded-service closures before task pruning runs.

Executor behavior:

- deterministic env initialization
- hermetic `PATH` from declared `runtimeInputs`
- deterministic defaults (`locale`, `timezone`, `umask`, workdir policy)
- runtime variable support for `NIX_ENV` and `PROJECT_ENV`
- workflow lifecycle phases via `preRun.tasks` and `postRun.tasks`
- summary artifact contract at `CI_ARTIFACTS_DIR/summary.json` when enabled
- run ids and orchestrator seeds incorporate `runtimeHash`, not only the cheap model hash

Workspace-scoped defaults:

- `model.state.workspaceId` is derived from project root and used to isolate default runtime state per workspace.
- Default registry root is `/tmp/nixfied-runtime/<projectId>/<workspaceId>/registry`.
- Default artifacts root is `/tmp/ci-artifacts/<projectId>/<workspaceId>`.
- If `REGISTRY_ROOT` is explicitly overridden and artifacts still use the legacy default, orchestrator resolves artifacts under `$REGISTRY_ROOT/artifacts`.

Task-scoped cache isolation:

- Command-task pre-hooks now set a task-scoped `CARGO_TARGET_DIR` for reproducible Rust cache separation.
- Default cache key path is `${TMPDIR:-/tmp}/mfm-ci-target/<run-id>/<workflow-id>/<task-id>`.
- `NIXFIED_TASK_ID`, `NIXFIED_ORCHESTRATOR_WORKFLOW_ID`, and `NIXFIED_PARENT_WORKFLOW_ID` are passed through so task logs and artifacts can be correlated.
- `NIXFIED_TASK_CACHE_KEY` is exported for quick debugging.
- Cache components are sanitized before use and truncated with deterministic fallback hashes when needed.

## Ephemeral Runtime Contract

Ephemeral workflow execution is mediated by `nixfied/framework/runtime/ephemeral.nix` and configured through `model.runtime.ephemeral`.

Mode selection:

- `copyMode = "git-files"` (default): copies files from `git ls-files --cached --others --exclude-standard`.
- `copyMode = "static-excludes"`: copies with fixed `rsync --exclude` patterns from `excludePatterns`.

Copy root behavior:

- If the caller is inside a git worktree, source root resolves to `git rev-parse --show-toplevel`.
- Otherwise, source root falls back to caller path.

Failure retention policy:

- `keepFailures` controls whether failed ephemeral roots are retained.
- Retained failures are renamed to `${projectId}-ephemeral-failed-...`.
- `maxFailedRootAgeHours` prunes old retained roots.
- `maxFailedRoots` prunes oldest retained roots beyond count limit.

Ephemeral env and workdir behavior:

- `HOME`, `TMPDIR`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `REGISTRY_ROOT`, and `NIXFIED_SERVICE_ROOT` are rebound into the ephemeral root.
- `CI_ARTIFACTS_DIR` defaults into the ephemeral root, but explicit caller artifact overrides remain respected.
- When execution starts from a project subdirectory, workdir resolution preserves that relative path inside the copied source tree.

Disk budget guardrails:

- `maxCopyBytes` enforces a maximum estimated copy size.
- `minFreeBytesAfterCopy` enforces a minimum estimated free space floor after copy.
- Budgets are evaluated before copy using dry-run `rsync --stats` estimates and fail fast with `ERROR:` logs.

## Core App Surfaces

Exposed core apps:

- `build`
- `check`
- `check-ports`
- `ci`
- `dev`
- `format`
- `framework::install`
- `framework::test`
- `framework::upgrade`
- `health`
- `ports`
- `ready`
- `test`
- `test-isolation`
- `validate-env`

Introspection apps:

- `introspect`
- `stateHash`
- `schema`

## Workflow Shape

CI modes are represented as separate workflow IDs:

- `workflow.ci.basic`
- `workflow.ci.app`
- `workflow.ci.env`
- `workflow.ci.full`

Internal CI step tasks (for workflow composition) include:

- `task.ci.quality`
- `task.ci.tests`
- `task.ci.system-quick`
- `task.ci.nginx-proxy`

Sensitive host env vars (for example `API_KEY`, `*_TOKEN`, `*_SECRET`) are blocked from passthrough by default.
CI step tasks that need them must set `allowSensitivePassThrough = true` and list the required
variables in `passThroughEnv`.

Workflow lifecycle fields:

- `preRun.tasks`
- `postRun.tasks`
- `postRun.alwaysRun`

Service requirement fields:

- tasks declare hard service capabilities with `requirements.services`
- workflow units can add their own `requirements.services`
- the effective unit requirement set is the union of task and unit requirements

Only top-level user app surfaces are exposed in help output.

## Operations Utilities

Operations tasks are model-derived from runtime/env configuration:

- `health`
- `ready`
- `validate-env`
- `ports`
- `check-ports`
- `test-isolation`

Port calculation is based on:

`base_port + env_offset + (slot * stride)`

## Registry Contract

Event log path:

- `$REGISTRY_ROOT/events.ndjson`

Log rules:

- one compact JSON object per line
- newline-delimited and newline-terminated
- append-only writes
- stable key ordering
- monotonic contiguous `seq`
- RFC3339 UTC `ts`

Run IDs are allocated by the orchestrator and used as the registry correlation key for lifecycle events and control surfaces.

Write and lock guarantees:

- Registry event snapshots are taken under lock before replay/inspection.
- Registry sequence updates and event appends are atomic.
- Orchestrator run records are written by temp-file replace, never in-place mutation.
- Workflow `summary.json` is written atomically.
- Lock metadata records owner pid/host/purpose/timestamps for diagnostics and recovery checks.

## Framework Validation

`framework::test` provides shard execution with configurable parallelism:

- `flake-check`
- `help`
- `workflow-ci`
- `isolation`
- `self-host`

Determinism and contract checks live under `tests/framework/` and are exposed via flake checks.
The `flake-check` shard evaluates that flake check graph with `nix flake check . --no-build` so `framework::test` does not recursively rebuild workflow-heavy checks that are already covered by the other shards and by direct `nix flake check` usage.
