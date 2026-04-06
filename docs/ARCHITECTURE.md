# Architecture

Nixfied is a model-first framework with a strict compiler/runtime split.

Typed Nix modules compile into one canonical `nixfiedModel`. The compiler owns semantic artifacts. Packaging publishes those artifacts. Runtime owns invocation-time selection and execution behavior.

## Ownership

Compiler-owned artifacts:

- canonical exported model
- execution graph under `model.compiled.execution`
- service surface catalog under `model.compiled.serviceSurfaceCatalog`
- validation IR
- contract bundle and generated contract docs
- introspection graph and introspection bundle
- generated help/docs/features views

Runtime-owned behavior:

- `run-task`
- `run-workflow`
- `run-workflow-parallel`
- `runs`
- `stop-run`
- `stop-all-runs`
- service dispatch for `svc::<service>::<op>`
- invocation-time exclusion via `--exclude-services <csv>`
- run inventory, stop behavior, summaries, ephemeral execution, process setup, process-group lifecycle, and artifact placement

Packaging responsibilities:

- thin flake apps that exec the runtime engine
- publication of compiler-owned artifacts
- no second semantic owner for selection or execution planning

## Model Pipeline

High-level compiler flow:

1. `resolve-modules`
2. `compile-state-policy`
3. `normalize-runtime`
4. `compile-service-catalog`
5. `compile-services`
6. `compile-tasks`
7. `compile-workflows`
8. `compile-features`
9. `compile-views`
10. `finalize-model`

The canonical exported model stays cheap. Heavy service runtime normalization remains internal as `compiled.services`. The compiler also emits the execution graph, service ABI catalog, validation IR, contract bundle/docs, and introspection assets.

## Public Surfaces

Core apps remain model-generated, for example:

- `build`
- `check`
- `ci`
- `dev`
- `format`
- `health`
- `ports`
- `ready`
- `test`
- `validate-env`

Runtime apps:

- `run-task <task-id> [--exclude-services <csv>] [-- ...]`
- `run-workflow <workflow-id> [--exclude-services <csv>] [-- ...]`
- `run-workflow-parallel <workflow-id> [--exclude-services <csv>] [-- ...]`
- `runs [run-id]`
- `stop-run <run-id>`
- `stop-all-runs`

Service ABI:

- `svc::<service>::<op>`

Published introspection and schema surfaces:

- `docs`
- `features`
- `help`
- `introspect`
- `schema`
- `stateHash`

Removed public families:

- `svcset::*`
- `services-*`
- `SKIP_<SERVICE>`
- `SVC_<SERVICE>_<OP>`

## Selection Semantics

There are exactly two service-selection mechanisms:

- Static graph exclusion: `nixfied.graph.excludedServices`
- Invocation-time exclusion: `--exclude-services <csv>`

Static exclusion is compiler-owned and removes service branches before project service projection and downstream graph compilation.

Invocation-time exclusion is runtime-owned and filters service execution for a specific run. It does not re-evaluate Nix.

Task and workflow service requirements are declared through `requirements.services`. Those requirements drive both compile-time pruning and runtime exclusion behavior.

## Runtime Structure

Public runtime flow:

`flake app -> nixfied-runtime -> orchestrator or service-dispatch leaf logic -> nixfied-kernel`

The runtime engine is the only public owner for task/workflow execution, service dispatch, run inventory, stop controls, summaries, ephemeral runs, and invocation-time service exclusion.

The kernel remains the framework-owned semantics layer for workflow driving, registry/status projection, summary composition, validation, and probe execution.

## Service Dispatch

The only surviving public service ABI is `svc::<service>::<op>`.

Inside task shells, the runtime helper `svc <service> <op>` forwards through the internal service dispatcher path exposed by `NIXFIED_RUNTIME_BIN`. Ambient `SVC_*` hook env vars are not a public contract.

Workflow pre/post service phases use compiled service-set policy internally, but service-set app families are not published.

## State and Isolation

The runtime preserves product guarantees for:

- workspace-scoped state roots
- deterministic registry and artifact placement
- foreground/background process control
- process-group lifecycle and stop behavior
- run inventory
- summary generation
- ephemeral execution
- deterministic assessment and validation
- stable ASCII output prefixes

## Repository Layout

- `nixfied/compiler/`: compiler passes and semantic artifact generation
- `nixfied/framework/core/`: core flake assembly and publication helpers
- `nixfied/framework/runtime/`: runtime engine and leaf runtime logic
- `nixfied/framework/runtime/registry/`: NDJSON event store, replay, and snapshot support
- `nixfied/framework/install/`: install and upgrade internals
- `nixfied/project/`: project-owned composition, tasks, workflows, and config
- `tests/framework/`: deterministic framework checks for surviving behavior and guarantees

## Determinism Gates

The framework test suite is expected to cover surviving user-facing behavior and real compiler/runtime guarantees, including:

- help and command surface contracts
- contract bundle and validation IR publication
- introspection graph/bundle publication
- runtime service exclusion behavior
- registry replay and event contracts
- summary generation
- ephemeral isolation
- process lifecycle and stop behavior
