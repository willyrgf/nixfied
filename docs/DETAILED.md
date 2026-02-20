# Detailed Architecture

## Model Contract

`nixfiedModel` is compiled from typed modules and is the canonical source for generated command and view surfaces.

Top-level structure:

- `schema`
- `identity`
- `runtime`
- `services`
- `tasks`
- `workflows`
- `views`
- `state`

## Compilation Passes

Deterministic pass order:

1. `resolve-modules`
2. `normalize-runtime`
3. `compile-services`
4. `compile-tasks`
5. `compile-workflows`
6. `compile-views`
7. `finalize-model`

The compiled state hash is `sha256(toCanonicalNix(model))`.

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

Executor behavior:

- deterministic env initialization
- hermetic `PATH` from declared `runtimeInputs`
- deterministic defaults (`locale`, `timezone`, `umask`, workdir policy)
- runtime variable support for `NIX_ENV` and `PROJECT_ENV`
- workflow lifecycle phases via `preRun.tasks` and `postRun.tasks`
- summary artifact contract at `CI_ARTIFACTS_DIR/summary.json` when enabled

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
- `health`
- `ports`
- `ready`
- `test`
- `test-isolation`
- `validate-env`

Introspection apps:

- `model`
- `stateHash`
- `tasks`
- `task::<id>`
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

Workflow lifecycle fields:

- `preRun.tasks`
- `postRun.tasks`
- `postRun.alwaysRun`

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

## Framework Validation

`framework::test` provides shard execution with configurable parallelism:

- `flake-check`
- `help`
- `workflow-test`
- `workflow-ci`
- `isolation`
- `self-host`

Determinism and contract checks live under `tests/framework/` and are exposed via flake checks.
