# RFC: Layers Refactor

Status: draft

## Purpose

This RFC explains why the kernel rewrite improved semantic ownership but did not materially reduce framework size, and proposes the next architectural step: collapse runtime layers until there is a single framework-owned execution authority.

This document is about deleting generic framework code, not reorganizing it.

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

## Decision

The framework should refactor around a single runtime execution authority.

The target architecture is:

- Nix owns model compilation, service selection, and generation of immutable runtime assets.
- The kernel owns framework runtime semantics and workflow/run execution state.
- Shell remains only as a thin adapter for process launch edges, service hooks, and environment bootstrapping.

This implies a deliberate collapse of generic runtime layers.

The framework should stop treating Bash as a host for framework metadata APIs and state-machine driving logic.

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

## Migration Strategy

This refactor should be staged.

### Stage 1: Delete Shell Metadata APIs

Goal:

- remove `workflow-modes.nix` as a large generated shell query surface

Actions:

- introduce a runtime manifest with task, workflow, hook, and service-selection data
- update runtime code to consume that manifest instead of shell case tables
- preserve user-facing behavior

Expected result:

- a large reduction in generated shell helper logic
- simpler executor and launcher paths

### Stage 2: Delete Stepwise Workflow RPC

Goal:

- remove shell-owned serial and parallel scheduler loops

Actions:

- move scheduler driving into the kernel runtime
- replace `serial-init/next/transition` and `parallel-init/next/transition` with coarse-grained operations
- reduce state temp-file handoff

Expected result:

- major shrink in `executor.nix`
- fewer process crossings
- less duplicated serial and parallel orchestration code

### Stage 3: Delete Export File Transport

Goal:

- stop using framework temp export files as the default kernel-to-shell transport

Actions:

- switch validation and policy exports to stdout or JSON transport
- keep shell quoting rules strict and deterministic

Expected result:

- less cleanup code
- fewer `mktemp` sites
- smaller shell helper layer

### Stage 4: Collapse Dispatcher, Orchestrator, And Executor Responsibilities

Goal:

- keep public surfaces, shrink internal framework layers

Actions:

- identify which orchestration responsibilities truly need separate shell layers
- move framework-owned run lifecycle logic behind a smaller runtime authority
- leave shell only where it is the practical leaf tool

Expected result:

- smaller generic runtime footprint
- clearer ownership
- fewer framework cross-layer seams to test and guard

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

- framework-owned task and workflow metadata is no longer exposed primarily as generated shell function tables
- executor no longer drives workflow scheduling through repeated `init/next/transition` kernel calls
- framework-owned validation and policy export paths no longer depend on temp export files by default
- generic runtime code materially shrinks rather than being redistributed across new files
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
