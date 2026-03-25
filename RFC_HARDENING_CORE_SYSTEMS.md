# RFC: Harden Core Systems

Status: reopened

Last updated: 2026-03-24

## Purpose

This RFC turns the current architecture review into a concrete replacement plan.

`RFC_REPLACE_BASH_JQ_TO_CUE.md` improved the framework by tightening
machine-boundary validation, but it did not make the core hard. The current
system is still soft because:

- runtime semantics still live in large shell programs
- command API meaning is split across multiple parallel schemas
- several framework-owned JSON adapter paths still rely on `jq`
- important module boundaries are still typed too loosely

This RFC defines which pieces of code must be replaced, what they must be
replaced with, and which layers should remain in place.

## Status Update (2026-03-24)

As of the current `2026-03-24` worktree, the hardening work is only partially
closed. The compiler/model/API consolidation is landed, the kernel owns the
validation and runtime artifact/state-writing paths, and several closure items
previously marked done are now confirmed to remain open in substance.

Landed:

- Lane F foundations: kernel packaging, validation IR, and runtime asset
  wiring
- Lane A authoring/model work: typed command authoring, compiled-only
  `model.apps`, compiler-owned service operation catalog, and
  `model.compiled.*`
- Lane K kernel responsibilities: `validate-*`, `run-record`, `registry`,
  `summary`, and `machine-output run`
- Lane R partial closure: run-id envelope rendering, run-record writes,
  registry append/replay, summary rendering/composition, and event-detail
  envelope rendering moved to kernel-backed paths; `common-runtime.nix` no
  longer contains the older framework JSON builders; and
  `shell-contract.nix` is on the kernel validation/export path
- Lane R additional closure: `orchestrator.nix`, `orchestrator-control.nix`,
  and `orchestrator-runtime.nix` now route run-record reads and terminal-state
  derivation through kernel-backed commands instead of shell field parsing and
  index scanning
- Lane D closure: `probe-plan-runtime.nix` now routes runtime probe execution
  through kernel-owned `probe evaluate` plans, and the kernel executes probe
  kinds end to end instead of shell-rendered probe bodies
- Contract-tightening tail: `runtime.summary.payload` now uses refined stable
  ids, workflow-mode enums, and UTC timestamp helpers
- Lane G guard foundations: CUE removal, deleted authored `nixfied.apps`,
  deleted static service surface, and no Python responders under
  `tests/framework`
- Lane G additional cleanup: framework build-check paths are `jq`-free, the
  hardening guard now covers the probe-plan runtime and the
  orchestrator run-record/terminal-state seam, and the related contract tests
  assert the kernel-backed boundary

Still open:

- Lane R thin-shell closure is not complete. `executor.nix` still owns
  the workflow serial/parallel scheduler state engines, including ready/pending
  queue management, lock arbitration, fail-fast cancellation, and unit
  transition bookkeeping.
- Lane G guard enforcement is not complete. The guard is `jq`/CUE/Python-free
  oriented, and it now covers probe execution, task dependency planning,
  executor summary aggregation, and orchestrator reads, but it does not yet
  fail on the remaining workflow scheduler semantics that still sit inside the
  hardened-core runtime boundary.

Residual `jq` use in smoke tests that only inspect outputs remains outside the
hardened-core ownership boundary described here.

## Decision Summary

- Keep Nix as the compile-time authority for modules, compiler passes,
  canonicalization, manifests, and public surface generation.
- Replace the current shell-heavy runtime core with one internal typed runtime
  kernel, referred to here as `nixfied-kernel`.
- The target core stack is `Nix + Rust + thin Bash`.
- Remove CUE from the repository and from the runtime architecture entirely.
- Replace generated external validators with `nixfied-kernel validate-*`
  commands operating on Nix-compiled validation data.
- Replace duplicated API vocabularies with one canonical `commandApi` model in
  Nix.
- Remove framework-owned `jq` from runtime core and adapter logic. During
  migration it may remain in tests only, then it must be removed from framework
  build checks as well.
- Tighten contracts to be closed by default, tagged by `kind` and `version`,
  and refined for timestamps, ids, and state transitions.
- Breaking changes are allowed wherever they simplify and harden the core.
- Do not preserve legacy code paths, compatibility adapters, or parallel
  validation engines.
- Keep the kernel narrow. It hardens runtime semantics and mutable
  artifact/state handling without re-implementing module evaluation, compiler
  passes, or public launcher generation.

## Change In Direction From The Previous RFC

The previous RFC deliberately stopped at boundary validation and preserved the
"checked-in framework source stays `.nix` only" constraint.

This RFC does not.

The review shows that boundary validation alone is not enough. The framework
needs a small typed execution and state kernel. That means the old `.nix`-only
constraint is now too expensive in the core runtime and should be relaxed for
the internal kernel implementation.

This RFC still rejects three failure modes:

- under-correcting by leaving shell as the semantics layer
- over-correcting by turning Rust into a second compiler or framework
- carrying forward transitional validation or compatibility layers after the new
  kernel path exists

The externally visible requirement is the interface, not the implementation
language:

- the internal binary name is `nixfied-kernel`
- it is not a public user surface
- it must be implemented in a typed compiled language
- if no separate language decision is made, Rust is the default target
- it must stay a narrow runtime-semantic unit rather than a parallel authoring
  or compilation system

## Hard Constraints

1. Breaking changes are allowed. Backward compatibility is not a goal of this
   RFC.
2. Keep human CLI output plain ASCII with stable prefixes.
3. Do not add compatibility shims between old and new internal artifact
   formats, CLIs, or validation paths. Move forward and delete the old path.
4. Keep runtime artifacts deterministic, versioned, and atomically written.
5. All framework-owned JSON semantics must live either in the Nix contract
   model or in `nixfied-kernel`, never in ad hoc shell or `jq` fragments.
6. `nixfied-kernel` must not duplicate module evaluation, compiler passes, or
   public flake surface generation.
7. Shell may launch workloads, but it must not regain framework-owned JSON,
   API, or state-machine semantics during migration.
8. CUE is removed from the framework. Machine validation must be performed by
   `nixfied-kernel validate-*` commands over Nix-compiled validation data.

## Target Layer Model

### 1. Module Typing Layer

Nix submodules and enums reject invalid framework configuration during module
evaluation.

Language:

- Nix

### 2. Compiler Layer

Deterministic Nix passes derive:

- canonical model
- canonical command API catalog
- runtime manifests
- contract bundle
- introspection bundle

Language:

- Nix

### 3. Contract Layer

The Nix contract DSL remains the single source of truth for machine-facing
shapes and renders:

- JSON Schema
- docs
- kernel validation data
- optional external schema exports

Languages:

- Nix

### 4. Runtime Kernel Layer

`nixfied-kernel` owns:

- input validation
- scalar validation
- payload validation against Nix-compiled contract data
- JSON encode/decode
- run-record state transitions
- registry event append and replay
- summary writing
- machine-output validation
- third-party adapter parsing approved by the framework
- runtime semantic interpretation wherever shell would otherwise need to
  re-derive framework meaning

Language:

- Rust

### 5. Shell Layer

Shell remains only for:

- environment setup
- process launch and trap handling
- file path and temp-dir staging
- invoking project shell commands
- invoking `nixfied-kernel`
- streaming stdout/stderr and carrying bytes between workloads and the kernel

Shell must not own framework semantics.

Language:

- Bash

## Layer Ownership Matrix

This is the strict ownership model for the framework. If a concern appears in
more than one layer, that is a design smell and should be treated as drift.

### Layer 1: Authoring Layer

Primary files:

- `nixfied/project/`
- `nixfied/modules/`

Responsibilities:

- declare project config
- declare tasks, workflows, services, and presets
- enforce option-level validity through submodules, enums, and typed attrs
- express intended command API shape in the canonical Nix authoring model

Language:

- Nix

Must not own:

- runtime state transitions
- JSON parsing
- machine artifact writing
- shell transport semantics

### Layer 2: Compiler Layer

Primary files:

- `nixfied/compiler/`

Responsibilities:

- resolve and normalize module output
- compile canonical model
- compile API catalog
- compile runtime manifests
- compile selection indexes and introspection bundles
- compute deterministic hashes

Language:

- Nix

Must not own:

- process execution
- mutable state
- runtime artifact mutation
- third-party adapter parsing

### Layer 3: Contract Layer

Primary files:

- `nixfied/contracts/`
- `nixfied/framework/contracts/`

Responsibilities:

- define machine-facing schemas
- define versioned envelopes
- define closed or open record policy
- render JSON Schema, docs, and kernel validation data

Languages:

- Nix

Must not own:

- execution orchestration
- state transitions
- business logic for runtime flows
- behavioral correctness for runtime state machines

### Layer 4: Runtime Kernel Layer

Primary files:

- `nixfied/framework/runtime/kernel/`

Responsibilities:

- parse and validate framework-owned runtime inputs
- own run-record lifecycle transitions
- own registry append and replay semantics
- write summaries and machine-output envelopes
- encode and decode framework JSON
- provide typed adapter entrypoints for approved third-party JSON sources
- interpret runtime semantics that would otherwise be re-derived in shell

Language:

- Rust

Must not own:

- project configuration authoring
- module evaluation
- flake surface generation
- canonical contract source definitions
- generic project workload implementation
- env staging, temp-dir management, and signal forwarding

### Layer 5: Adapter Layer

Primary files:

- runtime-kernel adapter modules for JSON-RPC, Helios, supervisor, and similar
  integrations

Responsibilities:

- translate third-party payloads into typed internal values
- validate edge contracts before runtime state changes depend on them
- isolate external schema drift from the rest of the framework

Language:

- Rust

Must not own:

- canonical framework schema definitions
- orchestration policy
- user-facing flake surface layout

### Layer 6: Launcher And Shell Layer

Primary files:

- `nixfied/framework/core/`
- thin wrappers under `nixfied/framework/runtime/`

Responsibilities:

- set up env vars, temp dirs, and workdirs
- launch project commands and service commands
- trap and forward signals
- call `nixfied-kernel`
- expose public launch surfaces generated by Nix

Language:

- Bash

Must not own:

- framework JSON semantics
- API validation rules
- state transitions
- machine artifact construction
- adapter parsing logic

### Layer 7: Project Workload Layer

Primary examples:

- project shell commands
- service CLIs
- test runners
- migrations
- builds

Responsibilities:

- do the actual work requested by tasks and workflows

Language:

- project-defined

Must not own:

- framework runtime state model
- framework machine contracts
- framework launcher semantics

## Language Placement Summary

- Nix owns authoring, module typing, compiler passes, manifest generation, and
  contract source definitions.
- Rust owns runtime meaning, state mutation, JSON semantics, and approved
  adapters, including all framework-owned validation.
- Bash owns launch, environment setup, process control, and public shell entry
  surfaces.
- `jq` owns nothing in the framework core. During migration it may remain in
  tests only.
- CUE owns nothing in the framework.
- The minimal technology set that can still deliver a hard core is `Nix + Rust
  + Bash`.

## Architecture Options Considered

1. `Nix + Bash`, minimal change.

   Rejected. It improves validation but still leaves runtime state, JSON
   semantics, and artifact writing in shell.

2. `Nix-heavy + thinner shell`, no Rust.

   Rejected. It improves authoring and compiler discipline, but mutable runtime
   behavior remains too soft once shell still owns state transitions and JSON
   handling.

3. `Nix + Rust kernel + thin Bash`.

   Accepted. Nix keeps compile-time authority, Rust hardens runtime semantics
   and validation, and Bash stays as orchestration glue.

4. `Maximal Rust kernel`.

   Rejected. This would over-correct by risking a second framework/compiler
   implementation outside Nix and by turning the kernel into a broad execution
   framework rather than a narrow semantic core.

## Anti-Fruit-Salad Rule

The same concern must not be implemented in multiple languages.

Examples:

- task and app API meaning must not be split between Nix authoring and Bash
  validators
- runtime artifact semantics must not be split between shell writers and kernel
  validators
- adapter parsing must not be split between Bash and Rust
- framework-owned JSON interpretation must not be split between Rust and `jq`
- Nix typing gaps must not be deferred into Rust if they can be rejected during
  module evaluation
- validation must not be split between the kernel and a second framework-owned
  validation engine

## New Core Units

This RFC introduces these new framework-owned units:

- `nixfied/modules/contracts.nix`
- `nixfied/modules/lib/contract-options.nix`
- `nixfied/modules/lib/api-options.nix`
- `nixfied/compiler/compile-api-catalog.nix`
- `nixfied/compiler/compile-runtime-manifest.nix`
- `nixfied/compiler/compile-validation-ir.nix`
- `nixfied/framework/runtime/kernel/`

The exact source layout inside `nixfied/framework/runtime/kernel/` depends on
the implementation language, but the Nix packaging and manifest boundary are
part of this RFC.

## Core Rules

### Rule 1: One Command API Model

The framework currently has at least three overlapping schemas:

- task `contract`
- shell `appContract`
- app-level `validation.contractRef`

These must be replaced by one canonical `commandApi` model in Nix.

### Rule 2: One Runtime Artifact Writer

Run records, registry events, summaries, machine-output failure envelopes, and
service-export payloads must be written by `nixfied-kernel`, not assembled by
shell and validated afterward.

### Rule 3: One JSON Parser

Framework-owned JSON parsing must live in `nixfied-kernel`. Shell may pass
bytes and file paths, but it must not interpret framework JSON or adapter JSON.

### Rule 4: Closed Contracts By Default

Framework-owned runtime artifacts are closed by default. Open records are only
allowed at explicit adapter edges and must be called out as such.

### Rule 5: Shell Is Not The Semantics Layer

Shell may orchestrate. It may not define API meaning, state transitions, or
schema behavior.

### Rule 6: Kernel Scope Stays Narrow

The kernel exists to harden runtime semantics, JSON interpretation, adapter
decoding, and mutable artifact/state handling. It must not become a second
compiler, module evaluator, or public flake-surface generator.

### Rule 7: One Validation Engine

Framework-owned validation lives in `nixfied-kernel validate-*` over
Nix-compiled validation data. No second framework-owned validator path is
allowed.

## Replacement Map

### A. Module And API Typing Replacements

- `nixfied/modules/core.nix` at `nixfied.contracts.definitions`
  Replace `t.attrsOf t.anything` with a typed contract-definition option tree.
  Add `nixfied/modules/contracts.nix` and `nixfied/modules/lib/contract-options.nix`
  so invalid contract shapes fail during module evaluation instead of during
  later normalization.

- `nixfied/framework/contracts/default-definitions.nix`
  Replace loose string-heavy artifact fields with refined helpers and tagged
  unions. Add reusable contract helpers for RFC3339 UTC timestamps, stable ids,
  state enums, and closed tagged records. The current `runtime.registryEvent.detail`
  open record must be replaced with explicit variants such as `slotLifecycle`,
  `serviceLifecycle`, `workflowLifecycle`, `taskLifecycle`, and `controlSignal`.

- `nixfied/modules/apps.nix`
  Replace the current bag-of-fields app declaration with a canonical
  `nixfied.api.commands` authoring surface defined through typed submodules in
  `nixfied/modules/lib/api-options.nix`. `nixfied.apps` becomes compiled output
  only, not a handwritten authoring surface. The target is that a `taskRef`
  command cannot carry `workflowId`, a `machineOutput` command cannot omit its
  target, and invalid field combinations are impossible at the option layer.

- `nixfied/modules/tasks.nix`
  Replace the current split between `runner`, `contract`, and scattered runtime
  API details with:
  - a tagged `runner` union
  - a canonical `api` section
  - an `execution` section for runtime policy only
  The task option layer should describe one authoritative command surface rather
  than a mix of execution and contract fragments.

- `nixfied/framework/runtime/helpers/app-api.nix`
  Delete this hand-authored second API DSL and replace it with generated API
  projections emitted from the canonical `commandApi` model and compiled API
  catalog.

- `nixfied/project/module.nix`
  Replace the handwritten `contract = { ... };` assembly in task constructors
  with helpers that construct the canonical `commandApi` model directly. Project
  tasks should not manually duplicate framework API shape rules.

- `nixfied/modules/operations.nix`
  Replace handwritten operation-task contract assembly with the same canonical
  API constructors used by project tasks. Framework-owned operations should not
  be a separate contract authoring path.

- `nixfied/framework/runtime/helpers/service-api.nix` and
  `nixfied/framework/runtime/services/public-surface.nix`
  Replace the current split between module-derived service APIs and handwritten
  public service surfaces with one compiler-owned service operation catalog in
  Nix. Runtime launchers should be generated projections from that catalog.

- `nixfied/compiler/compile-apps.nix`
  Replace the current string-heavy normalization pass with a typed API compiler.
  Add `nixfied/compiler/compile-api-catalog.nix` and make `compile-apps.nix`
  consume typed refs and emit launch surfaces from the canonical API catalog,
  not from loosely related task/app/workflow attrs.

- `nixfied/compiler/finalize-model.nix`
  Extend the exported model with a canonical API catalog and runtime manifest
  references. Runtime launchers should consume compiled API/manifests directly
  instead of re-deriving semantics from `tasks`, `workflows`, and `apps`.

- `nixfied/compiler/compile-contract-bundle.nix`
  Keep this pass, but replace the current permissive compilation with a linted
  contract compilation step that rejects:
  - accidental open records
  - undefined refs
  - untagged unions where a tagged union is required
  - generic string use where a refined scalar helper exists
  This pass or a sibling pass must also emit a kernel-facing validation IR so
  runtime validation no longer depends on generated CUE.

- `nixfied/contracts/render-cue.nix`
  Delete this file. CUE generation is removed from the framework.

- `nixfied/framework/contracts/mkValidator.nix`
  Delete this shell wrapper and replace it with kernel validation commands over
  compiled validation IR.

### B. Runtime Kernel Replacements

- `nixfied/framework/runtime/executor.nix`
  Replace the current shell execution engine with a thin shell runner
  coordinated by `nixfied-kernel` semantic commands rather than a shell-owned
  state engine. The kernel must own:
  - summary writing
  - run outcome computation
  - machine-output validation
  - event emission
  - workflow step accounting
  `executor.nix` should retain env staging, process launch, stdout/stderr
  capture, and signal forwarding for project workloads rather than becoming a
  second semantic interpreter.

- `nixfied/framework/runtime/orchestrator-runtime.nix`
  Remove canonical run-record JSON construction and `.fields` sidecar semantics
  from shell. If helper functions remain here, they should be limited to launch
  wiring and argument transport.

- `nixfied/framework/runtime/orchestrator.nix`
  Replace the current shell-owned run lifecycle implementation with a thin
  orchestration wrapper around `nixfied-kernel` state commands. Shell may still
  spawn and signal processes, but run-id envelopes, run-record creation,
  transitions, reconciliation, and terminal-state derivation must move into the
  kernel. The kernel must not become a generic project workload runner.

- `nixfied/framework/runtime/orchestrator-control.nix`
  Replace the duplicated run-record mutation logic with the same kernel state
  commands used by `orchestrator.nix`. This file should not retain a parallel
  run-record writer or updater.

- `nixfied/framework/runtime/registry/events-append.nix`
  Replace shell JSON construction and append semantics with `nixfied-kernel registry append`.
  The kernel must own sequence allocation, envelope construction, validation,
  and append discipline. Shell should only provide root paths and locks if
  lock handling is not also moved into the kernel.

- `nixfied/framework/runtime/registry/replay.nix`
  Replace the shell replay reader with `nixfied-kernel registry replay`.
  Replay semantics should be typed and share the same event model as append.

- `nixfied/framework/runtime/helpers/runtime-events.nix`
  Split this file into:
  - shell path/discovery helpers that may remain
  - kernel-backed event and status helpers that replace current semantic logic
  The runtime helper layer must stop being a second state engine.

- `nixfied/framework/runtime/helpers/run-registry.nix`
  Replace the parallel `meta.json` and `meta.fields` writer with the same
  kernel-backed run-record/state path used by the orchestrator or delete this
  helper if the orchestrator state model subsumes it.

- `nixfied/framework/runtime/helpers/summary.nix`
  Replace summary rendering based on `summary.fields` and `summary.steps.tsv`
  sidecars with `nixfied-kernel summary render-human`. Sidecars may remain as
  derived caches for grep, but the semantic source for the summary surface must
  be the validated `summary.json` envelope.

- `nixfied/framework/core/mkMachineOutputPrograms.nix`
  Replace the current shell body with a thin wrapper around
  `nixfied-kernel machine-output run`. The kernel should own:
  - declared payload-file channel handling
  - payload validation
  - stable failure envelope emission
  - setup/target/teardown result classification

### C. Input Validation Replacements

- `nixfied/framework/runtime/helpers/shell-contract.nix`
  Replace the 1300+ line shell runtime validator with a much smaller launcher
  helper. Argument parsing, env resolution, JSON validation, enum checks, and
  failure-code semantics must move into `nixfied-kernel validate-input`.
  The target state is that the current semantic validator is deleted and
  replaced by a thin generated launcher runtime with no JSON parsing and no API
  semantics.

- `nixfied/framework/runtime/helpers/env-loader.nix`
  Replace typed value validation, especially JSON validation, with
  `nixfied-kernel validate-scalar`. Shell may continue to read `.env` files line
  by line, but type interpretation must no longer depend on `grep`/`jq`.

- machine payload and artifact contract enforcement
  Replace the current generated external validator path with
  `nixfied-kernel validate-payload` and `nixfied-kernel validate-artifact`
  operating on compiled validation IR from Nix.

- `nixfied/framework/runtime/common-runtime.nix`
  Delete framework JSON string builders and keep only generic shell/process
  helpers. Framework JSON assembly should not survive here under a different
  name.

- `nixfied/framework/runtime/helpers/helpers.nix`
  Keep generic shell helpers such as waiting, artifact path resolution, and log
  capture. Replace any remaining framework-specific semantic validation helpers
  with kernel calls.

### D. Adapter And `jq` Replacement Map

- `nixfied/framework/runtime/helpers/probe-commands.nix`
  Replace generic JSON-RPC field extraction with typed helper commands in
  `nixfied-kernel probe` or `nixfied-kernel jsonrpc`. Shell should not be able
  to ask for arbitrary jq expressions over framework-owned probe paths.

- `nixfied/framework/runtime/helpers/probe-plan-runtime.nix`
  Keep Nix-side probe-plan generation, but replace runtime execution and JSON
  interpretation with kernel-backed probe execution. The target is:
  - Nix defines the plan
  - the kernel executes and interprets it
  - shell only wires inputs together

- `nixfied/framework/runtime/services/helios/lifecycle.nix`
  Replace direct `jq` parsing of consensus API payloads with a typed Helios
  adapter owned by the kernel. The kernel should expose a narrow function or
  subcommand that extracts:
  - finalized slot
  - epoch checkpoint root
  and validates those values against explicit adapter contracts.

- `nixfied/framework/runtime/services/supervisor/status.nix`
  Replace `jq` parsing of `process-compose` JSON with a typed supervisor adapter
  in the kernel. Health evaluation should consume a typed process list model,
  not raw jq filters embedded in shell.

- `nixfied/framework/runtime/helpers/discovery.nix`
  Keep repository discovery shell if desired, but move any future JSON emission
  normalization into the canonical API/contract layer. This file should not grow
  new framework semantics while the core is being hardened.

- `nixfied/framework/core/mkCoreSurfaces.nix`
  Replace `jq`-based build assertions with Nix `builtins.fromJSON` assertions or
  kernel-asset build checks. Build-time correctness checks should not require
  either `jq` or CUE.

### E. Contract Tightening Replacements

- `runtime.runRecord.payload` in `nixfied/framework/contracts/default-definitions.nix`
  Replace bare `nonEmptyString` fields with refined scalars where possible.
  `execution_mode` should become an enum, timestamps should use a dedicated
  timestamp helper, and ids should use dedicated refined scalar helpers.

- `runtime.summary` in `nixfied/framework/contracts/default-definitions.nix`
  Tighten timing and step contracts so the human summary surface and the machine
  summary surface both come from one validated envelope. If sidecars remain,
  they must be derived from the validated envelope, not treated as a second API.

- `runtime.registryEvent.detail` in `nixfied/framework/contracts/default-definitions.nix`
  Replace the open record with a tagged union. This is one of the biggest
  remaining soft spots because it lets the most important event payload in the
  system evolve without schema discipline.

- `runtime.registryEvent.produces` in `nixfied/framework/contracts/default-definitions.nix`
  Replace the open record with a closed record with explicit optional fields.
  Unknown fields here should not silently pass validation.

### F. Test And Policy Replacements

- `tests/framework/contract-migration-guard.nix`
  Replace the current exact-site jq allowlist with a new `core-hardening-guard`
  that enforces:
  - no framework-owned `jq` in runtime or service adapter code
  - no framework-owned CUE generation or validator path
  - no framework-owned Python helper reintroduction
  - no new shell JSON semantics in executor/orchestrator/kernel-bound paths

- `tests/framework/*` contract and smoke coverage
  Add new tests that prove:
  - invalid module contract shapes fail during evaluation
  - invalid app/task API mixtures fail during evaluation
  - compiled validation IR matches the contract source model
  - run records, summaries, and events are written by the kernel path only
  - third-party adapter failures are rejected before state mutation
  - no framework-owned CUE path remains
  - no framework-owned jq sites remain

## Pieces That Should Stay

The hardening work should not rewrite everything.

- `nixfied/compiler/default.nix` and the pure pass pipeline should stay.
- `nixfied/compiler/compile-app-execution-manifests.nix` is already close to
  the right seam and should be extended or absorbed into compiled runtime
  manifest work, not discarded casually.
- `nixfied/contracts/default.nix`, `nixfied/contracts/types.nix`, and
  `nixfied/contracts/render-json-schema.nix` should stay and be strengthened,
  not replaced.
- `nixfied/framework/introspection/runtime.nix` is already relatively thin and
  should stay thin. It may need API-catalog integration, but it does not need a
  ground-up rewrite unless it starts accumulating new semantics.
- `nixfied/framework/runtime/dispatcher.nix` is already mostly a thin launcher
  surface and should stay thin.
- public launch surfaces may change if that materially simplifies and hardens
  the framework. They should not be preserved by compatibility shims.

## Required `nixfied-kernel` Responsibilities

The kernel introduced by this RFC must support at least these internal
operations. These are narrow runtime-semantic operations. They do not replace
Nix compilation, module evaluation, or generic project workload
implementation:

- `validate-input`
- `validate-scalar`
- `validate-payload`
- `validate-artifact`
- `machine-output run`
- `run-record create`
- `run-record transition`
- `registry append`
- `registry replay`
- `summary write`
- `summary render-human`
- `probe evaluate` for framework-owned probe plans
- `adapter decode` for explicitly approved third-party JSON boundaries

These are internal operations. They may sit behind public launchers, but they do
not constrain which public launchers or names survive the hardening work.

## Phased Delivery

### Phase 1: Tighten Module And Contract Types

- add typed contract option modules
- replace `t.anything` on framework-owned contract paths
- delete CUE generation and validator wrappers
- add compiled validation IR for kernel validation
- tighten runtime artifact contracts

### Phase 2: Unify The API Model

- replace duplicated task/app API schemas with canonical `commandApi`
- add compiled API catalog
- make framework task constructors emit canonical API data

### Phase 3: Introduce Kernel State And Artifact Writers

- migrate run-record creation and transitions
- migrate registry append and replay
- migrate summary writing

### Phase 4: Replace Shell Input Validation And Machine Output Semantics

- migrate `shell-contract.nix`
- migrate `.env` typed validation
- migrate machine and artifact validation to `validate-payload` and
  `validate-artifact`
- migrate machine-output execution and failure envelopes

### Phase 5: Replace Adapter Parsing

- migrate probe JSON-RPC parsing
- migrate Helios JSON parsing
- migrate supervisor JSON parsing

### Phase 6: Remove Dead Shell Semantics And Tighten Guards

- delete or shrink obsolete semantic shell code
- delete obsolete validation code and compatibility code rather than preserving
  shims
- replace jq allowlist tests with zero-jq core policy
- cap remaining shell files to thin-wrapper roles only

## Acceptance Criteria

The RFC is not complete until all of the following are true:

- no framework-owned machine-contract option path uses `t.anything`
- no framework-owned CUE code, CUE packages, or CUE validator path remains
- no framework-owned runtime or service adapter path uses `jq`
- `tasks.contract` and `appContract` no longer exist as parallel schemas
- run records, summaries, events, and machine-output failure envelopes are
  written by `nixfied-kernel`
- executor and orchestrator shell files are thin wrappers rather than semantic
  state engines
- remaining shell runtime code stages env, paths, process launch, and signal
  forwarding only
- `runtime.registryEvent.detail` is a tagged union, not an open record
- machine payload and artifact validation runs through `nixfied-kernel
  validate-*`
- `nixfied-kernel` does not duplicate module evaluation, compiler passes, or
  public launcher generation
- no compatibility shims remain for removed validation or launcher paths
- framework guards fail if new shell JSON semantics are introduced

## Why This RFC Is Worth The Disruption

The current framework is already disciplined at compile time, but its core is
soft where it matters most:

- state mutation
- runtime API interpretation
- JSON parsing
- adapter semantics

This RFC hardens exactly those seams.

It goes further than the previous validation work by removing CUE from the
architecture entirely, deleting legacy validator paths, and moving all
framework-owned validation and runtime semantics into a small typed runtime
kernel.

## Appendix A: Commit Execution Plan

This appendix defines the execution order for the refactor.

- Land changes per commit, not per PR.
- Use parallel worktrees or parallel branches with disjoint write scopes.
- Merge each commit as soon as its slice is green.
- Do not preserve legacy code once the replacement path lands.
- When a barrier commit lands, immediately rebase dependent lanes on it.

### Lane Model

- Lane F: foundations and shared compiler wiring
- Lane A: authoring, typing, and API compilation
- Lane K: kernel validation and state machinery
- Lane R: runtime launcher/orchestrator rewiring
- Lane D: adapter migration
- Lane G: guards, deletions, and final cleanup

### Progress Snapshot (2026-03-24)

- Wave 0 is landed.
- Wave 1 is landed for the RFC-critical surfaces that motivated this document.
- Wave 2 is landed.
- Wave 3 is landed.
- Wave 4 is landed.
- Wave 5 is landed.
- Wave 6 is landed.

### Wave 0: Shared Foundations

1. `commit 01` in Lane F
   Add `nixfied/framework/runtime/kernel/` with a packaged `nixfied-kernel`
   binary skeleton and subcommand surface for `validate-*`, `run-record`,
   `registry`, `summary`, `probe`, and `adapter`.

2. `commit 02` in Lane F
   Add `nixfied/compiler/compile-validation-ir.nix` and export kernel-facing
   validation IR from the compiled core/model.

3. `commit 03` in Lane F
   Wire kernel packaging and validation IR into the Nix surfaces that need to
   materialize runtime assets.

Barrier: all later lanes rebase on `commit 03`.

### Wave 1: Parallel Type, Contract, and API Hardening

4. `commit 04` in Lane A
   Replace `nixfied.contracts.definitions` `t.anything` usage with typed
   contract option modules.

5. `commit 05` in Lane A
   Tighten framework runtime artifact contracts in
   `nixfied/framework/contracts/default-definitions.nix`, especially
   `runtime.runRecord.payload`, `runtime.registryEvent.detail`, and
   `runtime.registryEvent.produces`.

6. `commit 06` in Lane A
   Add canonical `commandApi` authoring options and remove the handwritten app
   bag-of-fields authoring model.

7. `commit 07` in Lane A
   Add `compile-api-catalog.nix` and `compile-runtime-manifest.nix`, then make
   model finalization export canonical API/runtime references.

8. `commit 08` in Lane A
   Migrate `nixfied/project/module.nix`, `nixfied/modules/operations.nix`, and
   related helpers to emit canonical `commandApi` data directly.

9. `commit 09` in Lane A
   Consolidate `service-api.nix` and `services/public-surface.nix` into one
   compiler-owned service operation catalog.

10. `commit 10` in Lane G
    Add hardening guards that ban new framework-owned CUE, jq, Python, and shell
    JSON semantics while the migration is underway.

Barrier: `commit 04` through `commit 10` must land before shell/runtime rewiring
starts.

### Wave 2: Parallel Kernel Validation and CUE Removal

11. `commit 11` in Lane K
    Implement `nixfied-kernel validate-input` and `validate-scalar` against the
    compiled validation IR.

12. `commit 12` in Lane K
    Implement `nixfied-kernel validate-payload` and `validate-artifact` against
    the compiled validation IR.

13. `commit 13` in Lane G
    Delete `nixfied/contracts/render-cue.nix`, delete
    `nixfied/framework/contracts/mkValidator.nix`, and remove CUE from package
    wiring and framework dependencies.

Barrier: `commit 11` through `commit 13` must land before any shell path stops
calling the old validators.

### Wave 3: Parallel Runtime State Replacement

14. `commit 14` in Lane K
    Implement `nixfied-kernel run-record create` and `run-record transition`.

15. `commit 15` in Lane K
    Implement `nixfied-kernel registry append` and `registry replay`.

16. `commit 16` in Lane K
    Implement `nixfied-kernel summary write` and `summary render-human`.

17. `commit 17` in Lane R
    Rewire `orchestrator-runtime.nix`, `orchestrator.nix`, and
    `orchestrator-control.nix` to use kernel run-record and registry commands.

18. `commit 18` in Lane R
    Rewire `runtime-events.nix`, `run-registry.nix`, and summary helpers to use
    kernel state and summary commands.

Barrier: `commit 14` through `commit 18` must land before deleting old shell
state writers.

### Wave 4: Parallel Input and Machine-Output Migration

19. `commit 19` in Lane R
    Replace `shell-contract.nix` with a thin launcher path over
    `nixfied-kernel validate-input`.

20. `commit 20` in Lane R
    Replace typed `.env` validation with `nixfied-kernel validate-scalar` and
    remove framework JSON builders from `common-runtime.nix`.

21. `commit 21` in Lane R
    Rebuild `mkMachineOutputPrograms.nix` around `nixfied-kernel
    machine-output run`.

Barrier: `commit 19` through `commit 21` must land before deleting the old
shell validation/runtime helpers they replace.

### Wave 5: Parallel Adapter Replacement

22. `commit 22` in Lane D
    Implement kernel-backed probe evaluation and replace generic JSON-RPC field
    extraction in `probe-commands.nix` and `probe-plan-runtime.nix`.

23. `commit 23` in Lane D
    Implement the Helios adapter in the kernel and remove jq parsing from
    `services/helios/lifecycle.nix`.

24. `commit 24` in Lane D
    Implement the supervisor adapter in the kernel and remove jq parsing from
    `services/supervisor/status.nix`.

Barrier: `commit 22` through `commit 24` must land before final jq removal.

### Wave 6: Final Deletion and Simplification

25. `commit 25` in Lane G
    Delete dead shell semantic code in executor/orchestrator/helper paths that
    are now redundant.

26. `commit 26` in Lane G
    Remove remaining jq usage from framework runtime and build-check paths,
    including `mkCoreSurfaces.nix`.

27. `commit 27` in Lane G
    Rewrite guards and tests to enforce the final rules:
    no CUE, no jq, no compatibility shims, no shell JSON semantics, kernel-only
    validation and state mutation.

28. `commit 28` in Lane G
    Final simplification pass: delete transitional code, collapse dead helpers,
    update docs, and record the post-refactor LOC snapshot.

### Parallelism Rules

- Wave 0 is serial.
- Within Waves 1 through 5, commits in different lanes should be developed in
  parallel whenever their write scopes do not overlap.
- Within a single lane, preserve the listed commit order.
- Do not start a dependent wave before its barrier commits have landed.
- If two lanes converge on the same file, stop parallelization at that seam and
  merge the narrower change first.

### Commit Discipline

- Every commit must either add a new hard path or delete an old soft path.
- Avoid commits that introduce a new path without naming the deletion commit
  that will immediately follow.
- Prefer commits that end in one less technology, one less validator path, one
  less shell semantic helper, or one less duplicated schema.
- If a commit cannot be explained as a narrower ownership move, it is too
  broad.

### Closure Note (2026-03-24)

The short serial finish described earlier is complete:

1. Lane R shell JSON removal is landed.
   `executor.nix`, `orchestrator.nix`, and `runtime-events.nix` no longer
   assemble framework-owned run-envelope or event/control JSON in shell.

2. Lane R thin-wrapper reduction is landed.
   `shell-contract.nix` is on the kernel validation path and
   `common-runtime.nix` no longer contains framework JSON builders.

3. Lane D probe execution migration is landed.
   The JSON-RPC probe request/evaluate flow is kernel-owned end to end.

4. The contract-tightening tail is landed.
   `runtime.summary.payload` is refined to stable ids, workflow-mode enums, and
   UTC timestamps.

5. Lane G final cleanup is landed for the hardened-core/build-check boundary.
   Framework build-check paths are `jq`-free and the guard enforces that seam.

6. The RFC is now closed.
   Any future follow-up should be treated as new work, not as an extension of
   the open hardening tail recorded by this document.

### Reopen Note (2026-03-24)

The closure note above is preserved as historical context, but it is no longer
accurate for the current worktree.

Post-closure review found that:

1. Lane R thin-wrapper reduction is incomplete.
   The executor path still retains shell-owned workflow scheduler semantics
   beyond env/path/process/signal staging.

2. Lane G guard enforcement is incomplete.
   The guard does not yet fail on the remaining executor-owned shell workflow
   scheduler semantics that this RFC intended to prohibit inside the
   hardened-core runtime boundary.

The RFC is therefore reopened until those items are implemented and the guard
and tests enforce the final boundary in substance rather than by status text.

## Appendix B: Target LOC Snapshot

Use this as the recorded target snapshot after the refactor lands.

```text
❯ loc
--------------------------------------------------------------------------------
 Language             Files        Lines        Blank      Comment         Code
--------------------------------------------------------------------------------
 Nix                    327        59125         5175         2872        51078
 Rust                     1         3791          337            0         3454
 JSON                     5         1478            0            0         1478
 Markdown                 8         1150          267            0          883
 Plain Text               5           83           11            0           72
--------------------------------------------------------------------------------
 Total                  346        65627         5790         2872        56965
--------------------------------------------------------------------------------
```
