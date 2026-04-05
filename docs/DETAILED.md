# Detailed Architecture

## Model Contract

`nixfiedModel` is the canonical compiler output. Its top-level exported structure remains:

- `schema`
- `identity`
- `runtime`
- `serviceCatalog`
- `tasks`
- `workflows`
- `features`
- `views`
- `state`

The exported model is the cheap canonical view used for hashing, docs, help, and introspection.

Compiler-owned semantic artifacts are published under `model.compiled.*`, including:

- `compiled.services`
- `compiled.execution`
- `compiled.serviceSurfaceCatalog`
- `compiled.validationIr`

The compiler also publishes contract bundle/docs and introspection graph/bundle as first-class artifacts.

## Compilation Passes

Deterministic pass order:

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

`stateHash` is `sha256(toCanonicalNix(model))`.

`runtimeHash` fingerprints heavy compiled runtime inputs separately from `stateHash` and is used in runtime run identity.

## Graph Exclusion

Compile-time graph exclusion is configured through `nixfied.graph.excludedServices`.

Properties:

- exclusion happens before project service projection
- excluded service branches are not evaluated
- dependent tasks, workflow units, features, and views are pruned during compilation
- the pruned graph is reflected in help/docs/features/introspection

Invocation-time exclusion is separate:

- use `--exclude-services <csv>`
- owned by runtime
- does not re-evaluate Nix
- only affects execution of an already-compiled graph

Task and workflow-unit service requirements are declared through `requirements.services`. Those requirements drive both compile-time pruning and runtime exclusion.

## Runtime Entry

One public runtime engine fronts execution:

- `nix run .#run-task -- <task-id> [--exclude-services <csv>] [-- ...]`
- `nix run .#run-workflow -- <workflow-id> [--exclude-services <csv>] [-- ...]`
- `nix run .#run-workflow-parallel -- <workflow-id> [--exclude-services <csv>] [-- ...]`
- `nix run .#runs [-- <run-id>]`
- `nix run .#stop-run -- <run-id>`
- `nix run .#stop-all-runs`

Thin flake apps for tasks, workflows, and service operations exec that runtime directly.

Runtime option parsing:

- consumes `--exclude-services <csv>` as the surviving invocation-time service selector
- stops parsing runtime options at the first non-runtime arg or `--`
- forwards remaining args unchanged to the target task/workflow/service op

## Service ABI

The only surviving public service ABI is:

- `svc::<service>::<op>`

The runtime publishes those service operation apps from the compiled service surface catalog.

Inside task shells, the helper:

- `svc <service> <op> [args...]`

forwards to the same runtime-owned dispatch path.

Removed public contracts:

- `SKIP_<SERVICE>`
- `SVC_<SERVICE>_<OP>`
- `svcset::*`
- `services-*`

## Execution Graph

`compiled.execution` is the canonical execution representation consumed by runtime.

It carries the task and workflow execution graph the runtime needs for:

- task execution
- workflow execution
- workflow mode resolution
- workflow service-phase policy
- closure-selected service resolution
- runtime help and introspection linkage

Packaging no longer synthesizes app-scoped execution manifests as a second semantic layer.

## Runtime Responsibilities

The runtime engine and its leaf runtime components own:

- invocation-time exclusion
- task/workflow/service dispatch
- run inventory
- stop controls
- artifact directory setup
- summary file handling
- foreground/background process behavior
- process-group lifecycle
- ephemeral execution behavior

`nixfied-kernel` remains the framework-owned semantic layer for:

- workflow driving
- scheduler transitions
- registry append/replay/status projection
- summary composition
- validation and contract checks
- probe execution

## State Policy

Default state policy remains workspace-scoped.

Default roots include:

- runtime base under cache/workspace state
- registry root under cache/workspace state
- artifacts root under `/tmp`

The runtime preserves deterministic state derivation and isolation per workspace and per run.

## Ephemeral Execution

Ephemeral execution remains a first-class runtime guarantee.

The runtime preserves:

- isolated `HOME`, `TMPDIR`, and `XDG_*`
- isolated `REGISTRY_ROOT`
- isolated `CI_ARTIFACTS_DIR`
- isolated `NIXFIED_SERVICE_ROOT`
- failure retention and deterministic pruning policy
- budget checks before copy

## Published Artifacts

First-class published artifacts include:

- bundled schemas
- validation IR
- contract bundle
- contract docs
- introspection graph
- introspection bundle
- generated help/docs/features views

These are compiler-owned artifacts. Packaging may expose them, but does not become a second semantic owner.

## Registry Contract

Registry state remains NDJSON-based with replay support.

Product guarantees include:

- append-only event log
- stable key ordering
- atomic metadata/summary writes
- deterministic status projection
- stable ASCII log prefixes:
  - `INFO:`
  - `WARN:`
  - `ERROR:`
  - `OK:`
  - `SKIP:`
