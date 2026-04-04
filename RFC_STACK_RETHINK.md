# RFC Stack Rethink

Date: 2026-04-03

Commit reviewed: `d19df0a`

Scope: current-HEAD deletion-first architecture teardown, no implementation changes

## Purpose

This RFC records an independent redesign-for-deletion review of the current
repository state.

It is not:

- a style review
- an incremental cleanup review
- a defense of current boundaries
- a migration plan optimized for compatibility

It is an answer to a narrower question:

if breaking changes are allowed, what architecture should replace the current
stack so the repository keeps its core product value while deleting as much
framework machinery as honestly possible?

## Relation To Earlier Layer Docs

This RFC is intended to supersede these earlier working documents as the active
stack-rethink record:

- `docs/LAYER_DECOMPOSITION.md`
- `docs/LAYER_OPTIONS.md`

What this RFC preserves from `docs/LAYER_DECOMPOSITION.md`:

- the distinction between irreducible responsibility, relocatable
  responsibility, and delete-pressure seams
- the main compile/runtime/service/test ownership breakdown
- the central claim that the architecture problem is duplicated authority rather
  than mere file size
- the practical conclusion that several named shell-era layers are not real
  target abstractions

What this RFC preserves from `docs/LAYER_OPTIONS.md`:

- the deletion-first stance
- the substantive recommendation sequence:
  - unify execution authority
  - delete shell metadata and export transport
  - collapse runtime control ownership
  - only then decide how much further to push kernel ownership and service
    boundary cleanup
- the locked design direction that:
  - `dispatcher` is not a runtime authority
  - runtime handoff is not a real layer
  - runtime manifests are not a separate target abstraction
  - shell should not keep a fine-grained metadata query API
  - kernel scope should grow only when a whole shell seam disappears
  - service boundary cleanup is justified only when it deletes duplicate
    framework glue

What this RFC intentionally does not preserve as active documentation:

- the exhaustive 27-layer sublayer inventory from the decomposition note
- the candidate scorecard and option menu as a standing decision framework
- intermediate cleanup framing that was useful while the earlier refactor was
  still unsettled

Those are safe to delete as active docs if the repository no longer needs that
historical option-analysis trail.

## Context Internalized Up Front

These constraints were treated as true only after checking the current tree:

- the earlier cleanup wave already landed
- the old cleanup RFC is effectively closed
- old deleted seams should not be re-litigated unless they still survive in
  another form
- the goal is architecture-for-deletion, not one more cleanup pass

Tree verification at `HEAD`:

- `tests/framework/default.nix` is now thin and only delegates to
  `tests/framework/framework-test-catalog.nix`
- `tests/framework/framework-test-catalog.nix` is the single framework test
  catalog authority
- `nixfied/framework/core/mkLauncherMetadata.nix` is gone
- `nixfied/framework/core/service-api.nix` is gone
- old framework test governance and taxonomy files referenced by earlier RFC
  narratives are gone
- old test rematerialization helpers are gone and the remaining test fixture is
  `tests/framework/lib/runtime-fixture.nix`

One important caveat:

- compiler-side `nixfied/compiler/service-contract-validation.nix` still exists,
  so the older intuition that this whole concern was fully deleted is false at
  `HEAD`. It survived in a smaller and better-located compiler form.

## Basis

Primary documents reviewed first:

- `docs/LAYER_DECOMPOSITION.md`
- `docs/LAYER_OPTIONS.md`
- `RFC_LAYERS_REFACTOR.md`
- `docs/ARCHITECTURE.md`
- `docs/DETAILED.md`

Primary repository areas inspected:

- `flake.nix`
- `nixfied/project/*`
- `nixfied/modules/*`
- `nixfied/compiler/*`
- `nixfied/framework/core/*`
- `nixfied/framework/launch/*`
- `nixfied/framework/runtime/*`
- `nixfied/framework/runtime/kernel/src/*`
- `tests/framework/*`

Representative commands used:

- `sed -n` on the documents and hotspot files listed above
- `rg --files .`
- `rg -n` for ownership, handoff, manifest, service, launcher, and test-catalog
  paths
- `wc -l` on hotspot files and major directories
- `git rev-parse --short HEAD`

Measured area snapshots used in this RFC:

- `nixfied/framework/runtime`: about `20,588` LOC
- `tests/framework`: about `15,268` LOC
- `nixfied/framework/core`: about `6,766` LOC
- `nixfied/compiler`: about `6,361` LOC
- `nixfied/modules/services`: about `5,449` LOC

Focused hotspot groups:

- flake plus launcher stack:
  - `flake.nix`
  - `nixfied/framework/core/mkFlakeOutputs.nix`
  - `nixfied/framework/launch/run-selected-app.nix`
  - `nixfied/framework/launch/run-runtime-app.nix`
  - `nixfied/framework/core/mkTaskHelpFiles.nix`
  - `nixfied/compiler/compile-views.nix`
  - total about `1,519` LOC
- shell control stack:
  - `nixfied/framework/runtime/dispatcher.nix`
  - `nixfied/framework/runtime/orchestrator.nix`
  - `nixfied/framework/runtime/executor.nix`
  - `nixfied/framework/runtime/runtime-handoff.nix`
  - `nixfied/framework/runtime/orchestrator-runtime.nix`
  - `nixfied/framework/runtime/shared-runtime-lib.nix`
  - `nixfied/framework/runtime/control-bootstrap.nix`
  - `nixfied/framework/runtime/env-sandbox.nix`
  - `nixfied/framework/runtime/artifacts-runtime.nix`
  - `nixfied/framework/runtime/common-runtime.nix`
  - total about `4,873` LOC
- service materialization stack:
  - `nixfied/framework/core/materializeExecution.nix`
  - `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
  - `nixfied/framework/core/mkServiceSetPrograms.nix`
  - `nixfied/framework/core/service-config.nix`
  - `nixfied/framework/core/service-observability.nix`
  - total about `2,894` LOC
- compiler execution and service authority subset:
  - `nixfied/compiler/compile-execution.nix`
  - `nixfied/compiler/compile-service-surface-catalog.nix`
  - `nixfied/compiler/compile-services.nix`
  - `nixfied/compiler/service-contract-validation.nix`
  - `nixfied/compiler/compile-service-catalog.nix`
  - `nixfied/compiler/compile-service-sets.nix`
  - total about `1,788` LOC
- kernel semantic core subset:
  - `nixfied/framework/runtime/kernel/src/task.rs`
  - `nixfied/framework/runtime/kernel/src/workflow.rs`
  - `nixfied/framework/runtime/kernel/src/summary.rs`
  - `nixfied/framework/runtime/kernel/src/registry.rs`
  - `nixfied/framework/runtime/kernel/src/run_record.rs`
  - `nixfied/framework/runtime/kernel/src/validation.rs`
  - `nixfied/framework/runtime/kernel/src/probe.rs`
  - total about `7,403` LOC

## 1. Executive Verdict

At `HEAD`, drastic simplification is still available, but it is no longer true
that most of the remaining size is generic framework fat. The compiler,
service-contract compilation, and the Rust kernel now carry a substantial amount
of real product behavior. The remaining oversized glue is concentrated in the
flake launcher and selection path plus the shell control stack that still sits
between compile-time truth and runtime truth. The single biggest architectural
mistake still present is dual runtime ownership: the compiler already knows the
execution graph, but shell layers still re-select, re-export, re-hydrate,
supervise, and query that graph as if they were a second runtime authority.
This RFC should therefore be read as "delete duplicate authorities in the
middle of the stack", not as "delete service reuse or runtime lifecycle
guarantees themselves". Public service reuse, `SKIP_<SERVICE>` in env and CI,
env isolation, artifact placement, process setup, process-group tracking,
slot-scoped run visibility, and stop behavior are all real product concerns
that should survive under one owner.

## 2. Current Stack Truth

### Current top-level layers that really exist at `HEAD`

| Layer | Classification | Why it exists now | Concrete files |
| --- | --- | --- | --- |
| typed authoring and schema | `relocatable` | project and framework configuration still need a typed declaration boundary | `nixfied/project/module.nix`, `nixfied/modules/default.nix`, `nixfied/modules/tasks.nix`, `nixfied/modules/workflows.nix`, `nixfied/modules/services/*` |
| compiler authority | `irreducible` | canonical semantic owner for tasks, workflows, services, views, features, and execution graph | `nixfied/compiler/default.nix`, `nixfied/compiler/compile-execution.nix`, `nixfied/compiler/finalize-model.nix` |
| service contract compiler | `irreducible` | validates and compiles the public service API and operation catalog | `nixfied/compiler/compile-service-surface-catalog.nix`, `nixfied/compiler/service-contract-validation.nix`, `nixfied/modules/services/*.nix` |
| execution materialization | `relocatable` | still turns compiled service and app data into concrete runtime surfaces | `nixfied/framework/core/materializeExecution.nix`, `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`, `nixfied/framework/core/mkServiceSetPrograms.nix` |
| flake surfacing plus launchers | `delete-pressure` | exports user-facing apps and currently re-runs selection through shell and Nix recursion | `flake.nix`, `nixfied/framework/core/mkFlakeOutputs.nix`, `nixfied/framework/launch/run-selected-app.nix`, `nixfied/framework/launch/run-runtime-app.nix` |
| shell control runtime | `delete-pressure` | still owns process supervision, run inventory, stop controls, shell parsing, and handoff transport | `nixfied/framework/runtime/dispatcher.nix`, `nixfied/framework/runtime/orchestrator.nix`, `nixfied/framework/runtime/executor.nix`, `nixfied/framework/runtime/runtime-handoff.nix` |
| kernel runtime | `irreducible` | owns task dependency execution, workflow scheduling, summary composition, registry semantics, run-record validation, and probe logic | `nixfied/framework/runtime/kernel/src/task.rs`, `nixfied/framework/runtime/kernel/src/workflow.rs`, `nixfied/framework/runtime/kernel/src/summary.rs`, `nixfied/framework/runtime/kernel/src/registry.rs`, `nixfied/framework/runtime/kernel/src/run_record.rs`, `nixfied/framework/runtime/kernel/src/validation.rs` |
| guarantee harness | `relocatable` | centralized test catalog and behavior proofs | `tests/framework/framework-test-catalog.nix`, `tests/framework/default.nix`, `tests/framework/lib/harness.nix` |

### Layer truth notes

- `dispatcher` is not a real long-term boundary. It is a surfacing layer over
  `orchestrator`, not an authority.
- `runtime-handoff` is not a real boundary. It is a transport layer around
  fine-grained kernel getter commands.
- `service runtime surfaces` still perform real work, but too much of that work
  is framework translation rather than irreducible product ownership.
- `tests/framework/framework-test-catalog.nix` is large, but it is correctly a
  single authority now. The question is which behaviors it should still test,
  not whether a catalog file should exist.

## 3. Real Complexity vs Framework Fat

### Mostly Real Complexity

- `nixfied/compiler/compile-execution.nix`
  - This file is big because it owns workflow family and mode derivation,
    service-closure computation, task and workflow execution descriptors, and
    cross-graph narrowing math.
  - The real semantic center is the `nixfied-execution` graph built in this
    file.
  - The suspicious part inside it is not the closure logic. It is the extra
    model wrapping for per-app execution manifests and shell-facing string
    payloads.

- `nixfied/framework/runtime/kernel/src/task.rs`
  - Real complexity.
  - Task dependency traversal, service-skip handling, adapter invocation, and
    argument validation are actual product behavior.
  - This file should not be aggressively targeted just because it is large.

- `nixfied/framework/runtime/kernel/src/workflow.rs`
  - Real complexity.
  - Workflow state transitions, fail-fast behavior, dependency cancellation,
    parallel scheduling semantics, and mode resolution are actual product
    behavior.
  - The shell boundary around it can shrink. The workflow semantic core should
    remain.

- `nixfied/compiler/compile-service-surface-catalog.nix`
  - Real complexity.
  - This file is now a legitimate public contract compiler: operation catalog,
    hook names, generated service app shape, and command contract metadata all
    derive from typed service contract data.

- `nixfied/compiler/service-contract-validation.nix`
  - Real complexity.
  - Runtime primitive validation, required lifecycle op validation, operation
    composition checks, and contract-shape checking belong in the compiler.

- `nixfied/modules/services/*.nix`
  - Mostly real product behavior.
  - The public service contract and project-visible knobs belong here.
  - Example: `nixfied/modules/services/postgres.nix` is large because PostgreSQL
    itself is a real product subsystem.

- `tests/framework/framework-test-catalog.nix`
  - Mostly real as a single test authority.
  - The file is large because it is centralizing the full framework behavior
    matrix.
  - The target is not to split it again; the target is to reduce how many
    current seams it must freeze.

### Still Suspicious / Architecture-Heavy

- `nixfied/framework/core/mkFlakeOutputs.nix`
  - Still architecture-heavy glue.
  - It builds launcher help fast paths, selector parsing, selected-service CSV
    plumbing, internal base targets, and recursive `nix-build` launcher jumps.
  - This is too much machinery for what should be thin app surfacing.

- `nixfied/framework/launch/run-selected-app.nix`
  - Pure transport and compatibility logic.
  - It reparses selector inputs, recompiles the project with `selectedServices`,
    and returns a tiny wrapper around the selected app.
  - This is a clear deletion target.

- `nixfied/framework/runtime/runtime-handoff.nix`
  - Pure transport leftover.
  - It shells out to kernel `task export`, `workflow export`,
    `workflow export-resolved`, `task runtime-plan`, `selected-services-csv`,
    and hook export getters, then `eval`s shell export text.
  - This is the strongest remaining echo of the old shell-first runtime design.

- `nixfied/framework/runtime/dispatcher.nix`
  - Mostly compatibility surfacing.
  - It adds little irreducible value beyond proxying to `orchestrator`,
    packaging static help, and preserving current app surfaces.

- `nixfied/framework/runtime/orchestrator.nix`
  - Mixed file.
  - Run inventory and stop control are real runtime concerns.
  - But it is also a second control authority layered over the kernel and
    `executor`, with separate run record, signal, janitor, and shell launch
    choreography.

- `nixfied/framework/runtime/executor.nix`
  - Mixed file leaning too glue-heavy.
  - Real concerns exist: env isolation, process launch, hooks, summary writing.
  - But it still repeatedly calls `task_handoff_use`, resolves workflow handoff
    state through exports, hosts multiple internal adapters, and splits task
    execution across kernel plus shell.

- `nixfied/framework/core/materializeExecution.nix`
  - Too many materialization authorities.
  - It recomputes service runtimes, per-app runtimes, service-set runtimes, and
    app program families from the compiler output.

- `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
  - Still a major deletion target.
  - Some work here is real, but too much of it is generic translation from
    compiler-owned contracts into `serviceAppPrograms`, `serviceHookEnv`, slot
    scripts, and framework runtime launchers.

- `nixfied/framework/core/mkServiceSetPrograms.nix`
  - Still very framework-centric glue.
  - The file turns service-set policy and service APIs into a generated app
    platform. That is not an irreducible boundary.

- tests bound to current seams rather than final product behavior
  - `tests/framework/launcher-help-fast-path-smoke.nix`
  - `tests/framework/dispatcher-help-fast-path-smoke.nix`
  - `tests/framework/framework-install-no-caller-compile-smoke.nix`
  - `tests/framework/framework-upgrade-no-caller-compile-smoke.nix`
  - `tests/framework/runtime-controls-no-service-materialization-smoke.nix`
  - `tests/framework/flake-show-no-service-materialization-smoke.nix`
  - These are justified only while the current launcher and materialization seams
    survive.

### What Earlier Intuitions About "Framework Fat" Get Wrong Now

- It is no longer correct to treat the remaining size of
  `kernel/src/task.rs` and `kernel/src/workflow.rs` as generic fat. Those files
  now mostly implement real execution behavior.
- It is no longer correct to treat the existence of one large framework test
  catalog as a smell by itself. Centralized catalog authority is cleaner than
  the older fragmented governance files.
- It is no longer correct to assume service contract work still lives mainly in
  framework runtime glue. Public service authority is now mostly compiler-owned.
- The remaining large deletion opportunity is not "delete the kernel". It is
  "delete everything that still mediates between the compiler and the kernel."

## 4. Delete-With-Breaking-Changes Plan

### Target architecture

If breaking changes are allowed, the target architecture should collapse to
five layers only:

1. typed authoring
2. compiler
3. single runtime engine
4. thin packaging
5. guarantee harness

### Final desired layers only

- typed authoring
  - typed Nix modules define tasks, workflows, service contracts, and runtime
    policy
- compiler
  - emits one canonical execution graph plus one small contract bundle if
    external validation still needs a detached artifact
- single runtime engine
  - one Rust binary owns task execution, workflow execution, run inventory, stop
    controls, summaries, and service operation dispatch
- thin packaging
  - flake apps are dumb wrappers that exec the runtime against the compiled
    graph
- guarantee harness
  - user-facing contract and e2e tests only

### Whole current layers that should disappear entirely

- dispatcher as an architectural layer
- orchestrator as a separate authority
- runtime-handoff as a layer
- selector launchers as a first-class layer
- per-app execution-manifest models
- generated `serviceHookEnv` as a separate authority
- generated `serviceSetPrograms` as a separate authority
- duplicate generated service wrapper families as parallel authorities
- the current ambient `SVC_*` hook transport as a framework-wide implicit
  contract, if one explicit runtime ABI replaces it

### Smallest necessary top-level layers starting from product needs

Starting from product needs rather than current implementation, the repository
needs only:

- one typed authoring boundary
- one compiler authority
- one runtime authority
- one packaging surface
- one test harness

Everything else is a candidate to collapse into one of those.

### Product constraints that must survive the deletion

- users still need one supported service-facing interface to reuse, adapt, and
  customize service setup, configuration, and operations
- `SKIP_<SERVICE>` in env and CI is a public contract to preserve, even if its
  implementation owner changes
- env isolation, artifact placement, process setup, process-group lifecycle,
  slot-scoped run tracking, and stop semantics are real runtime
  responsibilities
- the redesign target is one owner for each concern, not removal of those
  capabilities
- per-app execution manifests still look like compatibility transport rather
  than a product requirement
- the strongest remaining deletion targets are launcher recompilation,
  runtime-handoff export and getter transport, duplicate service wrapper
  platforms, and split runtime control ownership

## 5. Canonical Ownership Map

| Responsibility | One surviving owner |
| --- | --- |
| compiled execution | compiler-owned canonical execution graph |
| service contracts | typed service modules compiled by `compile-service-surface-catalog.nix` |
| service public surface | one compiler-defined service ABI exposed through one supported packaging and runtime contract |
| service runtime materialization | compiler-owned service operation plans consumed directly by the runtime engine |
| task execution | single runtime engine |
| workflow execution | single runtime engine |
| help rendering | compiler, from the same execution graph |
| selection and narrowing | compiler-owned selection metadata applied by the single runtime engine |
| service-skip public API (`SKIP_<SERVICE>`) | one normalized invocation contract interpreted once by the single runtime engine |
| env isolation | single runtime engine |
| artifact placement | single runtime engine |
| process setup and process-group lifecycle | single runtime engine |
| slot-scoped run inventory and visibility | single runtime engine |
| stop controls | single runtime engine |
| framework test catalog | `nixfied/framework/testing/catalog.nix` projected into `tests/framework/framework-test-catalog.nix` |
| framework test execution | one public `test` surface over `workflow.test.<mode>` plus internal `task.test.framework.<profile>.<shard>` debug entrypoints |
| guarantee harness | `tests/framework` focused on user-facing behavior proofs |

### Ownership comments

- help rendering should stop being split across `compile-views.nix`,
  `mkTaskHelpFiles.nix`, `dispatcher`, and launcher fast-paths
- selection should stop being split across:
  - compiler closure math in `compile-execution.nix`
  - launcher parsing in `mkFlakeOutputs.nix`
  - selected app recompilation in `run-selected-app.nix`
  - kernel selected-service getters
- invocation-time `SKIP_<SERVICE>` input can stay public, but it should be
  normalized once and not reinterpreted in launcher glue
- service runtime materialization should stop being split across:
  - compiler contract compilation
  - framework runtime surface generation
  - shell hook env materialization
  - service-set program generation
- service reuse itself should not be deleted; only duplicate wrapper families
  and ambient transport should be targeted
- env, artifacts, process setup, process-group tracking, and slot-scoped run
  visibility should stop being split across shell helper layers

## 6. Deletion Waves

The waves below are grouped to be parallel-safe at the authority level, not at
the file-edit level.

### Wave 1: unify execution authority

- goal
  - replace the current family of execution artifacts with one canonical
    compiler-owned execution graph
- whole authorities to delete
  - per-app execution manifest models
  - shell-facing execution getter forms
  - indirect "model or manifest" dual loading in the kernel
- main write set
  - `nixfied/compiler/compile-execution.nix`
  - `nixfied/compiler/finalize-model.nix`
  - `nixfied/framework/runtime/kernel/src/execution_metadata.rs`
  - `nixfied/framework/runtime/runtime-handoff.nix`
- expected breakages
  - execution payload shape
  - kernel export subcommands
  - task and workflow runtime-plan getter paths
- estimated LOC win
  - about `0.8k` to `1.5k`
- risk
  - high
- why it is worth the churn
  - every later deletion depends on one canonical execution representation

### Wave 2: delete launcher and selection as a layer

- goal
  - make app surfacing thin, stop recursing back into Nix for selection, and
    preserve `SKIP_<SERVICE>` as a public input while deleting launcher-side
    ownership of it
- whole authorities to delete
  - selector-aware launchers
  - runtime launcher indirection
  - fast-path launcher help as a separate system
- main write set
  - `flake.nix`
  - `nixfied/framework/core/mkFlakeOutputs.nix`
  - `nixfied/framework/launch/run-selected-app.nix`
  - `nixfied/framework/launch/run-runtime-app.nix`
  - `nixfied/compiler/compile-views.nix`
  - `nixfied/framework/core/mkTaskHelpFiles.nix`
- expected breakages
  - public app names
  - `--exclude-services`
  - `--launcher-help`
  - launcher-side implementation details for `SKIP_<SERVICE>`, while preserving
    the env and CI contract itself
- estimated LOC win
  - about `1.0k` to `1.8k`
- risk
  - high
- why it is worth the churn
  - current flake surfacing is doing far more than packaging should ever do

### Wave 3: collapse shell runtime control into one owner

- goal
  - remove the dispatcher plus orchestrator plus executor split and leave one
    runtime owner for execution, env, artifacts, process setup, process-group
    lifecycle, run inventory, and stop control
- whole authorities to delete
  - dispatcher
  - runtime-handoff
  - orchestrator-runtime
  - shared-runtime-lib as a separate layer
  - most of orchestrator as a distinct authority
- main write set
  - `nixfied/framework/runtime/dispatcher.nix`
  - `nixfied/framework/runtime/orchestrator.nix`
  - `nixfied/framework/runtime/executor.nix`
  - `nixfied/framework/runtime/kernel/src/task.rs`
  - `nixfied/framework/runtime/kernel/src/workflow.rs`
  - `nixfied/framework/runtime/kernel/src/summary.rs`
- expected breakages
  - runtime CLI shape
  - internal stop-control plumbing
  - run-record write path
  - summary output path
- required semantics to preserve
  - env isolation
  - artifact placement
  - process setup and process-group tracking
  - slot-scoped run inventory and framework API visibility
  - reliable stop semantics
- estimated LOC win
  - about `2.5k` to `4.5k`
- risk
  - very high
- why it is worth the churn
  - this is the biggest remaining ownership collapse in the repository

### Wave 4: collapse service runtime surface generation

- goal
  - stop treating duplicate generated service surfaces as a first-class
    framework platform and leave one supported service reuse surface
- whole authorities to delete
  - ambient or generated `serviceHookEnv` as a separate authority
  - generated service app wrappers as a parallel ownership layer
  - generated `serviceSetPrograms` as a separate authority
  - duplicate public service entrypoint families when one narrower service ABI
    can replace them
- main write set
  - `nixfied/framework/core/materializeExecution.nix`
  - `nixfied/framework/core/mkServiceRuntimeSurfaces.nix`
  - `nixfied/framework/core/mkServiceSetPrograms.nix`
  - `nixfied/compiler/compile-service-surface-catalog.nix`
- expected breakages
  - current `svc::*` or `svcset::*` entrypoint shapes if they are not chosen as
    the surviving public surface
  - the current ambient `SVC_*` task hook env transport if it is replaced by
    explicit runtime inputs
  - internal wrapper-composition APIs
- estimated LOC win
  - about `1.5k` to `3.0k`
- risk
  - very high
- why it is worth the churn
  - service contracts are now compiler-owned enough that the extra runtime
    platform layer is more transport than authority, but service reuse itself
    must survive

### Wave 5: prune seam-freezing tests

- goal
  - keep behavior proofs and delete internal seam-freezing guarantees tied to
    removed layers
- whole authorities to delete
  - current launcher-specific proofs
  - no-caller-compile seam tests
  - no-service-materialization seam tests
  - helper harnesses that exist only to support deleted runtime seams
- main write set
  - `tests/framework/framework-test-catalog.nix`
  - `tests/framework/lib/harness.nix`
  - `tests/framework/lib/runtime-fixture.nix`
  - the adapter and migration shard members tied to removed seams
- expected breakages
  - many current adapter-shard checks
  - some migration-shard checks
- estimated LOC win
  - about `2.0k` to `4.0k`
- risk
  - medium
- why it is worth the churn
  - the current harness still protects several seams that the redesign should
    intentionally delete

## 7. Maximum Honest LOC Win

### Likely

About `8k` to `12k`.

Assumptions:

- launcher recursion and selector launchers are deleted
- dispatcher and runtime-handoff disappear
- orchestrator and executor collapse materially
- most generic service runtime transport disappears
- seam-freezing tests are pruned accordingly
- kernel semantics, service modules, and compiler semantic core mostly remain

### Aggressive but plausible

About `13k` to `18k`.

Assumptions:

- the runtime path fully collapses to one Rust-led engine
- service operations stop existing as a generated app platform
- current shell env-hook transport becomes a narrow runtime ABI or disappears
- help and selection fully become compile-owned data
- large parts of adapter-heavy test coverage are deleted with the seams

### Unrealistic fantasy

`20k+`.

Assumptions that would have to be false:

- that the remaining kernel logic is mostly framework vanity
- that large service modules are mostly generic glue
- that typed validation and contract machinery can be removed without removing
  real product guarantees
- that the test harness is large only because of catalog structure rather than
  real product surface area

### Honest maximum

The realistic ceiling is meaningfully below earlier "delete the framework fat"
intuition because the cleanup wave already cashed in some low-quality layers.
The remaining high-value deletion is concentrated in cross-layer transport,
runtime dual ownership, and test scaffolding that freezes those seams.

## 8. Bottom Line

### If we want drastic LOC reduction, where do we attack first?

Attack these first:

- the flake launcher and selection stack
- the dispatcher plus orchestrator plus executor split
- runtime-handoff and kernel export getter transport
- service runtime surface generation as a separate platform

### What should we stop attacking because it is now mostly real complexity?

Stop treating these as primary deletion targets:

- `nixfied/framework/runtime/kernel/src/task.rs`
- `nixfied/framework/runtime/kernel/src/workflow.rs`
- compiler-owned service contract validation and surface cataloging
- the existence of one large `tests/framework/framework-test-catalog.nix`
  authority
- service modules whose size mostly reflects real subsystem behavior

### What architecture should we choose if we are allowed to break everything necessary?

Choose this:

- typed authoring boundary in Nix
- one compiler that emits one canonical execution graph
- one runtime engine that owns task execution, workflow execution, service
  execution, env isolation, artifact placement, process setup, process-group
  lifecycle, slot-scoped run inventory, stop controls, and summary composition
- one supported public service surface, not multiple generated wrapper
  platforms
- thin flake packaging that only execs the runtime
- user-facing guarantee tests only

### Final plain answer

Yes, major simplification is still available.

No, the remaining stack is not mostly glue overall.

This is a delete-duplicate-authorities RFC, not a delete-service-capabilities
RFC.

The remaining fat is concentrated in the space between the compiler and the
runtime, where the repository still pays for:

- multiple execution representations
- shell-first launch and selection recursion
- kernel export and shell re-hydration
- duplicated runtime control ownership
- test machinery that preserves those seams

If the explicit goal is drastic LOC reduction, the redesign should delete that
middle space rather than keep polishing it.
