# RFC v2 Implementation Plan: Pre-M1 Through M7

Source of truth for scope and architecture remains `RFC_v2.md`.

## Current Progress Checkpoint

Status reflects the `v2` branch as of 2026-06-09. The full capability line (M1-M6,
M7 deferred) is implemented and verified green: `cargo test`, `cargo clippy -D warnings`,
`nix flake check`, and every `tests/*` proof pass. C2 de-milestoning is complete
across **both** product and test surfaces. **C1 is done**: the e2e gate is now the
**self-hosted dogfood gate** — nixfied's own repo is a nixfied project whose
`conformance` workflow tests the framework through its own surfaces, run by the
nix-built runtime under test (see "Consolidation C1" below). The bespoke
`nix run .#conformance` harness interface has been removed.

| Milestone | State | Evidence |
| --- | --- | --- |
| M0 walking skeleton | Done | `tests/minimal/`, `tests/runtime/`, full runtime spine |
| Pre-M1 view surfaces | Done | `tests/views/prove-view-surfaces.sh` |
| M1 slot isolation | Done | `tests/slots/prove-slot-isolation.sh` |
| M2 cancellation & GC hardening | Done | `tests/lifecycle/{prove-cancellation,prove-gc-hardening,prove-lifecycle-ops}.sh` |
| M3 Nix-side Postgres adapter | Done | `nix/adapters/postgres.nix`, `tests/postgres/prove-postgres-adapter.sh` |
| M4 workflow graphs | Done | `examples/workflow`, `tests/workflows/prove-workflow-graphs.sh` |
| M5 installable downstream wrapper | Done | `nix/install/upgrade.sh`, `tests/install/prove-upgrade.sh` |
| M6 polyglot example | Done | `examples/polyglot-stack`, `tests/polyglot/prove-polyglot-stack.sh` |
| M7 optional manifest envelope | Deferred by decision | none |
| C1 self-hosted conformance gate | Done | repo-root `nixfied.nix` `conformance` workflow; `nix/packages/runtime.nix`; per-check `nixfied-conformance` closure; `examples/downstream`; `.github/workflows/conformance.yml` |
| C2 de-milestone product + tests | Done | `tests/guard-no-milestone-tokens.sh` (covers product + test surfaces) |

Notes:

- **A foundation generalization preceded M3 and was not anticipated by the original plan.**
  Despite M1/M2 being marked Done, the product was still a single hardcoded
  `synthetic`/`smoke`/`m0-helper` fixture: `validation.rs` was exact-match, the Nix surface
  emitted only that fixture, and the runtime selected services/tasks by hardcoded name. M3
  therefore first replaced exact-match validation with a structural contract, introduced a
  generic Nix declaration surface (`closures`/`execs`/`services`/`tasks` + `nix/adapters/`),
  and made the runtime drive arbitrary multi-service/multi-task models. See M3 below.
- The shell proofs now live under capability-named directories
  (`tests/{minimal,runtime,views,slots,lifecycle,install,postgres,workflows,polyglot}/`) and
  the runtime unit tests are capability-named (`admission.rs`, `state.rs`, `service.rs`,
  `registry.rs`, `model_contract.rs`). The C2 guard covers product **and** test surfaces;
  milestone vocabulary survives only in true history (this file, the RFC plans, git).
- Two capability-adjacent runtime changes were required and landed generically (no
  service-specific code): a `process-tree` containment mode (multi-process supervisors like
  Postgres whose children form their own process groups) and multiple services per run
  (idempotent run/lease registry rows). Both are documented in M3/M6 below.
- **The C1 redesign supersedes the bespoke e2e gate.** The interim `nixfied-conformance`
  crate and the `nix run .#conformance` flake app were a stepping stone; the agreed gate is
  nixfied testing itself (a `conformance` workflow run by the runtime under test). The
  bespoke crate's assertion logic is reused as task closures; the `#conformance` flake-app
  interface is removed. See "Consolidation C1".
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

Consolidation: *C1 self-hosted conformance gate (done), C2 de-milestone
product + test surfaces (done).*

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
2. End-to-end behavior is proved by a "bunch of shell scripts" (`tests/*/prove-*.sh`).
   For a framework of this capability that is not acceptable as the primary acceptance
   gate: shell `grep`-assertions are brittle, give no structured reporting, share no
   fixture model, and do not actually simulate a real downstream adopter operating the
   product. The chosen resolution is **self-hosted conformance**: nixfied tests itself
   through its own task/workflow surfaces (C1 below), so the gate *is* the product running.

The phase is two milestones. In execution they landed C2-then-C1-redesign: the capability
line shipped with interim shell proofs, C2 de-milestoned the product and test surfaces under
the guard, and C1 is now being rebuilt as the self-hosted gate. The original ordering
intent — never run a repository-wide rename without an executable safety net — was honored
by the interim shell proofs + `cargo test` acting as that net during C2.

### Scope Boundary For The Whole Phase

- No new runtime or model *capability*. This phase is naming + test-architecture only.
- The one intentional behavior change is the contract-version rename in C2 (old models
  stop admitting), which is the correct, documented consequence of exact-match ABI policy.
- Historical records keep their milestone names: `RFC_v2_implementation_plan_M*.md`, this
  file, and git history are development history and are *not* rewritten. Milestone
  vocabulary is retired only from the live product surface, not from the project's memory.

## Consolidation C1: Self-Hosted Conformance — nixfied Tests Itself (Done)

### Product Purpose

Make nixfied's end-to-end gate *be the product running itself*. A "CI job" in nixfied's own
vocabulary is a **task**, and a CI pipeline is a **workflow**. So the gate is not a bespoke
external harness — the nixfied repository is itself a nixfied project whose `conformance`
workflow exercises the framework end to end (install, all adapters, workflows, slots,
cancellation, cleanup), executed by the very `nixfied-runtime` under test and invoked through
the same surfaces every adopter uses. No new public interface; maximal reuse; the framework's
own `nixfied.nix` becomes the canonical, copy-paste adopter example.

This supersedes the interim bespoke harness. The `nix run .#conformance` flake app and the
`#conformance` interface are removed; the Rust assertion logic in
`runtime/crates/nixfied-conformance` is retained but reused as **task closures**, not a
standalone app.

### What Is Reused (the point)

Environments, tasks, workflows, service-requirement readiness gates, the topological
scheduler, per-task and per-workflow summaries, artifacts, cancellation/teardown, and the
`run --workflow` surface all do double duty: they orchestrate nixfied's own conformance.
Dogfooding removes the bespoke *interface and orchestrator*; the assertion *logic*
(install-driving, registry-sqlite checks, golden-view diffs) remains, as closures behind
tasks.

### Trust Model (resolving self-certification)

A system cannot fully certify itself: a bug in the runtime's scheduler or exit-policy could
run the broken code to "prove" itself and report a false pass. The gate is therefore layered,
and CI runs the layers in order:

1. **Trusted bootstrap — `cargo test`.** The white-box unit/integration tests verify the
   runtime's own primitives (admission, the workflow scheduler, task exit policy, registry
   events, marker-gated cleanup) *without the runtime grading itself*. This is the floor and
   must pass first.
2. **Dogfood gate — `nixfied-runtime run --workflow conformance`.** Because the bootstrap
   establishes the orchestration primitives are sound, the product can be trusted to
   orchestrate its own e2e. The conformance tasks additionally write **ground-truth
   artifacts** (per-check verdicts) so the result is independently inspectable, not only
   "the workflow said ok".
3. **`nix flake check`** keeps cheap structural gates: every example model builds, the
   nix-packaged binaries compile, and the de-milestoning guard passes.

The NixOS-VM option was considered and rejected: the no-limitations execution the gate needs
(a real Nix daemon, ports, multi-process supervisors, the installer's nested `nix`) is
already available in a normal shell, so a VM unlocks nothing here; hermeticity comes from
pinned inputs + nix-built binaries + throwaway repos/state + clean CI runners, not a VM.

### Components

- **Reproducible, host-toolchain-free binaries.** A pinned `rust-overlay` flake input builds
  `nixfied-runtime` (and the conformance closure) as nix store packages, so the gate — and
  `#install`-ed downstream projects — need only `nix`, not a host Rust toolchain. (nixpkgs
  25.05 ships an rustc older than the workspace's let-chain requirement, so a pinned
  toolchain is required regardless.)
- **The framework's self-project.** A repo-root `nixfied.nix` declares the `conformance`
  workflow. Its nodes are tasks bound to closures: capability checks (drive each example
  through every public surface) plus an **adoption** task that runs the real `#install` +
  `#upgrade` into a throwaway git repo pinned to `path:<checkout>`.
- **The downstream worked example.** `examples/downstream/` — a realistic "small system"
  (Postgres + services + a workflow + multi-slot) that the adoption task installs and drives,
  doubling as the integration guide other teams copy.
- **Assertions.** The retained `nixfied-conformance` code becomes the closure invoked by
  tasks (e.g. `nixfied-conformance --check postgres`), asserting only on operator-observable
  surfaces (run JSON, registry sqlite, summaries) plus golden snapshots of the
  `schema`/`docs`/`capabilities` views, with `--update-goldens`.

### Honest Weak Points

- **Self-cert blind spot.** Narrowed by the cargo bootstrap (which covers exit-policy /
  scheduler correctness) and the ground-truth artifacts, but not zero. Making it zero would
  require a non-nixfied orchestrator — exactly the bespoke harness we are removing.
- **Bootstrap dependency.** If the model compiler or runtime cannot launch the workflow, the
  gate yields no signal — but `cargo test` runs first and fails earlier with a clearer reason.
- **Nesting.** `nixfied-runtime` runs tasks that run `nixfied-runtime` against throwaway
  projects; each needs an isolated `NIXFIED_STATE_DIR` and its own slot/port window so inner
  and outer runs do not collide.
- **The logic does not vanish.** Dogfooding removes the bespoke interface, not the
  install-driving / sqlite-asserting / golden-diffing code.
- **`path:` self-pin.** Building the throwaway project pins the working tree, so the gate
  tests the checkout, not a committed rev — correct for CI, stated for clarity.

### Execution Plan (commit-by-commit)

1. **Packaging.** Add a pinned `rust-overlay` input; build `packages.nixfied-runtime` (and
   the conformance closure) via `buildRustPackage`/crane; verify `nix build` yields a working
   runtime binary. *(load-bearing risk; prove first.)*
2. **Worked example.** Add `examples/downstream/` (Postgres + services + workflow +
   multi-slot) and a short README that is the adoption guide.
3. **Self-project + workflow.** Add the repo-root `nixfied.nix` declaring the `conformance`
   workflow: capability-check tasks + the install/upgrade adoption task, bound to the
   conformance closure and the packaged runtime.
4. **Assertion closure.** Recast `nixfied-conformance` as a per-check closure (`--check
   <name>`, `--update-goldens`); add the install/upgrade adoption check and golden snapshots;
   remove the `#conformance` flake app and `apps.conformance`.
5. **Gate wiring + CI.** CI = `cargo test` (floor) then build the self-model + `nixfied-runtime
   run --workflow conformance`. `nix flake check` keeps model builds + binary compile + guard.
   Migrate the capability shell proofs into the workflow's tasks and delete the ones the
   workflow subsumes.
6. **Docs.** Record the self-hosted gate in this plan, `AGENTS.md`, and the example README.

### Delivered

Landed in six commits matching the execution plan above:

- `flake: package nixfied-runtime via pinned rust-overlay` — `nix/packages/runtime.nix`
  builds the host-Rust-free `nixfied-runtime`/`nixfied-conformance`/`nixfied` binaries with
  the pinned `rust-overlay` toolchain (rusqlite `bundled`, no system sqlite); `nix build
  .#nixfied-runtime` yields a working binary.
- `examples: add downstream worked-example project` — `examples/downstream/` (Postgres via
  `adapters.postgres` + `api` + `worker` + a `release` workflow, `slotPolicy.max = 1`) with
  its own pinning `flake.nix` and a README adoption guide; builds and runs end to end.
- `nixfied: add self-project conformance workflow` — repo-root `nixfied.nix` whose
  `conformance` workflow nodes are per-check tasks bound to the nix-built conformance closure,
  each driving a baked example model store path (capability checks need no nix at run time).
- `conformance: recast harness as per-check task closure` — `nixfied-conformance --check
  <name> [--update-goldens]` with capability/slots/adoption/negative checks, golden
  schema/docs/capabilities snapshots, and per-check ground-truth artifacts; the
  `#conformance` flake app, `apps.conformance`, and `checks.conformance-app` are removed.
- `ci: wire self-hosted conformance gate and migrate proofs` — `.github/workflows/conformance.yml`
  (cargo floor → `nix flake check` + guard → check smoke → `run --workflow conformance`); the
  subsumed capability shell proofs are deleted, the residual ones kept; the dev shell carries
  the pinned toolchain.
- `docs: document self-hosted conformance gate` — this section, `AGENTS.md`, and the
  example README.

### Proof (achieved)

- `cargo test` green (trusted floor, run under the pinned toolchain).
- `nixfied-runtime run --workflow conformance` green: the framework installs, builds, runs,
  upgrades, and cleans itself end to end through its own surfaces (nodes `minimal`, `workflow`,
  `polyglot`, `postgres`, `downstream`, `slots`, `negative`, `adoption`); a ground-truth
  verdict artifact is written per check; forcing a check to fail (e.g. a broken inner runtime)
  fails the workflow with a structured `MODEL_ADMISSION` reason naming the task and model hash.
- `nix flake check` green: every example + the self-model build, the binaries compile, the
  guard passes.
- No `#conformance` interface remains.

### Dependencies

- The completed capability line (M1-M6) and C2 de-milestoning. Reuses M4 workflows and M2
  cancellation as the orchestration substrate it runs on.

## Consolidation C2: De-Milestone The Product Surface (Done)

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

### Test-Layout De-Milestoning (Done)

A follow-up pass closed the test-layout tail:

- Runtime unit-test files renamed to capability names (`admission.rs`, `state.rs`,
  `service.rs`, `registry.rs`, `model_contract.rs`).
- Shell proofs reorganized into capability directories
  (`tests/{minimal,runtime,views,slots,lifecycle,install,postgres,workflows,polyglot}/`),
  fixing the cross-references to renamed cargo test targets.
- Milestone tokens cleaned out of test fixtures (e.g. `m0-helper` -> `synthetic-helper`,
  `nixfied-m0` -> `nixfied-state`, retired ABI values -> `legacy`).
- The guard (`tests/guard-no-milestone-tokens.sh`) was extended to cover the test surfaces
  (`tests/` + `runtime/crates/*/tests`), excluding only itself.
- `AGENTS.md` refreshed to the capability-named layout and current checks.

Milestone vocabulary now survives only in true history (this file, the RFC plan documents,
and git). The shell proofs are retained until the C1 self-hosted workflow subsumes them
(its Execution Plan migrates the capability proofs into workflow tasks).

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
- `tests/guard-no-milestone-tokens.sh` passes across **product and test surfaces** (`nix/`,
  `examples/`, `flake.nix`, `runtime/crates/*/src`, `tests/`, `runtime/crates/*/tests`): no
  `\bm[0-9]+\b` / `:mN:` / `pre-mN` token outside true history files.
- The full "no `mX` outside history files" bar is met (the test-layout pass closed it).

### Dependencies

- Requires C1 (the suite is the safety net) and the completed capability line.

## Appendix: Optional Runtime Adapter Protocol

Deferred beyond the M1-M7 line. RFC v2 lists it as a possible future milestone only if
direct generic primitive execution proves insufficient. No work is planned until that
insufficiency is demonstrated; the default remains RUNTIME-GENERIC-1 (concrete adapters
are Nix-side model generators).
