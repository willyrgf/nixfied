# RFC v2 Implementation Plan: Pre-M1 Through M7

Source of truth for scope and architecture remains `RFC_v2.md`.

## Current Progress Checkpoint

Status reflects the `v2` branch as of this revision.

| Milestone | State | Evidence |
| --- | --- | --- |
| M0 walking skeleton | Done | `tests/m0/*`, full runtime spine |
| Pre-M1 view surfaces | Done | `tests/pre-m1/prove-view-surfaces.sh` |
| M1 slot isolation | Done | `tests/m1/prove-slot-isolation.sh` |
| M2 cancellation & GC hardening | Done | `tests/m2/{prove-cancellation,prove-gc-hardening,prove-lifecycle-ops}.sh` |
| M3 Nix-side Postgres adapter | Not started | proof `tests/m3/prove-postgres-adapter.sh` (to create) |
| M4 workflow graphs | Not started | proof `tests/m4/prove-workflow-graphs.sh` (to create) |
| M5 installable downstream wrapper | Partial (scaffold only) | `nix/install/install.sh`, `tests/m0/prove-install-scaffold.sh` |
| M6 polyglot example | Not started | proof `tests/m6/prove-polyglot-stack.sh` (to create) |
| M7 optional manifest envelope | Deferred by decision | none |

Notes:

- No `tests/m3`, `tests/m4`, `tests/m6`, or `tests/m7` directories exist yet. Their
  proof scripts are listed above as deliverables, not as present non-blocking stubs.
- M5 currently ships only the M0-era install scaffold (refuse-to-overwrite create of
  `flake.nix`/`nixfied.nix`). Its proof lives under `tests/m0/` and is not yet a
  milestone-tagged adoption/upgrade hardening proof. See M5 below for the remaining work.
- M7 stays deferred until a concrete need (portable bundles, cache export/import,
  standalone distribution outside the Nix store, or integrity-bound materialised views)
  appears.

## Agenda

This continuation plan covers milestone work through M7 and keeps M0 constraints.

### Scope

*1. M1: slots, per-slot state pathing, and registry partitioning. (done)*
*2. M2: cancellation, lease reconciliation, cleanup hardening, and generic lifecycle. (done)*
*3. M3: Nix-side reference adapter, minimal postgres.*
*4. M4: workflow graphs.*
*5. M5: installable downstream wrapper hardening.*
*6. M6: polyglot example.*
*7. M7: optional manifest envelope (only if triggered by concrete need).*

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

## M3: Nix-Side Reference Adapter — Minimal Postgres (Not Started)

### Product Purpose

Prove that a concrete service adapter (Postgres) is expressible purely as a Nix module
that generates generic model primitives, while `nixfied-runtime` stays generic and gains
no Postgres-specific code path.

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

- `tests/m3/prove-postgres-adapter.sh` (to create): downstream-shaped example builds a
  Postgres model, runtime starts/readies/queries/stops it generically, and a minimal
  non-Postgres project still compiles without the adapter.

### Dependencies

- Requires M2 lifecycle, lease, and cleanup hardening.

## M4: Workflow Graphs (Not Started)

### Product Purpose

Add dependency-aware workflows over the existing generic primitives: bounded tasks,
service requirements, readiness gates, cancellation, artifacts, summaries, and cleanup
policy.

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

- `tests/m4/prove-workflow-graphs.sh` (to create): a multi-node workflow with a service
  requirement and dependent bounded tasks runs, cancels cleanly, and writes summaries.

### Dependencies

- Requires M2 cancellation/lease semantics and M3 generic-adapter confidence.

## M5: Installable Downstream Wrapper Hardening (Partial)

### Product Purpose

Make adoption and upgrade safe through Nixfied-owned flake input/import shims that never
overwrite project-owned declarations.

### Current State

- `nix/install/install.sh` scaffolds `flake.nix` and `nixfied.nix` only when absent and
  refuses to edit an existing `flake.nix`, printing a merge snippet instead.
- Proof exists at `tests/m0/prove-install-scaffold.sh` (create + refusal paths).

### Remaining Work

- Define and prove the upgrade path (bumping the Nixfied input without clobbering
  project-owned `nixfied.nix`).
- Milestone-tag the proof: move/extend coverage into `tests/m5/` rather than `tests/m0/`.
- Confirm the shim boundary: Nixfied owns input/import wiring; the project owns all
  semantic declarations.

### Proof Script

- Existing: `tests/m0/prove-install-scaffold.sh`.
- To add: `tests/m5/prove-install-upgrade.sh` covering non-destructive upgrade.

### Dependencies

- Independent of M3/M4; can proceed in parallel.

## M6: Polyglot Example (Not Started)

### Product Purpose

Provide `examples/polyglot-stack` demonstrating multiple codebases/services composed
through generic primitives and (once available) workflows.

### Strict Boundary

Included:

- A downstream-shaped example exercising public surfaces only.
- Multiple services/tasks across languages composed via the model.

Excluded:

- Any new runtime capability; the example must use already-shipped primitives.

### Proof Script

- `tests/m6/prove-polyglot-stack.sh` (to create): the example compiles to a store model
  and runs end to end through public APIs.

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

## Appendix: Optional Runtime Adapter Protocol

Deferred beyond the M1-M7 line. RFC v2 lists it as a possible future milestone only if
direct generic primitive execution proves insufficient. No work is planned until that
insufficiency is demonstrated; the default remains RUNTIME-GENERIC-1 (concrete adapters
are Nix-side model generators).
