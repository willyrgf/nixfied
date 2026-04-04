# Architecture

Nixfied is a model-first framework.

Typed Nix modules compile into a canonical `nixfiedModel`, and runtime interfaces (help text, app surfaces, and workflows) are generated from that model.
The canonical model is intentionally cheap: heavy service runtime normalization and package resolution stay outside `model.*` and are only materialized for selected execution surfaces.

## Model Pipeline

`modules -> resolved config -> compiler passes -> cheap nixfiedModel + scoped runtime materialization -> stateHash + runtimeHash + apps + execution manifests + introspection`

Compiler pass order (high-level core flow):

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

This is not a literal exhaustive call graph of `nixfied/compiler/default.nix`.
Additional derived compiled surfaces are still built around this core flow today,
including selection, app execution manifests, introspection, runtime manifests,
and runtime metadata.

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
- Runtime-facing task/workflow metadata is materialized separately as `compiled.runtimeMetadata` and embedded in selected execution manifests.
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
- The canonical executed proof for launcher sugar runs through the source-repo framework test workflow surface, for example `nix run .#run-task -- task.test.framework.feature-proof.e2e --summary`, backed by `tests/framework/launcher-skip-service-pruning-smoke.nix`.
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

- current public names still remain dispatcher -> orchestrator -> executor
- current public runtime path is public flake launcher -> selector-aware pure
  app selection -> scoped dispatcher -> orchestrator -> executor -> coarse
  kernel runtime
- `dispatcher` and separate runtime-control surfacing are current-state seams,
  not target architecture
- service contract -> generated `svc::...` apps / `SVC_...` hooks -> task runtime shell

Service selection and env scoping:

- `nixfied/framework/runtime/service-selection.nix` still exists today, but it
  is only a runtime alias over compiler selection data and is a deletion target,
  not a long-term layer.
- Public task apps plus `run-task`, `run-workflow`, and `run-workflow-parallel` are two-stage surfaces: select first, then execute a scoped runtime.
- `svc::<service>::<op>` surfaces are single-service scoped.
- Per-service `SVC_*` hook env and `NIXFIED_SERVICE_*` env are exported only for the selected service set of the current invocation.
- `NIXFIED_SERVICE_ROOT` remains runtime-global; per-service env does not remain ambient.

Execution contracts:

- Orchestrator-owned run lifecycle/state for all execution surfaces.
- Kernel-owned workflow driving, workflow phases, scheduler transitions,
  registry append/detail derivation, summary composition, and contract
  validation.
- Root task dependency execution is not yet kernel-owned end to end; current
  code still crosses shell through a kernel-generated plan/export path.
- Shell runtime is not thin yet. It still owns process supervision, run
  inventory, stop controls, hook choreography, task launch edges, and some
  summary/metadata transport. The target architecture is to shrink shell to
  OS-edge concerns only.
- Foreground/background process policy (`--fg` / `--bg`) with run inventory and stop controls.
- Hermetic runtime inputs for each task.
- Deterministic defaults for locale, timezone, umask, and workdir policy.
- Workspace-scoped default registry and artifact roots.
- Atomic run metadata and summary writes.
- Ephemeral env isolation for cache, temp, service, and registry state.
- Stable CLI prefixes (`INFO:`, `WARN:`, `ERROR:`, `OK:`, `SKIP:`).
- Deterministic scheduling and lock behavior.

## Contract Boundaries

Machine-facing boundaries are contract-owned.

- `nixfied/contracts/` defines the typed Nix contract DSL and compiles closed validation bundles and contract docs from the same source.
- App machine output uses typed `validation.contractRef` rather than inline schema fragments or runtime validator commands.
- Machine-output transport is explicit: target apps write the payload to `NIXFIED_MACHINE_OUTPUT_FILE`, and the wrapper validates that declared file instead of recovering payloads from stdout.
- Workflow machine output is sidecar-only: `run-workflow` writes `--run-id-file` and `--summary-file`, and no longer exposes a stdout JSON result surface.
- `nixfied-kernel` owns framework validation, coarse workflow execution, run-record transitions, registry append/replay/status projection, summary composition, runtime-event policy/state derivation, and probe execution.
- Workflow summaries, service-set JSON export, orchestrator run records,
  registry events, and introspection JSON are versioned validated envelopes.
  Shell runtime code still reads some kernel-exported fields through
  `runtime-metadata.nix` and export files today; the target is one coarse
  structured handoff rather than a fine-grained getter layer.
- `introspect` is backed by a compile-time bundle plus a thin runtime selector, and its generated JSON assets are validated against the contract bundle at build time.
- Repository guard tests and runtime contracts currently mix product guarantees
  with temporary deletion policy. The long-term harness should keep behavior
  proofs and shed seam-freezing guards as seams disappear.

## Generated App Surfaces

Core model-generated apps:

- `build`
- `check`
- `check-ports`
- `ci`
- `dev`
- `format`
- `framework::install`
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
- `nixfied/framework/runtime/`: public dispatcher/orchestrator/executor shells, env sandbox, and thin runtime adapters around kernel-owned semantics.
- `nixfied/framework/runtime/registry/`: NDJSON event log, replay, snapshot logic, and contract-backed append helpers.
- `nixfied/framework/core/`: canonical flake/core helpers and `mkNixfied`.
- `nixfied/framework/contracts/`: typed contract definitions and runtime artifact contract bundles.
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
- contract migration guard
- log prefix contract
- runtime service-selection contract
- launcher skip-service pruning contract
- service hook env scoping contract
