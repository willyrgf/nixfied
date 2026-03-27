# RFC: Layers Refactor

Status: discussion draft

Date: 2026-03-27

Supersedes: the earlier draft of this file

## Purpose

This document rewrites the previous RFC as a forensic architecture record.

The earlier draft became stale while the refactor was landing. It still described
seams that have already been deleted, and it did not clearly separate:

- what was truly removed
- what merely moved
- what was added to pay for stronger guarantees
- what complexity is still accidental

This rewrite is intentionally blunt.

Its job is to explain why a refactor whose stated goal was to delete generic
framework fat mostly relocated complexity instead of cashing out into major
deletion, and to define the problem space for the next round of solutions.

This is not a generic design critique.

It is a repository-specific diagnosis based on:

- `RFC_LAYERS_REFACTOR.md`
- `docs/ARCHITECTURE.md`
- `docs/DETAILED.md`
- the current codebase on 2026-03-27
- normalized LOC measurements
- git history through the kernelization and follow-on cleanup period

## Executive Verdict

- The refactor improved semantic ownership, but it did not materially reduce
  framework fat.
- The narrow comparison from Snapshot B to current is valid, but it hides the
  bigger story because both points are already post-kernel and post-manifest.
- From the last pre-kernel baseline to current, normalized code grew from about
  `53.5k` to `62.5k`.
- One important seam truly died: the shell-owned stepwise workflow driver and
  its old `workflow-modes.nix` planning surface.
- Most of the deleted responsibility reappeared as compiled runtime metadata, a
  shell metadata query layer, broader kernel query/load commands, and more test
  machinery.
- The runtime path still effectively remains:
  `launcher -> selector -> dispatcher -> orchestrator -> executor -> kernel`.
- Shell is no longer the scheduler of record, but it still owns too much
  control flow: task launch edges, process supervision, run inventory, stop
  controls, temp-file choreography, and export sourcing.
- Some growth is justified by real guarantees: typed validation, registry
  semantics, run-record handling, summary composition, selected execution
  manifests, and tighter test contracts.
- The main accidental cost is duplicated knowledge across compile-time Nix,
  shell adapters, Rust query/load APIs, runtime transports, and contract tests.
- If the goal is still deletion rather than better-organized machinery, the
  next round must remove whole seams, not add a cleaner version alongside the
  old one.

## Investigation Basis

### Primary source files

- `docs/ARCHITECTURE.md`
- `docs/DETAILED.md`
- `nixfied/compiler/*`
- `nixfied/modules/*`
- `nixfied/framework/core/*`
- `nixfied/framework/runtime/*`
- `nixfied/framework/runtime/helpers/*`
- `nixfied/framework/runtime/services/*`
- `nixfied/framework/runtime/kernel/src/*`
- `nixfied/project/*`
- `tests/framework/*`

### Git periods inspected

- `2b4e92d`:
  last useful pre-kernel baseline used in this investigation
- `7a9ca5d`:
  large framework reorganization commit
- `89a35ba`:
  pre-RFC comparison point used for current-vs-then subsystem deltas
- `13140b7`:
  serial workflow kernelization
- `148f181`:
  parallel workflow kernelization
- `8b4bbd4`:
  RFC-branch layer-collapse commit
- `bdfc68a`:
  current branch tip at time of investigation

### Measurement method

All whole-repo LOC comparisons in this RFC use:

```bash
nix run nixpkgs#scc -- --include-ext nix,rs,md,json,txt,toml .
```

Run from repository root.

That matters because:

- root-level measurement respects `.gitignore`
- this excludes ignored generated output such as
  `nixfied/framework/runtime/kernel/target/`
- it keeps the language set stable
- it matches the user-provided snapshots closely enough to treat them as the
  same measurement family

For subsystem measurements, git-tracked files were used to avoid local ignored
artifacts distorting per-directory counts.

## Apples-To-Apples Measurement Verdict

The Snapshot B to current comparison is valid enough to use.

The exact code counts differ by a few dozen lines depending on `scc` parser
version, but the file counts and total lines match the supplied snapshots.

### Normalized whole-repo baselines

| Baseline | Commit / point | Files | Code | Notes |
| --- | --- | ---: | ---: | --- |
| Pre-kernel | `2b4e92d` | 340 | 53,451 | before `nixfied-kernel` existed as the main runtime authority |
| Snapshot B / pre-RFC point | `89a35ba` | 380 | 60,817 | effectively matches the supplied Snapshot B |
| Current | `bdfc68a` | 388 | 62,473 | current working comparison point |

### What the narrow comparison says

From Snapshot B to current:

- files: `380 -> 388`
- code: `60,817 -> 62,473`
- net growth: about `+1,656` code

That is real growth, not tool noise.

### What the wider comparison says

From the last pre-kernel baseline to current:

- files: `340 -> 388`
- code: `53,451 -> 62,473`
- net growth: about `+9,022` code

That wider comparison is the more honest answer to "did the refactor actually
delete framework fat?"

The answer is no.

## What Actually Happened

### Short version

The refactor improved who owns semantics.

It did not reduce how many layers know about runtime behavior.

The codebase got more coherent locally, but the whole stack still carries:

- compile-time metadata materialization
- shell dispatch and supervision layers
- kernel semantic and query APIs
- multiple transport forms between them
- a larger test and migration-guard layer freezing the new seams

### Material timeline

#### 1. Reorganization before deletion

The March 12 commit `7a9ca5d` (`finish framework reorganization`) moved a large
amount of code into the current `nixfied/framework/*` layout.

This improved shape and naming, but it was mostly code motion, not
end-to-end seam deletion.

#### 2. Compile-time and manifest surface expansion

The March 20-22 period added or expanded machinery around:

- generated service hook surfaces
- split cheap vs heavy flake surfaces
- narrowed app execution manifests
- grouped service-set runtime surfaces

Representative commits:

- `92ac84e`
- `5bc4bcb`
- `c961afc`
- `12e0d9a`

This was not meaningless growth.

It bought stronger compile-time scoping and narrower execution surfaces, but it
also increased the amount of precomputed runtime structure the framework had to
carry.

#### 3. Kernelization of workflow driving

The March 24-25 period moved workflow scheduling into Rust.

Representative commits:

- `13140b7`
- `148f181`

This was the most important real improvement in the refactor.

Shell stopped owning serial and parallel scheduler state machines directly.

#### 4. RFC-branch cleanup and hardening

The March 27 RFC branch deleted some replaced seams and then added a layer of
regression guards, fixtures, metadata, and shard management to freeze the new
shape.

Representative commit:

- `8b4bbd4`

This phase did delete real seams, but it also added guarantee tax.

## What Truly Disappeared

These are real deletions, not just renamed responsibilities.

### 1. `workflow-modes.nix` died

`nixfied/framework/runtime/workflow-modes.nix` no longer exists.

That old file used to own:

- task runtime merging
- hook runtime merging
- shell rendering of runtime plans
- task and workflow metadata query functions

This deletion is real.

### 2. Stepwise workflow RPC died

The old kernel workflow family exposed:

- `serial-init`
- `serial-step`
- `parallel-init`
- `parallel-step`

Those commands are gone.

The current kernel workflow family is:

- `resolve-mode`
- `load-runtime`
- `run`

This is a real simplification inside the scheduler boundary.

### 3. Several old helper and contract seams died

The migration guard now asserts the absence of prior helper seams such as:

- `nixfied/framework/core/machine-output-validate.py`
- `nixfied/framework/core/introspection-query.py`
- `nixfied/framework/contracts/mkValidator.nix`
- `nixfied/framework/contracts/render-cue.nix`
- `nixfied/framework/runtime/helpers/run-registry.nix`
- `nixfied/framework/runtime/services/public-surface.nix`
- `nixfied/modules/apps.nix`
- `tests/framework/snapshots/contracts/example.cue`

Those deletions matter.

They show the project did remove legacy auxiliary authorities such as Python,
`jq`, and CUE-based framework semantics from runtime-critical paths.

### 4. Some manifest kinds really disappeared

Compared to the pre-RFC point, the current runtime no longer carries manifest or
state kinds such as:

- `nixfied-task-dependency-plan`
- `nixfied-workflow-scheduler-plan`
- `nixfied-workflow-serial-state`
- `nixfied-workflow-parallel-state`
- `nixfied-workflow-serial-skipped`
- `nixfied-workflow-parallel-skipped`

That is a real reduction in certain runtime transport forms.

## What Was Added

### 1. Compiled runtime metadata

The deleted `workflow-modes.nix` responsibilities did not vanish.

They were reintroduced in compiled form via:

- `nixfied/compiler/compile-runtime-metadata.nix`

This file now owns the merged task runtime, hook runtime, and shell-plan
rendering logic that used to live in the old workflow modes layer.

### 2. A shell metadata query API backed by the kernel

The current runtime still exposes a framework metadata API to shell through:

- `nixfied/framework/runtime/runtime-metadata.nix`

This layer caches task and workflow runtime state in shell variables and loads
them by calling:

- `nixfied-kernel task load-runtime`
- `nixfied-kernel task load-hook`
- `nixfied-kernel workflow load-runtime`

So the old generated shell query API died, but a new shell query API replaced
it.

### 3. A broader kernel metadata/query surface

The task family widened substantially.

Current task leaf operations:

- `execution-order`
- `exists`
- `workflow-ref`
- `validate-args`
- `render-help`
- `load-runtime`
- `load-hook`

This is a better authority boundary than the old shell-only logic, but it is
still more kernel API surface than "kernel is just the runtime executor".

### 4. More tests, more metadata, more ownership machinery

`tests/framework` grew because the refactor added:

- migration guards
- runtime manifest fixtures
- kernel-native proofs
- test ownership metadata
- shard validation
- service extractability proofs

This bought stronger guarantees, but it is still real code that the repository
must carry.

## What Mostly Moved Instead Of Dying

### Runtime-plan generation

Three representative functions moved almost directly from the deleted
`workflow-modes.nix` into `compile-runtime-metadata.nix`:

- `mergeTaskRuntimeWithRunnerPackage`
- `mergeHookRuntime`
- `renderRuntimePlanShell`

This is the cleanest example of relocation rather than simplification.

The ownership moved.

The responsibility did not disappear.

### Workflow driving

The kernel now owns scheduler transitions.

But shell still wraps that authority in:

- dispatcher entrypoints
- orchestrator process-mode handling
- executor task and service-set adapters
- temp-file creation for skipped-service and summary flows

The scheduler seam collapsed.

The surrounding shell control plane did not.

### Runtime selection

Selection knowledge still exists in more than one place:

- compiled selection data
- runtime imports of `compile-selection-index.nix`
- selected execution manifests
- workflow mode resolution helpers

The model is cleaner than before, but not yet singular.

## Current Architecture, Not The Idealized One

### Public runtime path

The docs currently describe the runtime path as:

- public flake launcher
- selector-aware pure app selection
- scoped dispatcher
- orchestrator
- executor
- coarse kernel runtime

That description is directionally true.

It is not the same as a collapsed runtime stack.

### Current runtime layers that still matter

1. Public launcher layer
2. Selector and manifest-scoping layer
3. Dispatcher shell apps
4. Orchestrator shell logic
5. Executor shell logic
6. Kernel runtime authority

That is still six meaningful layers on the hot path.

### Shell runtime footprint

The current shell-heavy runtime control files are still large:

| File | LOC |
| --- | ---: |
| `nixfied/framework/runtime/dispatcher.nix` | 316 |
| `nixfied/framework/runtime/orchestrator.nix` | 1,090 |
| `nixfied/framework/runtime/orchestrator-runtime.nix` | 409 |
| `nixfied/framework/runtime/orchestrator-control.nix` | 385 |
| `nixfied/framework/runtime/executor.nix` | 1,793 |
| `nixfied/framework/runtime/executor-runtime.nix` | 455 |
| `nixfied/framework/runtime/runtime-metadata.nix` | 322 |
| subtotal | 4,770 |
| `nixfied/framework/runtime/env-sandbox.nix` | 918 |
| subtotal including env sandbox | 5,688 |

That is not "shell as a tiny adapter only".

### Public surface count did not shrink

Public flake app count remained stable across the inspected pre-RFC and current
points:

- `38` apps before
- `38` apps now

This means most new code bought internal restructuring and guarantees, not a
larger user-facing command surface.

### Kernel command surface

Current kernel surface, counted as user-callable leaf operations:

- direct commands:
  `validate-payload`, `validate-artifact`, `validate-input`,
  `validate-scalar`, `validate-exit`, `help`
- family leaf operations:
  `run-id envelope`
- `event-detail render`
- `event-state derive`
- `service-policy runtime-event`
- `service-policy start-service`
- `service-policy fixture-keep-running`
- `run-record create`
- `run-record read`
- `run-record transition`
- `task execution-order`
- `task exists`
- `task workflow-ref`
- `task validate-args`
- `task render-help`
- `task load-runtime`
- `task load-hook`
- `workflow resolve-mode`
- `workflow load-runtime`
- `workflow run`
- `registry append`
- `registry replay`
- `registry terminal`
- `registry runtime-status`
- `summary write`
- `summary compose`
- `summary collect-steps`
- `summary render-human`
- `adapter decode`
- `probe evaluate`
- `probe jsonrpc`
- `machine-output run`

That is `34` concrete leaf operations across `12` command families plus the
direct validation/help commands.

This is not itself a bug.

But it is evidence that the kernel became both:

- the correct semantic authority
- a larger metadata and protocol surface than the original thesis implied

### Runtime transport forms

The current runtime still moves information across at least these forms:

1. JSON files and JSON manifests
2. shell export text rendered by the kernel
3. environment variables
4. line files and CSV-style selected-service lists
5. NDJSON event streams

The old workflow serial and parallel state files are gone, but the transport
picture is still wider than a truly collapsed stack would want.

### Manifest and metadata forms

Current notable manifest or metadata families include:

- `nixfied-app-execution-eval`
- `nixfied-app-runtime`
- `nixfied-execution-manifest`
- `nixfied-machine-output-plan`
- `nixfied-probe-execution-plan`
- `nixfied-runtime-manifest-catalog`
- `nixfied-runtime-metadata`
- `nixfied-validation-ir`
- `nixfied-workflow-summary-plan`

That is `9` relevant manifest or metadata forms in the current runtime path.

The pre-RFC shape had `14` in the equivalent family if you include the now
deleted workflow and task planning/state forms.

So manifest count did shrink.

It just did not shrink enough to offset the other added machinery.

## Responsibility Matrix

This is the current ownership map for the major runtime concerns.

| Concern | Nix role | Shell role | Rust role | Test / contract role | Duplication or glue introduced |
| --- | --- | --- | --- | --- | --- |
| Workflow driving | compile workflow runtime, phase tasks, selected services, fail-fast, max-workers | parse CLI, prepare temp inputs, invoke adapters, call kernel run | own scheduler transitions and workflow event appends | workflow smokes, executor contract, migration guard | shell still owns launch edges around kernel authority |
| Task execution | compile task runtime, hooks, help, deps, validation plans | run hooks, source export files, invoke runner commands, manage retries | compute execution order, validate args, load runtime and hook exports | task-hook tests, executor contract | same task knowledge exists in compile metadata, shell, and kernel query APIs |
| Run lifecycle | compile runtime hash and artifact contract bundle | process mode, pid and pgid handling, run inventory, stop controls | run-id envelope, run-record operations | run-id and run-record tests | shell owns live-process truth while Rust owns record truth |
| Selection and scoping | compile selection index and narrowed execution manifests | launcher resolution and fallback selection imports | workflow mode resolution, existence checks | manifest and selection contracts | selection authority is still re-materialized at runtime |
| Validation | compile validation IR | help fast paths and command framing | validate payloads, args, env, scalar values, exits | compiler-validation, contract checks | validation split across compile and runtime layers by design |
| Registry and summary | compile summary plan and artifact contracts | temp files, human report, summary writes, summary export sourcing | append, replay, terminal state, collect steps, compose summary | registry-detail, summary-json, replay tests | summary crosses JSON, NDJSON, export text, and shell |
| Service env export | compile service hook env and service apps | export `SVC_*` and `NIXFIED_SERVICE_*` into task runtime | limited direct role | service-hook and service-surface tests | service facts exist as both compiled surfaces and runtime exports |
| Service operations | compile service-set programs and service surface catalog | invoke per-service and service-set shell adapters | execute workflow phase entries through adapters | service-set adapter tests | service-set operations are materialized twice today |

The repeated pattern is:

- Nix is the compile-time authority
- Rust increasingly owns semantics
- shell still owns enough orchestration and transport that it remains a large
  framework layer instead of just an edge adapter

## Three End-To-End Traces

These traces show where layers multiplied instead of collapsing.

### Trace 1: Workflow driving

Current path:

1. flake app launches a shell wrapper
2. `dispatcher.nix` forwards to orchestrator
3. `orchestrator.nix` sets run lifecycle and runtime env
4. `executor.nix` prepares skipped-service files and shell adapters
5. `nixfied-kernel workflow run` drives scheduler transitions
6. kernel spawns task and service-set adapters
7. adapters return back into shell task or service operations

What improved:

- the serial and parallel scheduler state machines moved into Rust

What did not disappear:

- shell adapter generation
- shell task launch edges
- shell process-policy handling
- shell run inventory and stop controls

This is a partial collapse, not an end-to-end collapse.

### Trace 2: Task runtime metadata and hooks

Current path:

1. Nix compiles task runtime and hook runtime metadata
2. shell asks `runtime-metadata.nix` for task runtime information
3. that shell library calls `nixfied-kernel task load-runtime`
4. kernel prints shell exports
5. shell `eval`s the exports into cache variables
6. executor uses those variables to build runtime shell and invoke hooks

What improved:

- shell is no longer the authority on task runtime semantics

What did not disappear:

- shell metadata API
- export rendering protocol
- shell caching and indirection layer

This is better authority with only partial seam deletion.

### Trace 3: Run lifecycle and summary

Current path:

1. Nix compiles runtime artifact contracts and summary plan
2. orchestrator owns run-file layout, process mode, and inventory behavior
3. orchestrator-runtime shells out to `run-record read`
4. executor later shells out to `summary collect-steps`
5. kernel writes exports for summary counters
6. shell sources those exports and renders human output

What improved:

- run-record and summary semantics are typed and kernel-owned

What did not disappear:

- shell run-file logic
- shell refresh logic
- shell summary export sourcing
- temp-file choreography

This is exactly the pattern that kept the codebase from shrinking much.

## What Complexity Is Essential

The repository did gain real guarantees and features that justify some growth.

### 1. The kernel is a real authority now

The project removed framework-owned runtime semantics from:

- Python helpers
- `jq`-centric runtime logic
- CUE-based contract validation
- shell-owned scheduler state

That is a major architectural improvement.

### 2. Selected execution manifests are a real feature

The framework now compiles narrower execution surfaces and binds runtime
decisions to selected-service closure more explicitly.

That is not fake complexity.

It improves:

- scoping
- determinism
- service isolation
- help and validation fidelity

### 3. Registry, run-record, and summary guarantees are real

The current kernel owns meaningful behavior for:

- registry append and replay
- terminal state derivation
- run-record transitions
- summary step collection and composition

That is real product surface, not incidental plumbing.

### 4. Environment isolation is a real requirement

`env-sandbox.nix`, ephemeral materialization, slot and workspace policy, and the
related tests are paying for actual guarantees the framework advertises.

That work is legitimate.

### 5. Some test growth is warranted

When a repository exposes:

- machine-readable outputs
- stable runtime contracts
- stop controls
- registry replay
- selected-service scoping

it should expect meaningful contract coverage.

The mistake was not adding tests.

The mistake was failing to delete enough runtime seams before freezing them.

## What Complexity Is Accidental

This is the main problem.

### 1. The same knowledge exists in multiple representations

Examples:

- task runtime exists as compile-time metadata, kernel load commands, and shell
  cache variables
- workflow mode information exists as compiled metadata, kernel resolution
  commands, and shell wrappers
- service selection exists in selected manifests and runtime fallback imports

This is classic accidental framework fat.

### 2. Selection is still recomputable at runtime

Both:

- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/executor.nix`

can still fall back to importing `compile-selection-index.nix` if a compiled
selection index was not passed in.

That means the compile-time authority is still not singular in practice.

### 3. `serviceSetPrograms` still have dual authorities

`nixfied/framework/core/materializeExecution.nix` compiles `serviceSetPrograms`.

But `nixfied/framework/runtime/executor.nix` can synthesize them again if the
compiled value is empty.

That is a textbook example of duplicated responsibility surviving a refactor.

### 4. Runtime control is surfaced through more than one seam

`nixfied/framework/core/mkFlakeOutputs.nix` builds runtime control apps through
`orchestrator-control`.

At the same time, `dispatcher.nix` still defines shell app surfaces for:

- `runs`
- `stop-run`
- `stop-all-runs`

That is not catastrophic, but it is duplication.

### 5. Shell export transport still matters too much

The runtime still relies on:

- kernel rendering shell exports
- shell `eval`
- temp export files for some paths

This is better than pre-kernel JSON and `jq` plumbing, but it is still a
framework-specific transport seam that keeps shell logic larger than it should
be.

### 6. Service extractability is still more claimed than proven

The current extractability proof in
`tests/framework/service-extractability-contract.nix` uses fixture modules that
throw if private attributes are touched.

That proves the helper layer can consume a fake public API.

It does not prove the real service implementations are already behind small
enough public boundaries to split cleanly.

### 7. The test suite now carries architecture-governance machinery

The test suite gained:

- per-check layer metadata
- proof-kind metadata
- canonical coverage metadata
- owner-file metadata
- shard validation

This is useful, but it is still code paid to manage architecture that remained
too layered.

## Docs Versus Reality

### Where the docs overstate simplification

`docs/ARCHITECTURE.md` says shell runtime layers are thin adapters for launch
edges, env bootstrapping, signals, and service lifecycle commands.

That is not what the code size or responsibility map shows.

Shell still materially owns:

- run inventory
- stop controls
- process mode parsing
- adapter generation
- task launch edges
- summary export loading
- hook invocation

`docs/DETAILED.md` says shell metadata lookups are manifest-backed instead of
generated case tables.

That is true, but incomplete.

The shell still retains a metadata API layer through `runtime-metadata.nix`.

The representation is cleaner.

The layer did not die.

### Where the implementation is genuinely cleaner

- workflow scheduler state no longer lives in shell
- old stepwise workflow RPC is gone
- some old planning manifest kinds are gone
- kernel-owned validation and runtime semantics are much cleaner than the old
  mixed shell plus helper-tool model

So the codebase is not merely worse.

It is locally better and globally still too layered.

## Thesis Review

The original thesis was:

- collapse the old runtime layer stack
- delete generic framework code rather than merely reorganize it
- keep Nix as compile-time authority
- keep shell as a thin adapter only
- promote the Rust kernel to the single runtime execution authority
- make service boundaries strong enough that services are split-ready

### What landed

- Nix remains the compile-time authority in design intent
- the kernel is now the main semantic authority
- old helper-tool semantic seams really were removed
- workflow scheduler logic moved into Rust

### What partially landed

- shell became thinner than it used to be, but not thin enough
- runtime manifest count shrank, but not to a singular runtime form
- service scoping got stronger, but service APIs are still not obviously small
  or extractable enough

### What failed

- the runtime layer stack did not collapse end-to-end
- generic framework code was not deleted enough to offset new kernel, manifest,
  and test machinery
- the kernel did not become the single runtime execution authority; it became a
  stronger authority inside a still-large shell control plane
- services are not yet proven split-ready by real production boundaries

### What got diluted during rollout

The most important dilution was this:

when a better seam landed, the replaced seam often only partially died.

The result was:

- less semantic drift
- more explicit contracts
- no major reduction in total architectural surface area

By the RFC's own stated standard, that counts as failure to simplify.

## Why The Repository Grew

This is the shortest honest explanation.

### 1. Kernelization replaced shell semantics, but not shell orchestration

The repository paid to add a real Rust authority without deleting enough shell
control plane around it.

### 2. Deleted planning logic came back as metadata machinery

`workflow-modes.nix` died, but the same runtime planning knowledge came back as:

- compiled runtime metadata
- shell query helpers
- kernel load commands

### 3. Stronger guarantees created multi-layer guarantee tax

Guarantees were implemented and frozen across:

- compile-time Nix
- shell wrappers
- kernel semantics
- contract tests

That is legitimate only if whole replaced seams disappear.

In this refactor, many did not.

### 4. Service boundaries remained too framework-dependent

Because services are not yet behind truly small public contracts, the framework
still carries a large amount of generic service glue that cannot be cleanly
discarded.

### 5. The refactor improved several local subsystems at once

This was the correct engineering move for correctness.

But correctness and simplification are different goals.

The project achieved more of the first than the second.

## Concrete Surface-Area Evidence

### Subsystem deltas from `89a35ba` to current

Tracked-file code counts:

| Area | `89a35ba` | current | delta |
| --- | ---: | ---: | ---: |
| `nixfied/compiler` | 5,168 | 5,680 | +512 |
| `nixfied/framework/runtime` Nix core | 17,361 | 15,916 | -1,445 |
| `nixfied/framework/runtime/helpers` | 6,373 | 6,364 | -9 |
| `nixfied/framework/runtime/services` | 4,046 | 4,046 | 0 |
| `nixfied/framework/runtime/kernel/src` Rust | 6,356 | 7,921 | +1,565 |
| `nixfied/project` | 1,727 | 1,727 | 0 |
| `tests/framework` | 16,567 | 17,553 | +986 |

Interpretation:

- some shell runtime code really did shrink
- services did not shrink
- compiler and kernel grew
- tests grew materially
- the net effect was relocation plus moderate growth, not simplification

### Service glue versus sampled service implementation size

Generic framework glue around services:

| File | LOC |
| --- | ---: |
| `nixfied/framework/runtime/env-sandbox.nix` | 918 |
| `nixfied/framework/core/mkServiceRuntimeSurfaces.nix` | 442 |
| `nixfied/framework/core/mkServiceSetPrograms.nix` | 756 |
| `nixfied/framework/runtime/helpers/service-api.nix` | 647 |
| `nixfied/framework/runtime/services/service-operations-builder.nix` | 100 |
| `nixfied/framework/runtime/services/service-config-builder.nix` | 46 |
| `nixfied/framework/core/materializeExecution.nix` | 251 |
| total | 3,160 |

Sampled service implementation files:

| File | LOC |
| --- | ---: |
| `runtime/services/postgres/default.nix` | 289 |
| `runtime/services/postgres/config.nix` | 114 |
| `runtime/services/postgres/lifecycle.nix` | 601 |
| `runtime/services/nginx/default.nix` | 211 |
| `runtime/services/minio/default.nix` | 142 |
| `runtime/services/reth/default.nix` | 87 |
| `runtime/services/helios/default.nix` | 99 |
| total | 1,543 |

This is strong evidence that the framework platform around services is still
fatter than the services it is trying to abstract.

## Constraints For The Next Solution

These constraints remain valid.

### 1. New seam means old seam dies

If the next refactor lands a cleaner replacement, the old boundary must be
deleted in the same commit or the next one.

No dual authority.

No "we will clean this up later".

### 2. Nix remains compile-time authority

The answer is not to move authoring semantics out of Nix.

The answer is to stop re-materializing the same compile-time knowledge in
multiple runtime seams.

### 3. Shell should be judged by remaining control flow, not by intent

The test for "thin adapter" is not rhetoric.

It is whether shell still owns meaningful framework decisions or transports.

### 4. Service split-readiness must be tested against real services

Fixture-only proofs are not enough.

If service APIs are truly split-ready, real service modules should be forced to
depend only on public contracts and generated surfaces.

### 5. Test growth is justified only when it freezes a simpler runtime

If runtime layers stay multiplied, test metadata and shard governance can become
yet another architecture layer instead of a safety net around a leaner design.

## Candidate Deletion Program

These are the best current deletion-first discussion candidates.

They are not yet the final chosen plan.

### Candidate 1: delete runtime selection fallbacks

Remove runtime recomputation of `selectionIndex` and require compiled selection
data everywhere.

Likely files:

- `nixfied/framework/runtime/service-selection.nix`
- `nixfied/framework/runtime/orchestrator.nix`
- `nixfied/framework/runtime/executor.nix`

Payoff:

- removes duplicated authority
- makes "Nix is compile-time authority" real for selection

Risk:

- low

### Candidate 2: delete executor synthesis of `serviceSetPrograms`

Require compiled `serviceSetPrograms` and remove executor-side fallback
synthesis.

Likely files:

- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/core/materializeExecution.nix`

Payoff:

- removes an obvious duplicate representation

Risk:

- low to medium

### Candidate 3: choose one runtime-control surfacing path

Keep either:

- direct dispatcher ownership of runtime-control surfaces

or:

- `orchestrator-control` as the single runtime-control program surface

But not both.

Likely files:

- `nixfied/framework/runtime/dispatcher.nix`
- `nixfied/framework/runtime/orchestrator-control.nix`
- `nixfied/framework/core/mkFlakeOutputs.nix`

Payoff:

- removes duplicated public/internal glue

Risk:

- medium

### Candidate 4: delete the shell metadata getter layer

Replace `runtime-metadata.nix` and kernel `load-runtime` / `load-hook`
round-trips with one coarser runtime handoff.

Likely files:

- `nixfied/framework/runtime/runtime-metadata.nix`
- `nixfied/framework/runtime/kernel/src/task.rs`
- `nixfied/framework/runtime/kernel/src/workflow.rs`
- `nixfied/framework/runtime/executor.nix`

Payoff:

- deletes an entire Nix -> shell -> kernel -> shell metadata seam

Risk:

- high

### Candidate 5: move task dependency execution fully into the kernel

Today the kernel computes task execution order, but shell still loops the plan
and applies hook and failure choreography.

The more radical option is to let the kernel own task dependency execution
end-to-end as well.

Likely files:

- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/runtime/kernel/src/task.rs`

Payoff:

- large shell deletion opportunity
- fewer temp files
- fewer export protocols

Risk:

- high

### Candidate 6: make service boundaries real, then delete synthetic proof seams

Force real service modules to expose only public contracts or generated surfaces,
then simplify the synthetic extractability proof accordingly.

Likely files:

- `nixfied/framework/runtime/helpers/service-api.nix`
- `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
- `nixfied/framework/core/mkServiceSetPrograms.nix`
- `tests/framework/service-extractability-contract.nix`

Payoff:

- turns a design aspiration into a real deletion criterion

Risk:

- high

### Candidate 7: delete shell summary export sourcing

Let the kernel emit the final summary data directly in one form so shell stops
loading summary counters through export files.

Likely files:

- `nixfied/framework/runtime/executor.nix`
- `nixfied/framework/runtime/kernel/src/summary.rs`

Payoff:

- removes one more transport seam

Risk:

- medium

## Discussion Questions

These are the questions that should drive the next design conversation.

1. Do we want the kernel to own just workflow scheduling, or full task and
   workflow execution?
2. If shell must remain the live process supervisor, what exact control flow is
   still allowed to live there?
3. Should `nixfied-runtime-metadata` survive as a separate runtime family, or
   should it be absorbed into a single execution manifest family?
4. Which runtime transport forms are acceptable to keep?
5. Is service split-readiness a real goal, or just a cleanliness metaphor?
6. How much architecture-governance machinery in `tests/framework` is worth
   carrying if the runtime stack itself stays large?
7. Which seams can be deleted with low risk right now, before any deeper kernel
   expansion is attempted?

## What This RFC Is Not Saying

- It is not saying the kernel rewrite was a mistake.
- It is not saying the current architecture is worse than the old one.
- It is not saying all growth is accidental.
- It is not saying shell should disappear entirely no matter the operational
  cost.
- It is not saying test growth was useless.

It is saying something narrower and more important:

the repository improved correctness and authority more than it improved
simplicity, because it did not delete enough replaced seams.

## Acceptance Criteria For The Next Round

The next layers refactor should be considered successful only if it can show all
of the following.

### 1. A meaningful runtime seam disappears entirely

Examples:

- no runtime selection recomputation
- no executor synthesis fallback for service-set programs
- no shell metadata getter layer
- no shell summary export sourcing
- no duplicated runtime-control surfacing

### 2. Responsibility count goes down, not sideways

The same concern should stop being owned across:

- Nix
- shell
- Rust
- tests

unless the duplication is truly unavoidable.

### 3. The public runtime path gets shorter in substance, not just in docs

If the docs still describe shell as thin adapters, the code should reflect that.

### 4. The service boundary story becomes real

Real service modules should have to live behind small public surfaces if
split-readiness is still a design goal.

### 5. The measurement moves in the right direction

Improvement should be visible in some combination of:

- whole-repo code count
- runtime shell footprint
- manifest-family count
- transport-form count
- duplicated ownership count

## Appendix: Commands Run

Representative commands used during the investigation:

```bash
git log --oneline --decorate --graph --all
git diff --shortstat 89a35ba..bdfc68a
git show --summary --find-renames --stat 7a9ca5d
nix run nixpkgs#scc -- --include-ext nix,rs,md,json,txt,toml .
nix eval --json '.#apps.x86_64-darwin' --apply 'x: builtins.attrNames x'
rg -n '^## ' RFC_LAYERS_REFACTOR.md
rg -n 'serviceSetPrograms' nixfied -g '*.nix'
rg -n 'load-runtime|load-hook|validate-args|render-help|execution-order|exists|workflow-ref' \
  nixfied/framework/runtime/kernel/src/task.rs
```

## Final Position

The core thesis of the old RFC was directionally right:

- Nix should stay compile-time authority
- shell should get thinner
- the kernel should become the runtime authority
- services should be bounded by stronger public seams

But the rollout did not actually delete enough of the replaced framework.

The result is a codebase that is more correct, more typed, and more explicit,
yet still too fat for its own architectural story.

The next step should not be "organize the layers better".

It should be:

pick one duplicated runtime seam at a time and delete it for real.
