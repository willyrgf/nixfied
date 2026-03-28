# Layer Options

Status: working note

Date: 2026-03-28

## Purpose

This document turns the current RFC, layer decomposition, and direct codebase
inspection into a decision aid.

It is intentionally not a generic architecture menu.

It is a repository-specific answer to:

1. which seams should actually be deleted
2. which current layers are real boundaries versus transport leftovers
3. which target architecture reduces the number of authorities rather than
   reorganizing the same framework fat

Read this alongside:

- `RFC_LAYERS_REFACTOR.md`
- `docs/LAYER_DECOMPOSITION.md`
- `docs/ARCHITECTURE.md`
- `docs/DETAILED.md`

## Working Verdict

The current stack does not need fourteen meaningful layers.

The smallest stable architecture visible in the current codebase is closer to:

1. typed authoring boundary
2. compiler authority
3. surface builder
4. shell runtime authority
5. kernel authority
6. guarantee harness

That means several current named layers are not real target abstractions:

- dispatcher
- shell runtime metadata
- runtime manifests as a separate family
- orchestrator-control as a separate runtime authority
- executor-side service-set synthesis

The main decision is therefore not "which wrapper layers do we like better?"

The main decision is:

- do we first unify compile-time execution knowledge into one canonical artifact
  and delete shell metadata/query seams
- then, after that, do we push further toward kernel-led task execution or
  compiler-owned service contracts

## Evaluation Frame

Use this scorecard when comparing options:

1. which current authority disappears entirely
2. which duplicated representation becomes canonical
3. which shell export or temp-file transport disappears
4. which runtime owner stops re-deriving compile-time knowledge
5. which migration-proof test machinery becomes deletable
6. whether the proposal adds a new meta-framework surface instead of removing
   one

The working scoring scale is:

- `none`: no meaningful effect
- `low`: localized cleanup only
- `medium`: clear seam reduction
- `high`: deletes or strongly collapses a whole authority or transport family

## Candidate Scorecard

| Candidate | Authority removed | Canonical form gained | Transport removed | Shell reduction | Test simplification | Risk | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `1` delete runtime selection fallbacks | medium | medium | none | low | low | low | removes fake runtime selection layer |
| `2` delete executor synthesis of `serviceSetPrograms` | medium | low | none | low | low | low-medium | restores one materialization authority |
| `3` merge runtime-control ownership | medium | none | none | medium | low | medium | removes `orchestrator-control` as a separate seam |
| `4` unify compiled execution artifacts | high | high | medium | medium | medium | high | biggest architectural payoff |
| `5` delete shell metadata getter and export transport | high | medium | high | high | medium | high | removes the clearest remaining shell-era seam |
| `6` make service contracts compiler-owned | high | high | low | medium | high | high | only worth it if it deletes duplicate service glue |
| `7` move task dependency execution fully into kernel | medium | medium | high | high | medium | high | valid only after compile/runtime data is singular |
| `8` prune seam-freezing harness | medium | none | none | none | high | medium | should happen after real seam deletion, not before |

## Candidate Notes

### Candidate 1: delete runtime selection fallbacks

Primary effect:

- remove runtime recomputation of `selectionIndex`
- make selection compile-only in practice, not just in documentation

Files under pressure:

- `nixfied/framework/runtime/service-selection.nix`
- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/executor.nix`

Why it matters:

- this is low-regret cleanup
- it turns a claimed boundary into a real invariant

Why it is not enough:

- the system still keeps multiple compiled execution representations alive

### Candidate 2: delete executor synthesis of `serviceSetPrograms`

Primary effect:

- `materializeExecution.nix` becomes the only authority for service-set programs

Files under pressure:

- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/core/materializeExecution.nix`

Why it matters:

- duplicate materialization is framework fat, not abstraction

### Candidate 3: merge runtime-control ownership

Primary effect:

- delete `orchestrator-control.nix` as a separate runtime authority
- route `runs`, `stop-run`, and `stop-all-runs` through one shell runtime owner

Files under pressure:

- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/orchestrator-control.nix`
- `nixfied/framework/runtime/dispatcher.nix`
- `nixfied/framework/core/mkFlakeOutputs.nix`

Why it matters:

- duplicate surfacing is not the main problem
- duplicate ownership of run inventory and stop behavior is

### Candidate 4: unify compiled execution artifacts

Primary effect:

- replace the current family of:
  - `selectionIndex`
  - `compiled.runtimeMetadata`
  - app-specific recomputed runtime metadata
  - `runtimeManifests`
  - launcher CSV lookup tables
- with one canonical compiled execution graph

What that graph should own:

- task and workflow semantic descriptors
- selected-service closures
- workflow mode resolution inputs
- per-app narrowed execution projections
- runtime handoff data needed by shell and kernel

Files under pressure:

- `nixfied/compiler/compile-selection-index.nix`
- `nixfied/compiler/compile-runtime-metadata.nix`
- `nixfied/compiler/compile-app-execution-manifests.nix`
- `nixfied/compiler/compile-runtime-manifest.nix`
- `nixfied/compiler/finalize-model.nix`
- `nixfied/framework/core/mkLauncherMetadata.nix`
- `nixfied/framework/core/mkFlakeOutputs.nix`

Why it matters:

- this is the biggest unification available in the repository today
- without this, every runtime simplification keeps translating between internal
  framework forms

### Candidate 5: delete shell metadata getter and export transport

Primary effect:

- remove `runtime-metadata.nix` as a layer
- stop shell from loading task and workflow runtime via fine-grained kernel
  export calls
- stop summary counters from round-tripping through shell export files

Files under pressure:

- `nixfied/framework/runtime/runtime-metadata.nix`
- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/kernel/src/task.rs`
- `nixfied/framework/runtime/kernel/src/workflow.rs`
- `nixfied/framework/runtime/kernel/src/summary.rs`

Why it matters:

- this removes the strongest remaining echo of the old shell-owned planning era

Constraint:

- the replacement must be a coarse structured handoff, not a cleaner getter API

### Candidate 6: make service contracts compiler-owned

Primary effect:

- move service boundary authority into the compiler
- make the service boundary repository-separable in principle rather than only
  syntactically extractable
- stop importing service modules twice for contract extraction and runtime
  materialization
- stop regenerating service apps and hook env from the same knowledge in
  multiple layers
- split services into:
  - standalone contract data
  - minimal runtime context ABI
  - private service implementation
  - framework-side projections derived only from the contract

Files under pressure:

- `nixfied/compiler/compile-service-surface-catalog.nix`
- `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
- `nixfied/framework/core/mkServiceSetPrograms.nix`
- `nixfied/framework/core/serviceModulePath.nix`
- `nixfied/framework/runtime/helpers/service-api.nix`
- `nixfied/framework/runtime/helpers/app-api.nix`
- `nixfied/framework/runtime/helpers/service-module.nix`
- `nixfied/framework/runtime/helpers/service-observability.nix`
- `nixfied/framework/runtime/services/*`
- `tests/framework/service-*.nix`

Why it matters:

- this is valid only if it deletes duplicate framework knowledge
- if it merely adds a cleaner service platform beside the current one, it is a
  regression

Current evidence from code:

- `service-module.nix` still mixes contract construction with observability
  injection and private exported implementation
- `compile-service-surface-catalog.nix` still imports real service modules
  through synthetic `project` and `slots` context
- `serviceModulePath.nix` hardcodes built-in service locations, which is
  incompatible with repository-separable service ownership
- `mkServiceRuntimeSurfaces.nix` still regenerates apps and hook env and can
  fall back to re-importing service modules
- real services still depend on broad helper/runtime surfaces such as
  `managedServiceLifecycle`, `slotEnvRuntime`, `slots.getSlotInfo`, and
  `slots.getServiceDir`
- `service-api.nix` and `app-api.nix` still tie service contracts to shell/app
  runtime primitive conventions

### Candidate 7: move task dependency execution fully into kernel

Primary effect:

- kernel owns task dependency execution, not just dependency ordering

Files under pressure:

- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/runtime/kernel/src/task.rs`

Why it matters:

- it is the strongest shell-footprint reduction available

Why it is not the first move:

- doing this before compile/runtime data unification would centralize execution
  while keeping duplicate authorities alive

### Candidate 8: prune seam-freezing harness

Primary effect:

- shrink migration guards and ownership taxonomy that exist mostly because seam
  multiplication created governance tax

Files under pressure:

- `tests/framework/contract-migration-guard.nix`
- `tests/framework/framework-test-shards.nix`
- `tests/framework/framework-test-coverage-contract.nix`
- `tests/framework/feature-coverage-validation.nix`

Why it matters:

- some of the current test layer is proving deleted-seam state rather than
  product guarantees

## Necessary Baseline

Before debating target architectures, fix four classification errors:

1. `dispatcher` is not a runtime authority
   It is a wrapper/surfacing veneer and should either collapse into flake
   launchers or remain as a very small app wrapper.
2. `shell runtime metadata` is not a layer
   It is query glue over compiled data and kernel exports.
3. `runtime manifests` are not a separate target abstraction
   They are projections of execution data, not a second authority.
4. `orchestrator-control` is not a separate abstraction
   It duplicates run control already present in orchestrator.

Any option that keeps those as first-class long-term layers is not actually a
deletion-first architecture.

## Service Boundary Readiness

Question `4` has a working answer now:

- service split-readiness is a real requirement, but only as a deletion test
- it is not justified as a packaging goal by itself

Use split-readiness to force these properties:

- a service can declare its public contract without importing framework runtime
  helpers
- the compiler can consume the contract without synthesizing fake `project` or
  `slots` context
- runtime can surface apps, hooks, and service-set wrappers from compiled
  service contracts only
- service implementation depends on one small runtime context ABI instead of the
  current helper bundle
- service lookup is declared ownership, not hardcoded framework path knowledge

Reject the broader version:

- do not build a larger service plugin platform
- do not add a second service SDK next to the current one
- do not treat "movable to another repo" as success if the same framework glue
  still exists under a cleaner name

Practical reading:

- if split-readiness deletes hidden framework-service coupling, it is a valid
  architectural requirement
- if it only makes services nicer to package, service contracts should be
  simplified only as far as they delete duplicated framework glue

## Target Architectures

The realistic choice is not among seven independent cleanup items.

The realistic choice is among:

- one prep step that removes low-regret duplicate authorities
- one new default architecture that unifies compile/runtime data
- two follow-on bets for the remaining large decisions

### Prep Step: Low-Regret Seam Deletion

Bundle:

- candidate `1`
- candidate `2`
- candidate `3`

Core idea:

- delete obvious duplicate authorities first
- do not treat this as the end-state architecture

Benefits:

- cheap deletion
- strengthens later invariants
- reduces noise before the main decision

Limit:

- does not solve multiple compiled execution forms
- does not solve shell metadata transport

### Option A: Legacy Thin Shell Runtime

Bundle:

- the earlier "thin shell runtime" cleanup framing
- delete runtime selection fallbacks
- delete executor synthesis of `serviceSetPrograms`
- merge runtime-control ownership
- delete the shell metadata getter layer
- delete shell summary export sourcing

Re-evaluation:

- this is no longer a sufficient target architecture
- as previously framed, it removes duplicate shell seams but leaves the larger
  compile/runtime representation problem under-specified

Keep only as:

- a description of the old framing
- not the recommended end-state

### Option B: Kernel-Led Execution Core

Bundle:

- `Option D`
- candidate `7`

Core idea:

- after compile/runtime data is singular, move task dependency execution fully
  into the kernel
- shell becomes launch, env, hooks, and process supervision only

Benefits:

- strongest shell-footprint reduction
- most honest realization of "kernel owns execution semantics"

Costs:

- highest implementation risk
- kernel grows operationally as well as semantically

Constraint:

- do not choose this before `Option D`

### Option C: Compiler-Owned Service Contracts

Bundle:

- `Option D`
- candidate `6`

Core idea:

- after compile/runtime data is singular, move service contract authority fully
  into the compiler
- make repository-separable service boundaries possible in principle by
  separating contract, runtime ABI, and private implementation
- runtime surfaces become pure projections of compiled service contracts

Benefits:

- strongest path to deleting service-specific framework glue
- best path to making service boundaries real without synthetic extractability
  proof seams
- exposes and shrinks the hidden coupling now carried by `service-module.nix`,
  `serviceModulePath.nix`, helper bundles, and shell/app runtime conventions

Costs:

- high refactor cost
- only justified if it deletes the current duplicate service ownership
- requires a real runtime-context contract for services instead of the current
  `project` plus `slots` helper seam

Constraint:

- reject this if it grows a larger service meta-framework instead of reducing
  duplicated knowledge
- repository split-readiness is not the product by itself; it is the test that
  the service boundary is actually small and real

### Option D: Canonical Compiled Graph + Single Shell Runtime Authority

Bundle:

- prep step
- candidate `4`
- candidate `5`

Core idea:

- unify execution knowledge at compile time
- hand shell and kernel one coarse canonical compiled execution graph
- collapse shell runtime into one authority for OS-facing concerns only

Layers that collapse hard:

- separate shell runtime metadata layer
- runtime selection fallback seam
- separate runtime manifest family
- dispatcher as an architectural runtime layer
- duplicate runtime-control authority
- shell export transport for metadata and summary counters

Layers that remain:

- typed authoring boundary
- compiler authority
- surface builder
- single shell runtime authority
- kernel authority
- guarantee harness

Benefits:

- deletes the largest amount of duplicated knowledge without yet forcing the
  kernel/task-execution bet
- gives the codebase one canonical execution representation
- is likely mandatory regardless of whether `B` or `C` comes later

Costs:

- compiler refactor cost is real
- forces manifest and runtime data shape changes

Why this is the new default:

- it removes the most duplicated authorities
- it does not depend on a larger kernel to pay off
- it shrinks both runtime and test governance pressure

## Option Comparison

| Option | Best deletion win | Biggest remaining problem | Risk | Recommended order |
| --- | --- | --- | --- | --- |
| `Prep` | removes obvious duplicate authorities | canonical execution data still fragmented | low-medium | first |
| `A` legacy thin shell runtime | historical cleanup framing only | does not fully solve representation duplication | medium | do not use as target |
| `D` canonical compiled graph + single shell runtime authority | removes duplicate compile/runtime knowledge and shell metadata transport | service boundary duplication and kernel task loop still remain | high | first real target |
| `B` kernel-led execution core | deletes most shell semantic planning | kernel growth and task-edge migration complexity | high | after `D` |
| `C` compiler-owned service contracts | deletes duplicate service boundary glue and hidden framework-service coupling | difficult contract/runtime split plus service lookup/runtime ABI redesign | high | after `D`, or parallel after `D` |

## Recommendation

The best current sequencing is:

1. land the prep-step deletions
2. use those deletions to build `Option D`
3. after `D`, choose deliberately between `B` and `C`
4. prune migration-seam test governance after the deleted seams are truly gone

Reason:

- the prep step removes low-regret noise
- `D` is the only option that directly attacks the largest remaining
  duplication: multiple execution representations and shell metadata transport
- after `D`, the remaining choice becomes cleaner:
  - do we want even less shell execution logic
  - or do we want compiler-owned service boundaries

In other words:

- `Prep` is cleanup
- `D` is the real architecture correction
- `B` and `C` are the two follow-on directional bets

## Decision Questions

To choose between the options, answer these explicitly:

1. Do we agree that `selectionIndex`, runtime metadata, manifest narrowing, and
   launcher selection tables should become one compiled execution authority?
   <!-- //WR: yes -->
2. Do we want shell to keep any fine-grained metadata query API at all?
    <!-- //WR: no need form my PoV, but you can investigate necessity here -->
3. Is shell-owned task dependency execution acceptable after the compiled graph
   is unified, or only as an intermediate state?
   <!-- //WR: we need to expose for the user of the framework how to set their task dependencies, with that in mind with a proper well-defined API it could be only an step in the state execution workflow after compiled. But you understand the details better than me here. -->
4. Is service split-readiness a real requirement, or should service contracts be
   simplified only as far as they delete duplicated framework glue?
   Working answer:
   - yes, but only as a deletion test
   - repository split-readiness is justified when it forces a smaller public
     service boundary and exposes hidden framework coupling
   - reject any broader service-platform generalization that adds new framework
     surface without deleting the current one
5. Are we willing to grow kernel scope only when that growth deletes an entire
   shell planning seam?
   <!-- //WR: yes, kernel scope should only grow if it means simplifying the whole code base, increasing reusability and reproducibility/determinisnm while removing shell weak workflows. -->
6. Which current migration-guard tests are still proving product guarantees, and
   which are only freezing temporary refactor boundaries?
   <!-- //WR: I dont think migration-guard are actually testing product guarantees. But you can check .#features and documentation to make sure we dont have feature loss. Also the tests should be a good way to confirm it. -->

## Final Position

The old realistic choice was framed as:

- `A`: thin shell runtime
- `B`: kernel-led execution core
- `C`: extractable service platform

That framing is now incomplete.

The more accurate decision structure is:

- `Prep`: delete low-regret duplicate authorities
- `D`: make the compiled execution graph canonical and collapse shell runtime to
  one real authority
- then choose between:
  - `B`: push task dependency execution further into kernel
  - `C`: push service contracts fully into the compiler and make service
    boundaries repository-separable in principle

The strongest default recommendation is therefore no longer `A` first.

It is:

1. `Prep`
2. `D`
3. then a deliberate choice between `B` and `C`

That is the smallest path visible in the current repository that reduces
authorities instead of better-organizing them.
