# Architecture

Nixfied is a model-first framework.

Typed Nix modules compile into a canonical `nixfiedModel`, and runtime interfaces (help text, app surfaces, and workflows) are generated from that model.
The canonical model is intentionally cheap: heavy service runtime normalization and package resolution stay outside `model.*` and are only materialized for selected execution surfaces.

## Model Pipeline

`modules -> resolved config -> compiler passes -> cheap nixfiedModel + scoped runtime materialization -> stateHash + runtimeHash + apps + introspection`

Compiler pass order:

1. `resolve-modules`
2. `normalize-runtime`
3. `compile-service-catalog`
4. `compile-services`
5. `compile-tasks`
6. `compile-workflows`
7. `compile-features`
8. `compile-views`
9. `finalize-model`

Each pass is pure and deterministic.

## Model Separation

The canonical exported model contains:

- `schema`
- `identity`
- `runtime`
- `serviceCatalog`
- `tasks`
- `workflows`
- `features`
- `views`
- `state`

Separation rules:

- `model.serviceCatalog` is the cheap canonical service view used by introspection, hashing, and selection.
- Heavy normalized service runtime remains outside `model.*` as internal `compiled.services`.
- `stateHash` hashes the cheap canonical model.
- `runtimeHash` is a separate deterministic fingerprint of heavy service runtime inputs and is used by dispatcher/orchestrator/executor run identity.

Graph exclusion is resolved before service compilation:

- `nixfied.graph.excludedServices` is consumed during project service projection.
- Excluded services are removed before their project config branches are selected.
- Task and workflow-unit service requirements are declared only through `requirements.services`.
- Later compiler passes then prune dependent tasks, workflow units, features, and views.
- Runtime `SKIP_<SERVICE>` remains a separate execution-time control and does not change graph selection.
- Public flake task/workflow surfaces use thin launchers that accept explicit compile-time selectors such as `--exclude-services helios`, then perform a second pure evaluation of the selected app.
- Truthy `SKIP_<SERVICE>` env vars may be used as launcher sugar, but the canonical compile-time interface is the explicit selector.
- The canonical executed proof for launcher sugar is `nix run .#framework::test -- --shard launcher-pruning`, backed by `tests/framework/launcher-skip-service-pruning-smoke.nix`.
- Service operation surfaces are generated from the compiled service graph. Projects should consume `SVC_<SERVICE>_<OP>` hook env vars or `svc::<service>::<op>` apps instead of importing framework service modules directly.

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
- public flake launcher -> selector-aware pure app selection -> scoped dispatcher -> orchestrator -> executor
- service contract -> generated `svc::...` apps / `SVC_...` hooks -> task runtime shell

Service selection and env scoping:

- `nixfied/framework/runtime/service-selection.nix` derives sorted service sets from task requirements, recursive task deps, workflow units, workflow `preRun`/`postRun` tasks, `workflowRef` targets, workflow mode resolution, and explicit launcher selectors.
- Public task apps plus `run-task`, `run-workflow`, and `run-workflow-parallel` are two-stage surfaces: select first, then execute a scoped runtime.
- `svc::<service>::<op>` surfaces are single-service scoped.
- Per-service `SVC_*` hook env and `NIXFIED_SERVICE_*` env are exported only for the selected service set of the current invocation.
- `NIXFIED_SERVICE_ROOT` remains runtime-global; per-service env does not remain ambient.

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

- `introspect`
- `stateHash`
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
- runtime service-selection contract
- launcher skip-service pruning contract
- service hook env scoping contract
