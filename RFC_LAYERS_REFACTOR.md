# RFC: Layers Refactor

Status: draft

## Purpose

This RFC explains why the kernel rewrite improved semantic ownership but did not materially reduce framework size, and proposes the next architectural step: collapse runtime layers until there is a single framework-owned execution authority.

This document is about deleting generic framework code, not reorganizing it.

## Hard Constraints

These constraints are part of the refactor and are not optional.

1. Forward-only break change.

- do not preserve backward compatibility for replaced internal seams
- do not add compatibility shims, compatibility flags, dual-write paths, or old/new adapters
- when a new layer boundary lands, remove the replaced boundary rather than carrying both

2. Commits are the unit of rollout.

- do the refactor as a sequence of small reviewable commits
- each commit should remove a real layer seam, not just prepare for a later cleanup
- every commit should leave the repository in a working and testable state

3. Full cutoff, full cleanup.

- all break changes needed for simplification are available within the internal architecture
- obsolete runtime transports, helper APIs, manifest forms, and control paths should be deleted, not deprecated
- cleanup is part of each commit, not a later backlog item

## Refactor Policy

### What May Break

- internal runtime transports
- internal manifest shapes
- internal shell helper APIs
- framework-owned machine JSON shapes
- kernel subcommand structure
- executor and orchestrator internal seams

### What Must Not Regress

- framework behavior as a user-facing system
- determinism
- service selection and isolation
- run lifecycle and stop controls
- registry and summary behavior
- public flake app availability through `nix run .#<cmd>` unless a separate RFC explicitly chooses otherwise

### No-Compatibility Rule

When a new runtime seam lands:

- remove the old seam in the same commit if practical
- otherwise make the next commit its deletion, not a future cleanup item

The refactor should not spend time on:

- shims between old and new manifests
- old and new workflow driver protocols living together
- compatibility wrappers around export transport
- temporary shell APIs kept alive "just in case"

## Summary

The kernel rewrite was correct.

It removed framework-owned `jq`, CUE, and Python semantics from runtime paths and moved validation, JSON semantics, workflow state, registry logic, summary logic, and probe behavior into a typed Rust binary.

However, the overall framework still carries too much generic execution machinery for the job it performs.

The reason is structural:

- Nix still generates runtime metadata in multiple forms.
- Bash still owns a large amount of framework control flow.
- Rust owns framework semantics, but mostly as a stepwise coprocessor behind shell loops.

In other words, the refactor solved semantic drift, but not layer multiplication.

The next move should be a layers refactor:

- keep Nix as compile-time authority
- keep shell only as a thin adapter at the edges
- promote the kernel from semantic helper to runtime execution authority
- define service APIs and boundaries strongly enough that service implementations could move to separate repositories without dragging generic framework internals with them
- delete generated shell metadata APIs, stepwise kernel RPC loops, and temp-file export transport where possible

## Problem Statement

Nixfied now behaves less like a "task runner" and more like a local execution platform with:

- compile-time graph selection
- service capability closure
- workflow scheduling
- foreground and background run lifecycle
- registry and replay
- validated summaries
- ephemeral workspace isolation
- stable machine contracts
- service-scoped runtime env export

Those are real features, and some complexity is therefore justified.

But the generic runtime is still larger than it should be because the same framework knowledge is represented in too many layers.

The problem is not that the kernel is too big in isolation.

The problem is that runtime behavior is still spread across:

- compile-time Nix manifests and generated shell code
- dispatcher, orchestrator, and executor shell layers
- helper libraries that expose framework metadata through shell functions
- a kernel CLI that is called repeatedly for small semantic steps

There is a related boundary problem as well:

- service integrations are not yet tested primarily by whether they could stand behind a small public API and move out of the repository cleanly
- framework and service concerns are still close enough that some coupling is hidden by co-location rather than prevented by design

That composition keeps the codebase large even after semantic migration to Rust.

## Observed Code Shape

Measured locally on March 26, 2026:

- `nixfied/framework/runtime`: about 28.4k LOC
- `nixfied/framework/runtime/kernel/src`: about 6.9k LOC
- `nixfied/framework/runtime/helpers`: about 7.6k LOC
- `nixfied/framework/runtime/services`: about 5.3k LOC
- `tests/framework`: about 17.1k LOC across 148 test files

Two observations matter:

1. The generic runtime is much larger than the service implementations.
2. The kernel is only one part of the runtime cost.

That means the main issue is not "services are complicated".

It is "the framework platform around services is too layered".

## Current Layer Model

As documented in `docs/ARCHITECTURE.md` and `docs/DETAILED.md`, the public runtime path is effectively:

- public launcher
- selector-aware app resolution
- dispatcher
- orchestrator
- executor
- kernel

Important current examples:

- `nixfied/framework/runtime/workflow-modes.nix` generates a shell query API for task and workflow metadata.
- `nixfied/framework/runtime/executor.nix` contains large serial and parallel workflow drivers in shell.
- `nixfied/framework/runtime/orchestrator.nix` owns run lifecycle, stop controls, janitor logic, and process supervision in shell.
- `nixfied/framework/runtime/helpers/shell-contract.nix` still uses temp export files and `source` to receive kernel results.
- `nixfied/framework/runtime/kernel/src/main.rs` still acts as a many-subcommand command bus for multiple runtime domains.

The architecture is correct in direction, but it is still paying for all of these at once:

- generated metadata API
- shell orchestration library
- kernel semantic CLI
- validated file transport between them

## Diagnosis

### 1. The kernel solved ownership, not layering

The kernel correctly took over framework semantics such as:

- validation
- run-record transitions
- registry append and replay logic
- summary composition
- workflow scheduler state
- runtime-event policy
- probe execution

But shell still drives many of those semantics as a remote control.

The clearest example is workflow execution:

- executor calls `workflow serial-init`
- then loops on `workflow serial-next`
- executes a task in shell
- then calls `workflow serial-transition`

The parallel path does the same with more bookkeeping.

This is better than shell owning the logic directly, but it is still a split control plane.

### 2. Runtime metadata is still emitted as shell API instead of runtime data

`nixfied/framework/runtime/workflow-modes.nix` generates dozens of shell functions that answer questions like:

- does this workflow exist
- what mode does it use
- is it parallel
- what are its selected services
- what hooks does a task have
- what runtime plan shell snippet belongs to a task

That means compile-time knowledge is materialized as executable shell code rather than as one runtime data structure loaded once.

This is a major accidental-complexity source.

### 3. The runtime still uses fine-grained kernel RPC

The framework repeatedly crosses process boundaries for small semantic operations:

- workflow init
- workflow next
- workflow transition
- event detail render
- run-record read fields
- service-policy export generation
- validate input and emit shell exports

This is correct semantically, but expensive architecturally.

The shell ends up full of:

- temp files
- `mktemp`
- repeated kernel invocations
- repeated cleanup code
- string protocol parsing

### 4. Too many transport forms exist between layers

The runtime currently moves framework information through many channels:

- generated shell functions
- environment variables
- shell export files
- JSON plan files
- JSON state files
- tab or unit-separator delimited stdout protocols

This makes each layer smaller in responsibility than the total system suggests, but larger in glue than a simpler architecture would require.

### 5. The framework is paying a high "guarantee tax"

Nixfied intentionally provides:

- deterministic selection
- validated machine outputs
- workspace and slot isolation
- stable event and summary contracts
- run inventory and stop controls
- service scoping

Those are good guarantees.

But the codebase currently pays for them multiple times:

- once in compile-time model generation
- again in shell wrappers and helpers
- again in kernel semantics
- again in test guards that freeze the seams

The result is not just "a lot of code".

It is a lot of repeated framework control surface.

### 6. Service boundaries are not yet a hard architectural test

Another useful way to frame the problem is:

- if a service integration could not be moved into its own repository behind a small public contract
- then the framework and service are still coupled too deeply

That does not mean every service should immediately become a separate repository.

It means extractability should be used as a design test.

If the framework needs to know too much about:

- service config internals
- service lifecycle internals
- service-specific runtime env structure
- service-specific helper implementation details

then the generic runtime is still carrying service coupling that should instead be hidden behind a service API.

This matters for the layers refactor because unclear service boundaries force more framework-owned glue:

- more generated runtime metadata
- more helper libraries
- more service-aware branching in generic runtime code
- more difficulty collapsing dispatcher, orchestrator, executor, and kernel responsibilities cleanly

## Decision

The framework should refactor around a single runtime execution authority.

The target architecture is:

- Nix owns model compilation, service selection, and generation of immutable runtime assets.
- The kernel owns framework runtime semantics and workflow/run execution state.
- Shell remains only as a thin adapter for process launch edges, service hooks, and environment bootstrapping.

This implies a deliberate collapse of generic runtime layers.

The framework should stop treating Bash as a host for framework metadata APIs and state-machine driving logic.

It should also treat service extractability as a boundary check:

- a service integration should present a small public surface to the framework
- the framework should depend on that surface, not on service implementation layout
- a future service repository split should be mechanically difficult only in packaging and release terms, not because framework internals are entangled with service code

## Target Architecture

### Layer 1: Nix Compile-Time Authority

Nix should continue to own:

- modules
- compiler passes
- graph pruning and selection
- help and app surface generation
- immutable runtime manifests
- contract bundles

But Nix should emit fewer runtime artifacts with broader scope.

Instead of many narrow runtime files and shell case tables, it should prefer one compiled runtime manifest per selected surface, or a very small number of stable manifests.

Nix should also compile against explicit service descriptors rather than broad service implementation knowledge wherever possible.

### Layer 2: Kernel Runtime Authority

The kernel should own:

- workflow driving
- run lifecycle state transitions
- summary and registry behavior
- probe execution
- contract validation
- machine-facing runtime semantics

The important change is not "move every line into Rust".

The important change is:

- stop using the kernel as a small-step semantic RPC server
- start using it as the coarse-grained framework runtime driver

Public shell commands may remain, but they should become thin wrappers over kernel-owned operations rather than large control-flow programs themselves.

### Layer 3: Thin Shell Adapters

Shell should remain only for:

- launching leaf commands
- sourcing environment bootstraps that are inherently shell-native
- signal forwarding at process edges where shell remains the practical tool
- service lifecycle scripts that are naturally command-oriented

Shell should not own:

- task and workflow metadata lookup APIs
- workflow state-machine loops
- semantic JSON transport
- framework run-record or registry mutation logic
- framework selection logic already known at compile time
- service-specific framework knowledge that belongs behind a service API

## Core Refactor Principles

### 1. Replace shell query APIs with manifests

`workflow-modes.nix` should stop generating large shell function tables for framework metadata.

Instead, compile a single runtime manifest that contains:

- workflow descriptors
- task descriptors
- hook descriptors
- service selection closures
- runtime plan references
- phase and service-set operations
- contract references needed at runtime

Shell should load the manifest once if it still needs it.

Preferably, the kernel should load it directly and shell should not query it at all.

### 2. Replace stepwise workflow RPC with coarse-grained workflow execution

The serial and parallel workflow protocols should stop being:

- `init`
- `next`
- execute in shell
- `transition`
- repeat

That protocol duplicates control flow across Rust and shell.

The framework should instead expose a coarse-grained workflow runtime operation whose interface matches the real abstraction:

- run this workflow
- emit events and summaries
- return final status

If shell still launches leaf task commands, it should do so under a kernel-owned execution plan, not by driving the scheduler itself.

### 3. Replace export temp files with direct transport

`validate-input` and similar operations should stop writing temporary export files that shell later sources.

Preferred options:

- emit shell-safe exports to stdout
- emit JSON and let a single thin adapter translate it

The current pattern is correct but too glue-heavy.

### 4. Collapse runtime manifest count

The executor currently emits multiple runtime files such as:

- task dependency plan
- workflow scheduler plan
- workflow summary plan
- service-name lists
- validation bundles

These should be collapsed where possible into a stable runtime manifest family with clear ownership and fewer handoff seams.

### 5. Keep public command names, narrow internal seams

The user-facing contract should stay stable:

- `nix run .#<cmd>`
- `run-task`
- `run-workflow`
- `runs`
- `stop-run`

But internally, dispatcher, orchestrator, and executor should be allowed to collapse into much thinner layers or even into naming wrappers over a smaller runtime core.

### 6. Use service extractability as a boundary test

For each service integration, ask:

- what is the public configuration contract
- what is the public lifecycle contract
- what generated operation surface does the framework consume
- what implementation details remain private to the service package

If those answers are unclear, the framework is still carrying service coupling that will obstruct layer collapse.

The goal is not "split every service now".

The goal is "make every service split-ready by API shape".

## Proposed End State

The desired conceptual pipeline is:

- public launcher
- selected runtime manifest
- kernel runtime
- leaf task or service command

Not:

- public launcher
- selector-aware launcher logic
- dispatcher
- orchestrator
- executor
- helper library lattice
- kernel semantic CLI
- multiple state and export temp files

The exact number of binaries is less important than the number of framework-owned control layers.

## Commit-Based Rollout

This refactor should be executed as a forward-only commit sequence.

The sequence below is ordered to delete layer seams early and avoid building a second temporary architecture.

### Commit 1: Introduce A Single Runtime Manifest Family

Suggested commit message:

- `replace shell metadata tables with runtime manifest`

Goal:

- remove generated shell metadata tables as the primary runtime API

Actions:

- introduce an explicit runtime manifest family for task, workflow, hook, service-selection, and contract metadata
- update runtime consumers to read the manifest rather than generated shell case tables
- delete replaced query surfaces instead of dual-serving both forms

Commit must remove:

- shell metadata APIs that are superseded by the manifest
- compatibility loaders between case-table output and manifest input

Expected result:

- major reduction in `workflow-modes.nix`
- fewer shell helper entrypoints
- clearer separation between compile-time data and runtime execution

### Commit 2: Cut Over Workflow Driving To Coarse-Grained Kernel Execution

Suggested commit message:

- `move workflow driving into kernel runtime`

Goal:

- delete shell-owned workflow scheduler loops

Actions:

- replace stepwise `serial-init/next/transition` and `parallel-init/next/transition` protocols with coarse-grained kernel runtime operations
- move scheduler driving and transition ownership behind the kernel runtime boundary
- remove temp state handoff that exists only for shell-driven scheduling

Commit must remove:

- executor loops that drive workflow scheduling one state transition at a time
- obsolete workflow state temp-file machinery
- old workflow RPC subcommands if no longer needed

Expected result:

- major shrink in `executor.nix`
- fewer process crossings per workflow run
- serial and parallel orchestration logic owned in one place

### Commit 3: Delete Export File Transport

Suggested commit message:

- `replace kernel export files with direct transport`

Goal:

- remove temp export files as the default framework runtime boundary

Actions:

- cut validation and policy exports over to stdout or structured JSON transport
- update shell adapters to consume the new transport directly
- keep quoting and deterministic behavior explicit

Commit must remove:

- default temp export-file transport in framework-owned validation and policy paths
- helper cleanup code that exists only for export temp files
- compatibility wrappers for old export loading

Expected result:

- fewer `mktemp` sites
- less shell cleanup code
- narrower kernel-to-shell boundary

### Commit 4: Collapse Dispatcher, Orchestrator, And Executor Internals

Suggested commit message:

- `collapse runtime control layers`

Goal:

- delete generic shell control layers that no longer justify their existence

Actions:

- move remaining framework-owned run lifecycle logic behind a smaller runtime authority
- keep public command surfaces, but allow internal dispatcher, orchestrator, and executor roles to collapse
- leave shell only at the practical process edges

Commit must remove:

- internal layer splits that survive only for historical reasons
- helper glue that exists only to shuttle framework control between shell layers
- obsolete manifest or transport seams retained from older runtime splits

Expected result:

- smaller generic runtime footprint
- clearer ownership of process control versus framework semantics
- fewer internal seams to freeze in tests

### Commit 5: Harden Service Public APIs And Extractable Boundaries

Suggested commit message:

- `separate service contracts from service internals`

Goal:

- make service integrations split-ready by API shape

Actions:

- define the minimal public contract the framework consumes for each service integration
- separate service descriptors and generated operation surfaces from service implementation layout
- remove framework dependencies on service-internal helpers and structure where possible
- verify that services could move to separate repositories without redesigning the generic runtime

Commit must remove:

- generic runtime dependencies on service-internal layout
- hidden coupling preserved only by repository co-location
- framework knowledge that belongs behind service APIs

Expected result:

- clearer framework versus service ownership
- less hidden coupling by co-location
- easier future repository splits or independent service packaging

### Commit 6: Add Regression Guards For Layer Collapse

Suggested commit message:

- `add guards for collapsed runtime seams`

Goal:

- freeze the new architecture after the old seams are gone

Actions:

- add targeted guards for deleted shell metadata APIs
- add guards against stepwise shell-driven workflow scheduling
- add guards against temp export-file transport reappearing as the default
- add guards against generic runtime code depending on service-internal layout

Commit must remove:

- any remaining test expectations that assume deleted seams still exist

Expected result:

- the repository protects the new layer model rather than the old one
- future changes are pushed toward the simplified architecture

## What This RFC Is Not

This RFC does not propose:

- moving semantics back to Nix or shell
- reintroducing `jq`
- reintroducing Python helpers
- splitting the kernel into many crates as a first move
- deleting shell entirely
- deleting service lifecycle scripts that are naturally shell commands

This RFC also does not treat line count as the only metric.

The point is not "make the number smaller" in isolation.

The point is "delete framework-owned control layers that no longer buy enough value".

## Acceptance Criteria

The refactor should be considered successful only if it deletes major generic runtime seams, not merely moves them.

Minimum success criteria:

- no backward compatibility shims remain for replaced runtime seams
- framework-owned task and workflow metadata is no longer exposed primarily as generated shell function tables
- executor no longer drives workflow scheduling through repeated `init/next/transition` kernel calls
- framework-owned validation and policy export paths no longer depend on temp export files by default
- generic runtime code materially shrinks rather than being redistributed across new files
- service integrations expose clearer public APIs and require less framework knowledge of their internal layout
- public command availability through `nix run .#<cmd>` remains stable
- service selection, isolation, registry, summary, and stop-control behavior remain intact

Secondary success criteria:

- fewer runtime manifest types
- fewer `mktemp` sites in framework-owned runtime code
- fewer kernel invocations per workflow run
- simpler test guards because there are fewer internal seams to freeze

## Repository Guard Direction

The repository should eventually grow explicit guards against regression in this area.

Examples:

- a guard that fails if framework metadata query APIs reappear as large generated shell case tables
- a guard that fails if workflow scheduling reverts to stepwise shell-driven kernel RPC
- a guard that fails if temp export file transport becomes the default framework runtime boundary again
- a guard that fails if generic runtime code starts depending on service-internal implementation layout rather than declared service surfaces

These should be added as the refactor lands, not before.

## Open Questions

1. How much process supervision should remain in shell versus move into the kernel runtime?

This RFC does not require an immediate answer, but it assumes the current shell-heavy orchestration split is too expensive.

2. Should dispatcher, orchestrator, and executor remain distinct user-facing names even if they collapse internally?

Probably yes.

Their public names may still be useful even if their implementation becomes much thinner.

3. Should the kernel consume one runtime manifest or a small manifest family?

The answer should optimize for fewer seams, not ideological purity.

One manifest is preferable if it does not become unstable or too broad.

## Final Position

The kernel rewrite should be treated as the end of semantic migration, not the end of runtime simplification work.

It proved that nixfied can move framework meaning out of shell.

The next refactor should prove that nixfied can also move framework control flow out of shell where shell no longer adds enough value.

If this RFC is followed, the framework should become smaller not because features are removed, but because the same feature stops existing in three places.
