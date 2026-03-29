# Layer Decomposition

Status: working note

Date: 2026-03-29

## Purpose

This document decomposes the current Nixfied architecture into explicit layers
and identifies what is necessary in each one.

The goal is not to propose a final design yet.

The goal is to make solution evaluation possible by separating:

- irreducible responsibilities
- responsibilities that can move but still need to exist somewhere
- duplicated or accidental responsibilities that should be deleted

This document should be read alongside:

- `RFC_LAYERS_REFACTOR.md`
- `docs/ARCHITECTURE.md`
- `docs/DETAILED.md`

## Reading Guide

For each layer, this document uses four categories:

- `Necessary`:
  responsibilities that a functioning system still needs somewhere
- `Accidental`:
  responsibilities present mostly because the stack is split across too many
  seams
- `Keep`:
  the layer should remain, though perhaps in a smaller shape
- `Collapse/Delete pressure`:
  the layer should either merge into an adjacent one or lose specific seams

The working simplification rule is:

if a layer is only carrying query glue, fallback materialization, transport
bridges, or duplicate authority, it is under deletion pressure.

## Surface-Area Facts

These current facts matter when evaluating options:

- public flake apps stayed flat at `38`
- shell-heavy runtime control files still total about `4,738` LOC, or `5,656`
  if `runtime/env-sandbox.nix` is included
- generic service glue around runtime surfaces still totals about `3,143` LOC
- sampled service implementation files total about `1,118` LOC
- the kernel currently exposes `34` concrete leaf operations across validation,
  run-record, task, workflow, registry, summary, probe, and machine-output
  families
- current notable runtime manifest or metadata families are `9`, down from about
  `14` before workflow-state and task-plan cleanup landed

These numbers imply:

- the problem is not just "kernel got big"
- the problem is "authority improved without enough end-to-end layer deletion"

## Current Top-Level Stack

The current stack can be decomposed as:

1. project authoring layer
2. module schema layer
3. model compiler layer
4. execution materialization layer
5. flake surface and launcher layer
6. runtime selection and manifest narrowing layer
7. dispatcher layer
8. orchestrator layer
9. executor layer
10. runtime metadata projection and shell query layer
11. env and isolation layer
12. kernel runtime layer
13. service boundary helper layer
14. test and contract layer

Some of these are clearly fundamental.

Some are current implementation seams rather than architectural necessities.

## Compile-Time Layers

### Summary

| Layer | Keep? | Why it exists |
| --- | --- | --- |
| Project authoring | Keep | projects need a declarative surface for tasks, workflows, services, state policy |
| Module schema | Keep | typed options and validation of author intent |
| Model compiler | Keep | transforms module config into a canonical runtime model |
| Execution materialization | Keep, but shrink | turns compiled model into apps and service/runtime surfaces |
| Flake surfacing and launchers | Keep, but shrink | preserves stable `nix run .#<cmd>` entrypoints |
| Runtime selection and manifest narrowing | Keep, but collapse duplicates | narrows execution to selected service scope and app-specific model |

### 1. Project Authoring Layer

Current files:

- `nixfied/project/conf.nix`
- `nixfied/project/module.nix`
- `nixfied/project/tasks.nix`
- `nixfied/project/workflows.nix`
- `nixfied/project/runtime.nix`
- `nixfied/project/services.nix`

Necessary:

- project-specific declaration of tasks, workflows, services, envs, and state
  policy
- project-owned command contracts and defaults
- project-specific composition of framework presets and runtime behavior

Accidental:

- framework-level runtime details leaking into project-owned task definitions
- project-side knowledge about shell runtime transport, if any
- compatibility or legacy shims that survive only because lower layers are not
  singular
- the current `conf.nix -> runtime.nix/services.nix/tasks.nix/workflows.nix ->
  module.nix` projection pattern, which means project intent is first expressed
  as an untyped preset record and then re-projected back into typed `nixfied.*`
  config

Keep:

- yes

Collapse/Delete pressure:

- low
- this layer is not the main source of generic fat

What to evaluate in solutions:

- does the solution reduce how much runtime wiring project tasks need to know?
- does it keep project config purely declarative?

### 2. Module Schema Layer

Current files:

- `nixfied/modules/core.nix`
- `nixfied/modules/runtime.nix`
- `nixfied/modules/tasks.nix`
- `nixfied/modules/workflows.nix`
- `nixfied/modules/operations.nix`
- `nixfied/modules/service-sets.nix`
- `nixfied/modules/services/*`

Necessary:

- typed option schema for project and framework features
- stable config vocabulary for tasks, workflows, state, services, contracts
- early compile-time validation of user intent

Accidental:

- any option surface that exists only to feed a specific shell runtime seam
- compatibility options kept alive after the underlying runtime seam was replaced
- modules that both define schema and synthesize derived runtime structure, for
  example the default service-set emission in `modules/service-sets.nix` and the
  mixed option-plus-task/app emission in `modules/operations.nix`

Keep:

- yes

Collapse/Delete pressure:

- medium only if options exist purely to support removed runtime seams

What to evaluate in solutions:

- can a proposed deletion remove option fields that only exist to support
  shell-era transports?

### 3. Model Compiler Layer

Current files:

- `nixfied/compiler/default.nix`
- `nixfied/compiler/resolve-modules.nix`
- `nixfied/compiler/finalize-model.nix`
- `nixfied/compiler/compile-execution.nix`
- `nixfied/compiler/compile-validation-ir.nix`
- `nixfied/compiler/compile-service-catalog.nix`
- `nixfied/compiler/compile-service-sets.nix`
- `nixfied/compiler/compile-service-surface-catalog.nix`
- `nixfied/compiler/compile-tasks.nix`
- `nixfied/compiler/compile-workflows.nix`

Necessary:

- resolve modules into a canonical model
- normalize runtime and state policy
- compile validation plans
- compile service catalog and service-set structure
- compile task and workflow semantics
- compile selected app surfaces

Accidental:

- distinct manifest families when one would do
- runtime metadata forms that exist only because shell still wants a metadata API
- compile outputs duplicated again later by runtime fallbacks

Keep:

- yes

Collapse/Delete pressure:

- medium to high on manifest proliferation
- medium on compile outputs that are later recomputed in runtime

What to evaluate in solutions:

- can `runtime-metadata` be absorbed into execution manifests or another single
  runtime data structure?
- can selection become compile-only with no runtime fallback import?

### 4. Execution Materialization Layer

Current files:

- `nixfied/framework/core/materializeExecution.nix`
- `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
- `nixfied/framework/core/mkServiceSetPrograms.nix`
- `nixfied/framework/core/mkMachineOutputPrograms.nix`

Necessary:

- materialize app programs from compiled model
- bind selected-service scope to concrete runtime surfaces
- produce service operation surfaces and machine-output wrappers

Accidental:

- duplicate service-set program materialization when executor can synthesize them
  again
- per-app runtime construction that is later duplicated or queried through
  another shell metadata seam
- large generic service glue that is bigger than many service implementations

Keep:

- yes, but this layer is under strong slimming pressure

Collapse/Delete pressure:

- high on duplicated `serviceSetPrograms` authority
- medium on generic service glue that exists because service boundaries are still
  too framework-centric

What to evaluate in solutions:

- should this be the single source of service-set programs?
- should machine-output, probe, and service operation programs stay here or be
  further normalized?

### 5. Flake Surface And Launchers Layer

Current files:

- `nixfied/framework/core/mkCoreSurfaces.nix`
- `nixfied/framework/core/mkFlakeOutputs.nix`
- `nixfied/framework/core/mkLauncherMetadata.nix`
- `nixfied/framework/launch/run-runtime-app.nix`
- `nixfied/framework/launch/run-selected-app.nix`

Necessary:

- preserve stable flake app names
- keep flake evaluation reasonably cheap
- expose help, docs, schema, introspection, and runtime launch surfaces

Accidental:

- launcher wrapping layers that exist only because dispatcher and orchestrator
  have not collapsed
- duplicate runtime-control surfacing between flake outputs and dispatcher apps

Keep:

- yes

Collapse/Delete pressure:

- medium
- flake surfaces need to exist, but the number of shell wrapper hops should be
  minimized

What to evaluate in solutions:

- can dispatcher disappear as a separate shell layer while keeping flake app
  names stable?

### 6. Runtime Selection And Manifest Narrowing Layer

Current files:

- `nixfied/compiler/compile-execution.nix`
- `nixfied/framework/core/mkFlakeOutputs.nix`
- `nixfied/framework/core/materializeExecution.nix`

Necessary:

- map public command surfaces to selected service closure
- narrow model and services for a selected app
- preserve service isolation and deterministic runtime scope

Accidental:

- shell selection CSV logic still lives in launcher wrappers and app-resolution
  paths
- narrowed execution manifests and nested runtime metadata projections still
  exist beside the canonical execution graph
- selected-service closure logic is still spread across compiler execution data,
  launcher resolution, and runtime materialization
- the named `service-selection.nix` seam is gone, but the duplicated selection
  authority still exists

Keep:

- yes

Collapse/Delete pressure:

- high on runtime fallback recomputation
- medium on duplicative selection helpers

What to evaluate in solutions:

- should all selection become compile-only and manifest-backed?
- can `selectionIndex` fallback code simply be deleted first, before any deeper
  runtime redesign?

## Shell Runtime Layers

### Summary

| Layer | Keep? | Why it exists |
| --- | --- | --- |
| Dispatcher | Keep only if truly thin | public shell entrypoints and usage framing |
| Orchestrator | Keep unless kernel takes over process supervision | run lifecycle and stop controls |
| Executor | Keep unless kernel takes over execution edges | task launch, hooks, service-set adapter launch |
| Runtime metadata projection and shell query glue | Strong collapse pressure | project compiled runtime data into shell-consumable query paths |
| Env and isolation | Keep | shell-native env bootstrapping and isolation |
| Shared/common runtime helpers | Keep selectively | atomic file writes, common shell utilities, run-id env prep |

### 7. Dispatcher Layer

Current files:

- `nixfied/framework/runtime/dispatcher.nix`

Necessary:

- present stable public runtime commands
- basic usage and help framing
- forward to a lower execution layer

Accidental:

- carrying substantial runtime-control surfacing if those commands are already
  exposed elsewhere
- existing as a separate layer if it does nothing but tail-call orchestrator

Keep:

- maybe

Collapse/Delete pressure:

- medium to high

Minimal retained role:

- if kept, it should be a very small app wrapper layer only

What to evaluate in solutions:

- can launchers call orchestrator or a future single runtime binary directly?

### 8. Orchestrator Layer

Current files:

- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/orchestrator-runtime.nix`

Necessary:

- foreground and background process policy if shell remains the OS process
  supervisor
- run inventory
- stop controls and signal forwarding
- run-file path conventions and caller-facing process lifecycle behavior

Accidental:

- wrapper functions around kernel `run-record read`
- duplicate runtime-control app surfacing
- any recomputation of compile-time selection data
- logic that only exists to bridge shell back into kernel metadata helpers

Keep:

- probably yes unless process supervision moves into Rust

Collapse/Delete pressure:

- medium on the layer itself
- high on duplicated control surfacing and helper indirection inside it

Minimal retained role:

- own live process supervision and stop controls
- avoid semantic runtime queries that could be resolved before reaching it

What to evaluate in solutions:

- do we want kernel-owned child process supervision?
- if not, how small can orchestrator become while still owning bg/fg behavior?

### 9. Executor Layer

Current files:

- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/runtime/executor-runtime.nix`

Necessary:

- launch actual task runners
- invoke hooks
- set up shell runtime environment
- invoke service-set phase adapters
- integrate machine-output and workflow execution entrypoints

Accidental:

- task dependency execution loops driven from shell around kernel results
- temp-file choreography for task execution exports
- summary export sourcing
- dependence on the separate shell metadata getter layer
- synthesis fallback for service-set programs

Keep:

- probably yes if shell remains the child-process launcher

Collapse/Delete pressure:

- high on task dependency loop glue
- high on summary export sourcing
- high on metadata getter dependence
- high on service-set synthesis fallback

Minimal retained role:

- launch commands and hooks
- own shell-native env bootstrapping
- avoid being a semantic planner

What to evaluate in solutions:

- should the kernel own task dependency execution fully?
- should summary and runtime metadata stop round-tripping through shell exports?

### 10. Runtime Metadata Projection And Shell Query Layer

Current files:

- `nixfied/compiler/default.nix`
- `nixfied/compiler/compile-execution.nix`
- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/kernel/src/task.rs`
- `nixfied/framework/runtime/kernel/src/workflow.rs`

Necessary:

- none as a separate architectural layer

Necessary if shell remains large:

- some way for shell to access task and workflow runtime data

Accidental:

- top-level runtime metadata compilation remaining separate from the canonical
  execution graph
- shell still asking the kernel for task and workflow runtime fields piecemeal
- loader commands and runtime projections surviving after the named
  `runtime-metadata.nix` file was deleted

Keep:

- no strong architectural reason to keep this as its own layer

Collapse/Delete pressure:

- very high

Minimal retained role:

- ideally none
- next best would be one coarse runtime handoff rather than a getter library

What to evaluate in solutions:

- can executor and orchestrator consume one runtime blob instead of many query
  calls?

### 11. Env And Isolation Layer

Current files:

- `nixfied/framework/runtime/env-sandbox.nix`
- `nixfied/framework/runtime/ephemeral.nix`
- `nixfied/framework/runtime/common-runtime.nix`
- `nixfied/framework/runtime/shared-runtime-lib.nix`

Necessary:

- deterministic shell runtime env
- hermetic `PATH` from declared runtime inputs
- workdir and locale policy
- ephemeral and workspace isolation
- atomic file writes and shared shell utilities

Accidental:

- if generic env bootstrapping is tightly coupled to unrelated orchestration
  logic
- if run-id or export helpers exist only to support transport seams that could
  be removed

Keep:

- yes

Collapse/Delete pressure:

- low on the isolation concern itself
- medium on helpers that exist mostly for old or overly fine-grained runtime
  transport

What to evaluate in solutions:

- can env/isolation remain shell-native while metadata and summary seams shrink?

## Kernel Runtime Layer

### Summary

| Kernel family | Keep? | Why it exists |
| --- | --- | --- |
| Validation | Keep | typed validation authority |
| Run ID | Keep | stable run identity and envelope derivation |
| Event / state / policy | Keep | canonical runtime event semantics |
| Run-record / registry / summary | Keep | authoritative run state and reporting |
| Workflow execution | Keep | scheduler and workflow semantic authority |
| Task query / metadata helpers | Shrink | part essential, part shell-induced |
| Probe / machine-output | Keep | canonical runtime probe and contract enforcement |
| Adapter decode | Shrink | mostly bridge glue |

### 12. Validation Layer

Current files:

- `nixfied/framework/runtime/kernel/src/main.rs`
- `nixfied/framework/runtime/kernel/src/validation.rs`

Necessary:

- payload validation
- env and arg validation
- scalar and exit validation

Accidental:

- none obvious in the validation core itself

Keep:

- yes

Collapse/Delete pressure:

- low

### 13. Run Identity Layer

Current files:

- `nixfied/framework/runtime/kernel/src/run_id.rs`

Necessary:

- deterministic run-id envelope calculation

Accidental:

- very little

Keep:

- yes

Collapse/Delete pressure:

- low

### 14. Event, State, And Policy Layer

Current files:

- `nixfied/framework/runtime/kernel/src/event.rs`
- `nixfied/framework/runtime/kernel/src/policy.rs`

Necessary:

- canonical event detail derivation
- state derivation
- service policy semantics

Accidental:

- anything that exists only to render shell-specific transport

Keep:

- yes

Collapse/Delete pressure:

- medium on shell-export rendering, not on semantic ownership

### 15. Run-Record, Registry, And Summary Layer

Current files:

- `nixfied/framework/runtime/kernel/src/run_record.rs`
- `nixfied/framework/runtime/kernel/src/registry.rs`
- `nixfied/framework/runtime/kernel/src/summary.rs`

Necessary:

- authoritative run-record creation and transitions
- registry append and replay
- terminal state derivation
- summary composition

Accidental:

- shell-export write paths that still exist because shell expects exports or temp
  files

Keep:

- yes

Collapse/Delete pressure:

- medium on transport format

What to evaluate in solutions:

- can summary become single-form output without shell export sourcing?

### 16. Workflow Execution Layer

Current files:

- `nixfied/framework/runtime/kernel/src/workflow.rs`

Necessary:

- workflow mode resolution or equivalent canonical workflow selection
- workflow scheduler transitions
- fail-fast logic
- phase task and service-set execution ordering
- workflow event append semantics

Accidental or shell-induced:

- `workflow load-runtime` if retained only for shell metadata getters
- any shell adapter contract that exists because execution still bounces back
  into shell for each task or service-set step

Keep:

- yes

Collapse/Delete pressure:

- low on scheduler ownership
- medium on query subcommands and shell adapter protocol

What to evaluate in solutions:

- should `workflow run` remain the sole workflow operation plus maybe one
  resolution operation, with `load-runtime` removed?

### 17. Task Layer

Current files:

- `nixfied/framework/runtime/kernel/src/task.rs`

Necessary:

- some task execution semantics if kernel remains involved in dependency and
  workflow integration
- arg validation and help rendering if these stay runtime-derived

Accidental or shell-induced:

- `exists`
- `workflow-ref`
- `load-runtime`
- `load-hook`

Those look more like a shell metadata API support surface than irreducible
runtime authority.

Keep:

- yes, but shrink

Collapse/Delete pressure:

- high on shell-facing query operations

What to evaluate in solutions:

- should task execution become a coarse kernel operation instead of
  `execution-order` plus shell loops?

### 18. Probe And Machine Output Layer

Current files:

- `nixfied/framework/runtime/kernel/src/probe.rs`
- `nixfied/framework/runtime/kernel/src/machine_output.rs`

Necessary:

- canonical probe semantics
- canonical machine-output contract enforcement

Accidental:

- very little in the semantic core

Keep:

- yes

Collapse/Delete pressure:

- low to medium on wrapper and transport details only

### 19. Adapter Decode Layer

Current files:

- `nixfied/framework/runtime/kernel/src/adapter.rs`

Necessary:

- some adapter decoding may still be needed while shell and service probes use
  text protocols

Accidental:

- likely a bridge layer created by surrounding shell and service adapter seams

Keep:

- maybe temporarily

Collapse/Delete pressure:

- medium

What to evaluate in solutions:

- can probe and service adapter contracts move to a simpler single-form result?

## Service Boundary Helper Layer

### Summary

| Layer | Keep? | Why it exists |
| --- | --- | --- |
| Service API helpers | Keep, but shrink | turn service contracts into service apps and env surfaces |
| Service runtime surfaces | Keep, but simplify | expose service-specific runtime hooks and apps |
| Service-set program generation | Keep, but make singular | generate service-set operations once |

### 20. Service API Helpers

Current files:

- `nixfied/framework/runtime/helpers/service-api.nix`
- `nixfied/framework/runtime/services/service-operations-builder.nix`
- `nixfied/framework/runtime/services/service-config-builder.nix`

Necessary:

- convert service contracts into public ops and hook env
- validate enabled services expose the expected public contract

Accidental:

- broad framework-level service glue because real services are not yet forced
  behind tiny public surfaces
- helper surface that is larger than the public API it is trying to describe

Keep:

- yes

Collapse/Delete pressure:

- medium to high

What to evaluate in solutions:

- can real services depend only on public contracts and generated surfaces?

### 21. Service Runtime Surface And Service-Set Generation

Current files:

- `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
- `nixfied/framework/core/mkServiceSetPrograms.nix`
- `nixfied/framework/core/materializeExecution.nix`

Necessary:

- generate service apps and hook env for the selected service scope
- generate service-set operations

Accidental:

- executor fallback synthesis of service-set programs
- large generic glue layer compared to actual service implementation code

Keep:

- yes

Collapse/Delete pressure:

- high on duplicate synthesis
- medium on overall generic glue size

## Test And Contract Layer

### Summary

| Layer | Keep? | Why it exists |
| --- | --- | --- |
| Compile/runtime contracts | Keep | preserve behavioral guarantees |
| Kernel semantic tests | Keep | protect moved Rust authority |
| Adapter and end-to-end smokes | Keep selectively | prove shell and runtime edges still work |
| Migration guards | Temporary | ensure deleted seams do not reappear |
| Coverage metadata and shard governance | Keep only if worth the cost | organize a large suite, but also add meta-machinery |
| Service extractability proof | Keep intent, replace proof style | current proof is too synthetic |

### 22. Compile And Runtime Contract Tests

Current files:

- `tests/framework/compiler-validation.nix`
- `tests/framework/feature-manifest-proof.nix`
- `tests/framework/selected-execution-contract.nix`
- `tests/framework/service-set-behavior-contract.nix`

Necessary:

- freeze behavioral contracts for selected app manifests and compile-time
  validation
- ensure model and manifest narrowing stay correct

Accidental:

- some fixture-heavy layering if it exists only because runtime metadata and app
  execution are separate families

Keep:

- yes

Collapse/Delete pressure:

- low on coverage intent
- medium on fixture seams if manifest families collapse

### 23. Kernel Semantic Tests

Current files:

- `tests/framework/kernel-native-tests.nix`
- workflow lifecycle and registry tests
- run-id and run-record tests
- probe and summary tests

Necessary:

- protect the semantic authority that moved into Rust

Accidental:

- little beyond whatever transport-specific fixtures they still need

Keep:

- yes

Collapse/Delete pressure:

- low

### 24. Adapter And E2E Shell Tests

Current files:

- adapter-smoke and orchestrator/executor shell tests in `tests/framework/*`

Necessary:

- verify launch edges
- verify signals, stop controls, env bootstrapping, and runtime isolation

Accidental:

- shell-behavior tests that exist only because shell still owns too much
  semantic control

Keep:

- yes, but should shrink if shell actually becomes thinner

Collapse/Delete pressure:

- medium over time

### 25. Migration Guards

Current files:

- `tests/framework/contract-migration-guard.nix`
- related migration checks

Necessary:

- temporary protection while unstable seam deletions are still landing

Accidental:

- this is governance tax once the new architecture is settled

Keep:

- temporary only

Collapse/Delete pressure:

- high after the relevant seam deletions are complete

### 26. Coverage Metadata And Shard Governance

Current files:

- `tests/framework/default.nix`
- `tests/framework/feature-coverage-validation.nix`
- `tests/framework/framework-test-shards.nix`
- `tests/framework/framework-test-shard-validation.nix`

Necessary:

- some test organization at current suite size

Accidental:

- per-check layer, proof-kind, canonical, covers, and owner-file metadata
  becoming a second architecture layer
- shard governance added because the runtime layer model is still large and
  evolving

Keep:

- maybe

Collapse/Delete pressure:

- medium

What to evaluate in solutions:

- if runtime layers collapse, does this governance machinery become smaller or
  partly unnecessary?

### 27. Service Extractability Proof

Current files:

- `tests/framework/service-extractability-contract.nix`

Necessary:

- a way to test the claim that services can stand behind a small public API

Accidental:

- current proof uses fixture modules, so it proves helper behavior more than it
  proves real service extractability

Keep:

- keep the goal, change the proof

Collapse/Delete pressure:

- high on the current synthetic form

## Cross-Layer Pressure Summary

### Layers that are clearly necessary

- project authoring
- module schema
- compiler core
- flake surfacing
- env and isolation
- kernel semantic authority
- core contract tests

### Layers that should remain but get thinner

- execution materialization
- launcher and selection surfaces
- orchestrator
- executor
- service API helpers

### Layers or seams under strongest collapse pressure

- separate shell runtime metadata layer
- runtime selection fallback recomputation
- executor-side service-set synthesis fallback
- duplicate runtime-control surfacing
- shell summary export sourcing
- shell-driven task dependency loop around kernel output
- synthetic service extractability proof form

## Solution-Evaluation Checklist

Use this checklist against any candidate solution.

1. Which layer disappears entirely?
2. Which layer keeps the same responsibility but becomes smaller?
3. Which duplicated authority becomes singular?
4. Which transport form disappears?
5. Which shell control-flow block goes away?
6. Which test layer becomes simpler as a result?
7. Does service split-readiness become more real or just more documented?

If a proposed solution cannot answer at least one of questions 1 through 5 with
something concrete, it is probably another reorganization rather than a
simplification.

## Current Best Bets

Based on the current decomposition, the lowest-regret simplification targets are:

1. delete runtime selection fallback recomputation
2. delete executor synthesis fallback for `serviceSetPrograms`
3. choose one runtime-control surfacing path
4. collapse or delete the shell runtime metadata getter layer
5. remove shell summary export sourcing

The higher-risk, higher-payoff targets are:

1. move task dependency execution fully into the kernel
2. force real services behind public contracts and generated surfaces only
3. reduce shell adapter protocol breadth inside workflow execution

## Final Note

The decomposition here is intentionally unsentimental.

The current architecture is not "bad".

It is:

- stronger in semantic ownership
- stronger in guarantees
- still over-layered for the amount of generic framework it performs

That means solutions should now be judged primarily by deletion pressure, not by
whether they present a cleaner conceptual diagram.
