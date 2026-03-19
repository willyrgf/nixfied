# Architecture

Nixfied is a model-first framework.

Typed Nix modules compile into a canonical `nixfiedModel`, and runtime interfaces (help text, app surfaces, and workflows) are generated from that model.

## Model Pipeline

`modules -> resolved config -> compiler passes -> nixfiedModel -> stateHash + apps + introspection`

Compiler pass order:

1. `resolve-modules`
2. `normalize-runtime`
3. `compile-services`
4. `compile-tasks`
5. `compile-workflows`
6. `compile-features`
7. `compile-views`
8. `finalize-model`

Each pass is pure and deterministic.

Graph exclusion is resolved before service compilation:

- `nixfied.graph.excludedServices` is consumed during project service projection.
- Excluded services are removed before their project config branches are selected.
- Later compiler passes then prune dependent tasks, workflow units, features, and views.
- Runtime `SKIP_<SERVICE>` remains a separate execution-time control and does not change graph selection.

## Runtime Structure

Task/workflow execution is process-first and uses:

- `run-task <task-id> [-- ...]`
- `run-workflow <workflow-id> [-- ...]`
- `run-workflow-parallel <workflow-id> [-- ...]`
- `runs [run-id]`
- `stop-run <run-id>`
- `stop-all-runs`

Runtime path:

- dispatcher -> orchestrator -> executor

Execution contracts:

- Orchestrator-owned run lifecycle/state for all execution surfaces.
- Foreground/background process policy (`--fg` / `--bg`) with run inventory and stop controls.
- Hermetic runtime inputs for each task.
- Deterministic defaults for locale, timezone, umask, and workdir policy.
- Workspace-scoped default registry and artifact roots.
- Atomic run metadata and summary writes.
- Ephemeral env isolation for cache, temp, service, and registry state.
- Stable CLI prefixes (`INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`).
- Deterministic scheduling and lock behavior.

## Generated App Surfaces

Core model-generated apps:

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

- `model`
- `stateHash`
- `tasks`
- `services`
- `task::<id>`
- `schema`

## Project Layout

- `flake.nix`: top-level flake entrypoint.
- `nixfied/project/`: project configuration, composition, and project-owned runtime/task/workflow definitions.
- `nixfied/modules/`: typed option modules.
- `nixfied/compiler/`: model compilation passes.
- `nixfied/framework/runtime/`: dispatcher, orchestrator, executor, env sandbox, and runtime helpers.
- `nixfied/framework/runtime/registry/`: NDJSON event log, replay, and snapshot logic.
- `nixfied/framework/core/`: canonical renderer, flake/core helpers, and `mkNixfied`.
- `nixfied/framework/install/`: wrapper/install internals and vendored-wrapper generation.
- `tests/framework/`: deterministic framework gates and snapshots.

## Determinism Gates

Authoritative checks in `tests/framework/` include:

- model hash stability
- cross-machine hash stability
- scheduler order determinism
- help snapshot contract
- registry replay/events contract
- executor/env-sandbox contracts
- log prefix contract
