# RFC v2 Implementation Plan: Pre-M1 Through M7

Source of truth for scope and architecture remains `RFC_v2.md`.

## Current Progress Checkpoint

Status reflects the `v2` branch as of 2026-06-09. The full capability line (M1-M6,
M7 deferred) and the consolidation phase (C1, C2) are implemented and verified green:
`cargo test`, `cargo clippy -D warnings`, `nix flake check`, every `tests/*` proof, and
`nix run .#conformance` (5/5) all pass.

| Milestone | State | Evidence |
| --- | --- | --- |
| M0 walking skeleton | Done | `tests/m0/*`, full runtime spine |
| Pre-M1 view surfaces | Done | `tests/pre-m1/prove-view-surfaces.sh` |
| M1 slot isolation | Done | `tests/m1/prove-slot-isolation.sh` |
| M2 cancellation & GC hardening | Done | `tests/m2/{prove-cancellation,prove-gc-hardening,prove-lifecycle-ops}.sh` |
| M3 Nix-side Postgres adapter | Done | `nix/adapters/postgres.nix`, `tests/m3/prove-postgres-adapter.sh` |
| M4 workflow graphs | Done | `examples/workflow`, `tests/m4/prove-workflow-graphs.sh` |
| M5 installable downstream wrapper | Done | `nix/install/upgrade.sh`, `tests/m5/prove-install-upgrade.sh` |
| M6 polyglot example | Done | `examples/polyglot-stack`, `tests/m6/prove-polyglot-stack.sh` |
| M7 optional manifest envelope | Deferred by decision | none |
| C1 downstream conformance suite | Done | `nix run .#conformance`, `runtime/crates/nixfied-conformance` |
| C2 de-milestone product surface | Done (product surface); test-layout tail open | `tests/guard-no-milestone-tokens.sh` |

Notes:

- **A foundation generalization preceded M3 and was not anticipated by the original plan.**
  Despite M1/M2 being marked Done, the product was still a single hardcoded
  `synthetic`/`smoke`/`m0-helper` fixture: `validation.rs` was exact-match, the Nix surface
  emitted only that fixture, and the runtime selected services/tasks by hardcoded name. M3
  therefore first replaced exact-match validation with a structural contract, introduced a
  generic Nix declaration surface (`closures`/`execs`/`services`/`tasks` + `nix/adapters/`),
  and made the runtime drive arbitrary multi-service/multi-task models. See M3 below.
- `tests/m3`, `tests/m4`, `tests/m5`, and `tests/m6` directories now exist with their proofs.
  `tests/m7` is intentionally absent (M7 deferred).
- Two capability-adjacent runtime changes were required and landed generically (no
  service-specific code): a `process-tree` containment mode (multi-process supervisors like
  Postgres whose children form their own process groups) and multiple services per run
  (idempotent run/lease registry rows). Both are documented in M3/M6 below.
- **Open C2 tail (test-layout only):** the interim shell proofs still live under
  `tests/m0|pre-m1|m1|m2|m3|m4|m5|m6/` with milestone names and the runtime unit tests are
  still `m0_*.rs` files. The product/consumer surfaces are fully de-milestoned and guarded;
  folding those shell proofs into C1 scenarios and renaming the test files/dirs is the only
  remaining piece. The guard (`tests/guard-no-milestone-tokens.sh`) is scoped to product
  surfaces and treats the milestone-named test scaffolding as development history.
- M7 stays deferred until a concrete need (portable bundles, cache export/import,
  standalone distribution outside the Nix store, or integrity-bound materialised views)
  appears.

## Agenda

This continuation plan covers milestone work through M7 and keeps M0 constraints.

### Scope

*1. M1: slots, per-slot state pathing, and registry partitioning. (done)*
*2. M2: cancellation, lease reconciliation, cleanup hardening, and generic lifecycle. (done)*
*3. M3: Nix-side reference adapter, minimal postgres. (done)*
*4. M4: workflow graphs. (done)*
*5. M5: installable downstream wrapper hardening. (done)*
*6. M6: polyglot example. (done)*
*7. M7: optional manifest envelope (only if triggered by concrete need). (deferred)*

Consolidation: *C1 conformance suite (done), C2 de-milestone product surface (done;
test-layout fold/rename open).*

After the capability line (M1-M6; M7 deferred) is complete, a **Consolidation Phase**
(C1-C2) retires development scaffolding from the product: a first-class downstream
conformance suite replaces the ad hoc shell proofs, and a de-milestoning pass removes
`mX` vocabulary from every consumer-observable surface. See "Consolidation Phase" below.

The optional runtime adapter protocol remains deferred beyond this plan's main M1-M7
line. It is included as an appendix because RFC v2 lists it as a possible future
milestone if direct generic runtime execution becomes insufficient.

## Pre-M1: Generic View Surfaces (Done)

### Product Purpose

Repair the generated view surface contract so `schema`, `docs`, and `capabilities`
remain disposable projections derived from `model.json`, with model-derived view
commands in the CLI.

### Delivered

- Model-derived view commands (`cli: add model-derived view commands`).
- Repaired generated view surface contract (`surfaces: repair generated view surface contract`).
- Proof that views are disposable projections (`tests: prove views are disposable projections`).

### Proof Script

- `tests/pre-m1/prove-view-surfaces.sh`

## M1: Explicit Slot Constraints And Per-Slot State (Done)

### Product Purpose

Enable two slots of the same environment in the same checkout with deterministic
placement and registry isolation, while preserving M0 runtime boundaries.

### Strict Boundary

Included:

- Slot policy surface updates for admission and state path derivation.
- Per-slot state roots under `state/<project>/<environment>/<slot>`.
- Per-slot registry file ownership and process records.
- Deterministic port windows and slot-local readiness/port reservations.

Excluded:

- Multi-project or multi-environment orchestration beyond what M1 explicitly scopes.
- Cross-slot service reuse.
- Runtime protocol changes outside registry/process/cleanup scope.

### `Runtime` / `nixfied-model` Changes

Minimal schema and admission changes already made in prior steps for slot-aware model
interpretation and per-slot path materialization.

### `Nix` Module / Compiler Changes

- `slotPolicy` values (`min`, `default`, `max`) drive slot materialization.
- `slotPlacements` are emitted when slot cardinality > 1.
- Validation remains exact for M0/M1 constants where applicable.

### Model Invariants

- `slotPolicy.min <= slotPolicy.default <= slotPolicy.max`.
- Slot indices are contiguous from 0..max.
- Per-slot placement must remain deterministic and host-absolute free.

### `nixfied-runtime` Changes

- Admission validates the requested slot against policy and constraints.
- Registry/state paths are materialized as slot-specific roots.
- `ps` and `down` are slot-aware.

### Risks And Mitigations

Risk: slot drift in placement leads to collision or registry aliasing.

Mitigation: deterministic slot windows and per-slot state root path derivation.

### Proof Script

- `tests/m1/prove-slot-isolation.sh`

### Dependencies

- Requires M0 runtime model and registry shape.

## M2: Cancellation, Lease Reconciliation, And GC Hardening (Done)

### Product Purpose

Harden failure and lifecycle semantics before the first real stateful adapter, so
adapter complexity cannot accumulate on top of unproven cancellation/GC. Add a full
generic lifecycle operation contract the runtime executes without service-specific code.

### Strict Boundary

Included:

- Cancellation tokens and signal handling for runs, services, and tasks.
- Registry leases and `canceled` terminal states.
- Crash-safe cleanup, stale-reservation reclamation, and marker-gated GC refusal.
- A generic lifecycle operation contract (start/ready/health/stop/clean) the runtime
  dispatches against model primitives.

Excluded:

- Concrete adapters (deferred to M3).
- Workflow graph scheduling (deferred to M4).

### Delivered

- Cancellation tokens and signal handling (`runtime: add cancellation tokens and signal handling`).
- Leases and canceled terminal states (`registry: add leases and canceled terminal states`).
- Crash-safe cleanup and stale reservation hardening (`state: harden crash-safe cleanup and stale reservations`).
- Full generic lifecycle operation contract (`model: add full generic lifecycle operation contract`).
- Runtime execution of generic health and clean lifecycle ops
  (`runtime: execute generic health and clean lifecycle ops`).

### Proof Scripts

- `tests/m2/prove-cancellation.sh` — readiness/task/CLI-signal cancellation, canceled
  terminal states, lease-canceling `down` that unblocks cleanup, task-timeout summaries.
- `tests/m2/prove-gc-hardening.sh` — cleanup refuses unmarked roots, path escape, marker
  mismatch, protected state, and persistent state.
- `tests/m2/prove-lifecycle-ops.sh` — declared lifecycle class ordering, clean terminal
  events, and distinct post-ready health-failure recording.

### Dependencies

- Requires M1 per-slot registry/state isolation.

## M3: Nix-Side Reference Adapter — Minimal Postgres (Done)

### Product Purpose

Prove that a concrete service adapter (Postgres) is expressible purely as a Nix module
that generates generic model primitives, while `nixfied-runtime` stays generic and gains
no Postgres-specific code path.

### Delivered

Implemented in six commits, beginning with the foundation generalization the original plan
did not anticipate (the engine was still an exact-match `synthetic`/`smoke` fixture):

- `model: replace exact-fixture validation with structural contract` — `validation.rs` now
  validates structure + references for arbitrary service/task/closure/exec names; the model
  contract test suite was rewritten accordingly (including a test that the previously
  rejected multi-service/multi-exec shape is now accepted).
- `nix: generic service/task/closure declaration surface with adapters` — typed
  `nixfied.closures/execs/services/tasks` options in `nix/modules/primitives.nix`, generic
  `environments`, a data-driven `nix/compiler/derive.nix`, and `nix/adapters/` (a `synthetic`
  adapter plus `default.nix`) injected into every compiled module via `specialArgs.adapters`.
  The minimal example imports `adapters.synthetic`.
- `runtime: generic multi-service multi-task run and clean loop` — `main.rs` iterates the
  environment's services/tasks instead of hardcoded `synthetic`/`smoke`; `process.rs` gained
  generic `start_service_for_slot` / `run_slot_clean`.
- `runtime: add ${stateDir} placeholder for stateful execs` — generic placeholder
  substitution (`${port}`, `${stateDir}`) so a stateful service can locate its data dir
  under the runtime-materialised slot state root.
- `nix+runtime: postgres reference adapter with process-tree containment` —
  `nix/adapters/postgres.nix` emits generic primitives (one closure per `initdb`/`postgres`/
  `pg_ctl`/`psql`, since admission requires exec executable == closure executable);
  `examples/postgres`; flake `postgres-model`. A new generic `ContainmentRequirement::ProcessTree`
  contains supervisors whose children form their own process groups (Postgres), with no
  Postgres-specific runtime code. Unix sockets are disabled in the adapter (TCP only) to
  avoid the macOS `sun_path` length limit under deep state dirs.
- `tests: prove postgres adapter end to end (m3)`.

The runtime gained no Postgres-specific code path: `process-tree` containment and
`${stateDir}` are generic capabilities any multi-process/stateful service can use.

### Strict Boundary

Included:

- A Nix module that compiles a minimal Postgres service into existing generic primitives
  (`ClosureSpec`, `ExecSpec`, `EndpointSpec`, `ProbeSpec`, `ServiceSpec`, `TaskSpec`,
  lifecycle ops).
- Data-directory init, start, readiness via endpoint ownership, a dependent task
  (e.g. a smoke query), and clean shutdown — all through generic lifecycle ops.
- Persistent state handling that respects M2 marker/cleanup refusal rules.

Excluded:

- Any runtime adapter protocol or Postgres-aware runtime branch (RUNTIME-GENERIC-1 holds).
- Service reuse beyond what RFC SVC-ID-1 exact-match identity already allows.
- Replication, tuning, or multi-instance orchestration.

### `Nix` Module / Compiler Changes

- New adapter module emitting Postgres closures and primitives.
- Adapter must not be required by minimal (non-Postgres) projects.

### `nixfied-runtime` Changes

- None expected beyond generic primitive execution. Any required change is a signal the
  primitive set is incomplete and should be generalized, not specialized.

### Risks And Mitigations

Risk: Postgres specifics leak into the runtime.

Mitigation: keep all Postgres logic in the Nix-side adapter; assert the runtime diff is
empty or limited to generic primitive support.

### Proof Script

- `tests/m3/prove-postgres-adapter.sh`: builds `.#postgres-model`, the runtime drives
  `initdb` (prepare) -> `postgres` (start) -> TCP-ownership readiness -> health ->
  `psql SELECT 1` -> stop -> marker-gated clean, and asserts a minimal non-Postgres project
  (`.#minimal-model`) still compiles without the adapter.

### Dependencies

- Requires M2 lifecycle, lease, and cleanup hardening.

## M4: Workflow Graphs (Done)

### Product Purpose

Add dependency-aware workflows over the existing generic primitives: bounded tasks,
service requirements, readiness gates, cancellation, artifacts, summaries, and cleanup
policy.

### Delivered

- `model: promote workflows to a validated bounded task DAG` — `WorkflowSpec` is now
  `{ workflowId, servicesRequired, nodes[{ nodeId, taskId, dependsOn }] }`; validation
  enforces unique node ids, references to declared services/tasks, and acyclicity (Kahn
  reduction). `capabilities.workflows` mirrors the declared workflows.
- `nix: typed workflow surface and example` — typed `nixfied.workflows` options, `derive.nix`
  emission, relaxed `validate.nix`; `examples/workflow` + flake `workflow-model`.
- `runtime: schedule workflow node graphs with readiness gates` — `run --workflow <id>`
  starts the workflow's required services (readiness + health gate), runs nodes in
  topological order honoring `dependsOn`, reuses M2 cancellation tokens for teardown, and
  writes a per-workflow summary (`artifacts/workflow-<id>.json`). The default (non-workflow)
  run is the same engine over the environment's services/tasks.
- `tests: prove workflow graphs end to end (m4)`.

Cleanup policy reuses the slot's marker-gated `clean`; per-node artifact namespacing beyond
the aggregate workflow summary was not needed for the current examples.

### Strict Boundary

Included:

- `WorkflowSpec` schema in `nixfied-model` (currently rejected as unsupported).
- Nix module options and validation for workflow declarations.
- Runtime scheduling honoring service-requirement readiness gates and task dependencies.
- Cancellation propagation across the graph reusing M2 cancellation tokens.
- Per-workflow artifacts, summaries, and cleanup policy.

Excluded:

- Cross-project or distributed scheduling.
- Dynamic/loop-based graphs beyond a bounded dependency DAG.

### `Runtime` / `nixfied-model` Changes

- Promote `workflows` from "empty only / rejected" to a validated executable spec.
- Add workflow run/event records to the registry under existing total ordering.

### Model Invariants

- Workflow graphs are acyclic and bounded.
- Every task dependency and service requirement resolves to a declared primitive.

### Risks And Mitigations

Risk: workflow scheduling reintroduces service-specific runtime logic.

Mitigation: schedule only generic primitives and lifecycle ops; gate on readiness the
same way single services do.

### Proof Script

- `tests/m4/prove-workflow-graphs.sh`: a two-node workflow (`probe` -> `verify`) with a
  service requirement runs in dependency order, both nodes succeed, and a workflow summary
  is written; marker-gated clean removes the slot state.

### Dependencies

- Requires M2 cancellation/lease semantics and M3 generic-adapter confidence.

## M5: Installable Downstream Wrapper Hardening (Done)

### Product Purpose

Make adoption and upgrade safe through Nixfied-owned flake input/import shims that never
overwrite project-owned declarations.

### Delivered

- `m5: add non-destructive nixfied input upgrade surface` — `nix/install/upgrade.sh` and
  `nix run .#upgrade` repin the project's `nixfied` flake input and refresh only that lock
  entry. It refuses a directory with no `flake.nix` / no nixfied input, and it never creates
  or edits the project-owned `nixfied.nix`, enforcing the shim boundary (Nixfied owns the
  input/import wiring; the project owns every semantic declaration).
- `install.sh` was updated so a freshly scaffolded `nixfied.nix` imports `adapters.synthetic`
  (a runnable starter), so the scaffold compiles a valid model under the generalized surface.

### Proof Scripts

- `tests/m5/prove-install-upgrade.sh`: install with an unbuildable placeholder pin, make a
  project-owned edit to `nixfied.nix`, repin to the local checkout, assert `nixfied.nix` is
  byte-identical, then compile a Nix-store model. Refusing a directory with no `flake.nix`
  is also covered.
- `tests/m0/prove-install-scaffold.sh` continues to cover the create + refusal scaffold
  paths.

### Dependencies

- Independent of M3/M4; proceeded in parallel (landed first).

## M6: Polyglot Example (Done)

### Product Purpose

Provide `examples/polyglot-stack` demonstrating multiple codebases/services composed
through generic primitives and (once available) workflows.

### Delivered

- `runtime+nix: allow multiple services per run and add polyglot example` —
  `examples/polyglot-stack` declares a Python service (`api`) and a Perl service (`worker`),
  each with a dependent task, all through the generic surface; flake `polyglot-stack-model`.
  Running it required relaxing the registry's single-service-per-run assumption: the `runs`
  and `run_leases` rows are now created idempotently (`INSERT OR IGNORE`) and the
  existing-run refusal was dropped (run ids are unique per invocation), keeping the
  per-instance lease/service/port safety gates. The run loop assigns each service a distinct
  port (`window.start + index`) within the slot window.
- `tests: prove polyglot stack end to end (m6)`.

### Strict Boundary

Included:

- A downstream-shaped example exercising public surfaces only.
- Multiple services/tasks across languages composed via the model.

Excluded:

- Any new runtime capability; the example must use already-shipped primitives.

### Proof Script

- `tests/m6/prove-polyglot-stack.sh`: builds `.#polyglot-stack-model`, runs both services on
  distinct ports, asserts both tasks succeed (`python-ok` / `perl-ok`), and marker-gated
  clean removes the slot state.

### Dependencies

- Strongest after M3 (adapter) and M4 (workflows) so the example is representative.

## M7: Optional Manifest Envelope (Deferred)

### Trigger Condition

Implement only if a concrete need appears: portable compiled-output bundles, cache
export/import, standalone distribution outside the Nix store, or integrity-bound
materialised views.

### Strict Boundary

- A minimal, non-semantic manifest envelope that binds artifact bytes only.
- `model.json` remains the sole semantic authority; the manifest must not become a
  second source of truth.

### Proof Script

- `tests/m7/prove-optional-manifest-envelope.sh` (only if triggered).

### Dependencies

- None blocking; intentionally deferred.

## Consolidation Phase (Post-Implementation)

### Why A Consolidation Phase Exists

Milestone identifiers (`m0`, `m1`, `m2`, `pre-m1`, ...) are *project-management* artifacts:
they record the order work landed. They are not *product* concepts. A shipped framework
must be named by capability and contract, not by the sprint that delivered a capability.
Two debts have accumulated and must be paid once the capability line is done:

1. `mX` names have leaked across the repository, including into consumer-observable and
   admission-gating surfaces. The clearest symptom: the admission contract strings are
   `nixfied-toolchain:m2c:1` / `nixfied-runtime-abi:m2c:1` — they have churned `m0 -> m2c`
   across milestones, which RFC v2 explicitly forbids ("should not churn for rendered docs
   or non-semantic compiler changes"). The state epoch is `nixfied-m0`, the helper closure
   is `m0-helper`, model `maturity` is `m0`, and the example is `m0-minimal`. Milestone
   progress is being conflated with contract versioning and product identity.
2. End-to-end behavior is proved by a "bunch of shell scripts" (`tests/mX/prove-*.sh`).
   For a framework of this capability that is not acceptable as the primary acceptance
   gate: shell `grep`-assertions are brittle, give no structured reporting, share no
   fixture model, and do not actually simulate a real downstream adopter's repository.

The phase is two milestones, executed **C1 then C2**. C1 builds the real conformance
suite first so that C2's renames are provably behavior-preserving (green suite before and
after). This inverts the order the debts were listed in, deliberately: never run a
repository-wide rename without an executable safety net already in place.

### Scope Boundary For The Whole Phase

- No new runtime or model *capability*. This phase is naming + test-architecture only.
- The one intentional behavior change is the contract-version rename in C2 (old models
  stop admitting), which is the correct, documented consequence of exact-match ABI policy.
- Historical records keep their milestone names: `RFC_v2_implementation_plan_M*.md`, this
  file, and git history are development history and are *not* rewritten. Milestone
  vocabulary is retired only from the live product surface, not from the project's memory.

## Consolidation C1: Downstream Conformance Suite (Done, with deviations)

### Product Purpose

Replace the ad hoc end-to-end shell proofs with a first-class, black-box conformance
harness that simulates a real downstream project adopting and operating Nixfied purely
through public surfaces, with structured fixtures, golden artifacts, and machine-readable
reporting.

### Delivered

- `c1: add downstream conformance suite over public surfaces` — `runtime/crates/nixfied-conformance`,
  a black-box harness exposed as `nix run .#conformance`. Scenarios are declarative data; for
  each it builds the example model via `nix build`, runs the real `nixfied-runtime` binary,
  and asserts on operator-observable outputs (run JSON: services set, task success, workflow
  node order; the written summary; the marker-gated clean result). It emits a structured JSON
  report (per-scenario pass/fail, reason, timing, computed model hash) and exits non-zero on
  failure.
- Scenarios: `minimal-service`, `postgres-adapter`, `workflow-graph`, `polyglot-stack`, and a
  `negative-unknown-workflow` self-test (a `run --workflow <missing>` that must fail), proving
  the harness distinguishes pass from fail.

### Deviations From The Original C1 Spec

These were scoped down to land C1 within the session; they are the documented gaps, not
silent omissions:

- The harness does **not** scaffold a throwaway git repo and run the **installer** per
  scenario. It drives the public surfaces by building the in-repo example models and running
  the runtime binary. (The install + non-destructive upgrade path is covered by
  `tests/m5/prove-install-upgrade.sh`, not yet a conformance scenario.)
- No **golden/snapshot** artifacts or `--update-goldens` workflow yet; assertions are
  structural on the run/summary JSON rather than snapshot diffs.
- It is **not** wired as an entry of `nix flake check`: the suite drives real `nix build` +
  the runtime binary, which the nix-build sandbox cannot host (nested nix). `nix flake check`
  instead gates that the conformance app **wrapper builds** (`checks.conformance-app`); the
  suite runs via `nix run .#conformance`.
- The interim `tests/mX/prove-*.sh` shell proofs are **kept**, not migrated-and-deleted. They
  cover behavior C1's five model scenarios do not (cancellation, GC hardening, lifecycle-op
  ordering via cargo tests, slot isolation, install scaffold/upgrade, view surfaces).

### Strict Boundary

Included:

- A dedicated Rust harness crate (working name `nixfied-conformance`) exposed as
  `nix run .#conformance` and wired as the single end-to-end entry of `nix flake check`.
- A **downstream-repo fixture**: the harness scaffolds a throwaway git repository in a
  temp dir, pins `nixfied` as a flake input (a `path:` pin to the checkout under test),
  runs the **real installer**, then drives only public surfaces from that point on
  (`nix build .#model`, the runtime binary, operator-observable registry/summary/views).
- **Scenarios as declarative data**, not scripts. Each scenario names a downstream model
  declaration and its expected observable outcome: lifecycle event sequence, registry rows
  (runs/services/processes/ports/events/leases/cleanups), summary shape, endpoint-ownership
  verdict, and cleanup result. The harness runs all scenarios through one uniform engine.
- **Golden / snapshot artifacts** for summaries, `schema`/`docs`/`capabilities` views, and
  event sequences, with a documented `--update-goldens` workflow.
- **Structured JSON reporting** per scenario: pass/fail, reason, timing, and the computed
  model hash, suitable for CI consumption.
- Migration of the existing `tests/mX/prove-*.sh` end-to-end checks into harness scenarios,
  after which those shell scripts are deleted.

Excluded:

- White-box crate tests. The per-crate `#[test]` unit/integration tests stay as they are;
  they legitimately test internals. C1 only replaces the *black-box end-to-end* layer.
- Any harness access to `nixfied-model` / `nixfied-runtime` internals for behavior
  assertions. The harness asserts only on operator-observable surfaces — that is what makes
  it a real usage simulation rather than a privileged white-box test. (Reading the registry
  SQLite and summary JSON is allowed: those are documented operator surfaces.)

### Design Principles

- **Public-surface contract is enforced structurally**, e.g. the harness crate does not
  depend on the internal crates as libraries; it shells the built `nixfied` binary the way
  an operator would.
- **One toolchain.** The harness is Rust to match the runtime; no second test language.
- **Scenarios own their fixtures.** Adding a scenario adds data + golden files, not a new
  bespoke script.

### Sequencing: Interim Shell Proofs, Then One Strong Suite

Each capability milestone (M3, M4, M5, M6) keeps shipping its own `tests/mX/prove-*.sh`
script as **interim, intentionally weak** coverage while that milestone is in flight. The
shell proofs exist to catch gross regressions during development; they are explicitly not
the final acceptance gate. C1 is built **once the whole capability line (M1-M6) is
implemented and weakly proved this way**, not incrementally per milestone.

At that point C1 defines the meaningful, strong scenarios on top of real, installable
**example/simulation projects** — downstream-shaped repositories that the harness installs
the framework into and then exercises end to end (minimal service, postgres adapter,
workflow graph, slot isolation, cancellation/GC, install upgrade, polyglot). When a
scenario covers what a shell proof covered, that shell proof is migrated into the scenario
and deleted. Concretely:

- Subsumes the M5 upgrade proof: install + non-destructive upgrade become conformance
  scenarios rather than a separate `tests/m5/` script.
- Becomes the home for the M3/M4/M6 end-to-end proofs (postgres adapter, workflow graph,
  polyglot). Their interim `tests/m3|m4|m6/prove-*.sh` scripts are superseded and removed
  only when the corresponding conformance scenario exists and is green.

### Proof Of The Suite Itself

- `nix run .#conformance` runs green across all migrated scenarios.
- `nix flake check` invokes the suite and fails on any scenario regression.
- A deliberately broken scenario fixture makes the suite fail with a structured reason
  (negative self-test).

### Dependencies

- Lands after the full capability line (M1-M6) is implemented and weakly proved by interim
  shell scripts. C1 is a single consolidation effort, not bootstrapped or grown per
  milestone — its value is defining strong scenarios over the complete capability set on
  real installable example projects.

## Consolidation C2: De-Milestone The Product Surface (Done for product surface; test-layout tail open)

### Product Purpose

Remove `mX` vocabulary from every consumer-observable and internal-but-product surface,
and split the three concerns that milestone numbers currently conflate: **contract
version**, **product identity**, and **development history**.

### Delivered

- `c2: de-milestone contract version, maturity, and state identity tokens` — `m2c:1` ->
  `1` for `toolchainId`/`runtimeAbi` in `nix/spec/constants.nix` and `constants.rs` (lockstep;
  the retired `m2c` ABI is now refused, asserted by `abi_mismatch_is_contract_error`).
  `stateEpoch "m0" -> "1"`, `markerIdentity "nixfied-m0" -> "nixfied-state"`,
  `maturity "m0" -> "stable"` with the `SurfaceMaturity::M0` variant dropped, source
  fingerprint default `m0-placeholder -> live-fingerprint`. (`m0-helper` had already become
  `synthetic-helper` during M3.)
- `c2: strip milestone vocabulary from product source messages` — `validate_m0`/`ValidateM0`
  -> `validate`/`Validate`, `records::m0` -> `default_slot`, and all `M0` prose removed from
  Nix option descriptions and runtime error messages.
- `c2: rename minimal example and guard product surfaces against milestone tokens` —
  `examples/m0-minimal` -> `examples/minimal` (dir, `projectId`, flake attr `minimal-model`,
  conformance scenario, proofs). Added `tests/guard-no-milestone-tokens.sh`, scoped to product
  surfaces (`nix/`, `examples/`, `flake.nix`, `runtime/crates/*/src`), failing on any
  `\bm[0-9]+\b` / `:mN:` / `pre-mN` token so the debt cannot silently return.

The conformance suite was green before and after; the only intended behavior change is the
contract-version bump (old-ABI models stop admitting).

### Open Tail (test layout only)

Not done, and intentionally guard-exempt as development history:

- The interim shell proofs still live under `tests/m0|pre-m1|m1|m2|m3|m4|m5|m6/` with
  milestone names, and the runtime unit tests are still `m0_*.rs` files. The Classification
  Rule below assigns these to "folded into C1 / capability-named files"; that fold + rename
  is the remaining work. Because C1 was scoped down (see its deviations) and does not yet
  subsume the shell proofs, deleting them would lose coverage, so they were kept.
- `AGENTS.md` still references the old `tests/m0` layout and `m2c`/`M0` common checks and
  should be refreshed when the tail is closed.

### Classification Rule (drives every rename)

| Surface (current) | Class | Target |
| --- | --- | --- |
| `nixfied-toolchain:m2c:1`, `nixfied-runtime-abi:m2c:1` (`nix/spec/constants.nix`, `constants.rs`) | Contract version | Milestone-free monotonic version, e.g. `nixfied-runtime-abi:1`, bumped only on real breaking change |
| `stateEpoch = "nixfied-m0"` (`nix/modules/state.nix`), `maturity = "m0"` (`derive.nix`) | Contract/identity | Real epoch + maturity tokens decoupled from milestones |
| `m0-helper` / `nixfied-m0-helper` closure + execId (`nix/lib/closures.nix`, `derive.nix`) | Product identity | Capability name (e.g. `synthetic-helper`) |
| `m0-placeholder` source default (`nix/modules/source.nix`) | Product identity | Capability/intent name |
| `examples/m0-minimal`, flake attrs `m0-minimal-model` / `m0MinimalModel` | Public consumer surface | `examples/minimal`, `minimal-model` |
| `validate_m0` (`validation.rs`), `records::m0` (`records.rs`), `m0_*.rs` test files | Internal symbols | Capability names (`validate`, contract-named constructors, `admission.rs`, ...) |
| `tests/m0|pre-m1|m1|m2/` dirs | Test layout | Folded into C1 conformance scenarios |
| `RFC_v2_implementation_plan_M*.md`, this file, git history, commit messages | Development history | **Unchanged** — milestone names are correct here |

### Contract-Version Policy (the one behavior change)

- Define a versioning scheme independent of milestones: `runtimeAbi` and `toolchainId`
  carry an integer that increments **only** on a real breaking change to the
  model/executor contract, never per milestone or per docs rebuild.
- The Nix constant and the Rust constant change in lockstep; the generated model must still
  match the runtime exactly. A conformance scenario asserts admission succeeds on the new
  value and refuses a model carrying the old value (`RUNTIME_ABI_MISMATCH`).
- `stateEpoch` becomes a real epoch token with the existing cross-epoch refusal stance
  preserved.

### Execution Discipline

- Land **after C1 is green**. Run the conformance suite before and after; for every rename
  except the intentional contract-version bump, the suite output must be identical.
- Do it as one coherent, reviewable refactor (or a tight sequence) so the repository is
  never half-renamed. No `mX` token may remain in any non-history file at the end.
- Add a guard (e.g. a conformance/CI check or a simple repo lint) that fails if `mX`
  milestone tokens reappear in product surfaces, so the debt cannot silently return.

### Proof

- Conformance suite (`nix run .#conformance`) green before and after; only the
  contract-version bump changed behavior.
- Old-ABI (`m2c`) model is refused, new-ABI model admits (`abi_mismatch_is_contract_error`).
- `tests/guard-no-milestone-tokens.sh` passes: no `\bm[0-9]+\b` / `:mN:` / `pre-mN` token in
  **product surfaces** (`nix/`, `examples/`, `flake.nix`, `runtime/crates/*/src`).
- Not yet satisfied: the full "no `mX` outside history files" bar. Milestone tokens remain in
  the interim `tests/mX/` shell proofs and `m0_*.rs` unit-test files (the guard exempts them
  as development history pending the C1 fold; see "Open Tail" above).

### Dependencies

- Requires C1 (the suite is the safety net) and the completed capability line.

## Appendix: Optional Runtime Adapter Protocol

Deferred beyond the M1-M7 line. RFC v2 lists it as a possible future milestone only if
direct generic primitive execution proves insufficient. No work is planned until that
insufficiency is demonstrated; the default remains RUNTIME-GENERIC-1 (concrete adapters
are Nix-side model generators).
