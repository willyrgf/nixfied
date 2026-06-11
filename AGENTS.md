# AGENTS.md

This repository is Nixfied: a Nix-authored project model with a generic, Nix-free
Rust runtime. This file is the contributor/agent guide and the architectural
contract. Keep work small, explicit, and within the invariants below.
`docs/ARCHITECTURE.md` records the design rationale (the *why*); `README.md`
covers usage.

## Architecture

One boundary defines the product, split by verb:

| Verb | Owner |
| --- | --- |
| **Evaluate / build / realise** (modules → `model.json`, closures) | **Nix**, at authoring/build time |
| **Admit / execute / reconcile / clean** (process groups, ports, registry, cleanup) | **Rust**, against `model.json` and OS reality |

- **Nix is the user-facing integration and correctness layer.** Typed modules
  evaluate to one canonical `model.json`; invalid intent never compiles into an
  admitted model. Nix builds/realises closures and emits disposable views. Nix
  never starts services, owns process state, mutates the registry, or reports
  liveness.
- **Rust is the hidden, generic runtime.** `nixfied-runtime` reads one
  `model.json`, validates the admission contract, and owns all impure behavior. It
  knows no domain (no Postgres/Python/etc.) — it executes generic primitives and
  enforces typed lifecycle semantics. It never invokes Nix.
- **`model.json` is the only semantic seam.** `schema`, `docs`, and
  `capabilities` are disposable projections of it, not independent authority.

Correctness is layered: model correctness (Nix validation) → closure correctness
(Nix realisation) → admission correctness (Rust host checks) → execution
correctness (Rust impure graph).

Two binaries: `nixfied-runtime` (hidden engine: `check`, `run`, `ps`, `down`,
`clean`) and `nixfied` (ergonomic CLI: `model`, `schema`, `docs`, `capabilities`,
`install`). `compile` is `nix build`; `install`/`upgrade`/`gate` plus the dev
`check`/`test`/`ci` are flake apps, and adopters get `run`/`check`/`test`/`ci`
generated from their model by `lib.projectApps`.

## Invariants (the contract)

These hold across the codebase; do not weaken them without an explicit,
deliberate change to the contract (and matching version bump + docs + tests):

- **MODEL-SEAM-1 / SINGLE-MODEL-1:** `model.json` is the only required semantic
  artifact; views are generated from it, never separate authority.
- **MODEL-ORIGIN-1:** normal admission requires `model.json` under the Nix store;
  non-store admission is an unstable test/dev escape hatch (`--allow-non-store-model`).
- **MODEL-CONTRACT-1:** the model carries the full admission contract
  (generator/toolchain, runtime ABI, target, source policy, closure metadata,
  layered service identity, state policy, secret descriptors).
- **HASH-1:** the runtime computes `computedModelHash = sha256(raw bytes)`; no
  self-hash is embedded.
- **ABI-1:** admission requires an exact `runtimeAbi` / `toolchainId` match; no
  cross-version compatibility, no migrations. `runtimeAbi` is *derived*: its suffix
  is a digest of the capability descriptor
  (`runtime/crates/nixfied-model/capability.txt`), which Rust
  (`nixfied-model::constants`) and Nix (`nix/spec/constants.nix`) hash identically,
  so a contract change rotates the ABI on both sides at once.
- **SEAM-1:** `nixfied-runtime` never invokes Nix, `nix-store`, `nix build`, or
  `nix eval`, and never imports Nix expressions.
- **PREPARE-1:** every referenced closure is realised before the runtime starts;
  the runtime only verifies existence/executability/target/declaration.
- **RUNTIME-GENERIC-1:** the runtime executes generic primitives; concrete
  adapters are Nix-side model generators. A required runtime change for a specific
  service is a signal to generalize the primitive set, not to specialize.
- **NIX-API-1:** Nix modules are the integration API; project behavior is typed
  Nix data compiled into generic primitives.
- **SOURCE-1:** runtime operations observe source only through declared
  `codebaseId`s.
- **SVC-ID-1:** service reuse requires exact service address, endpoint identity,
  state identity, runtime compatibility hash, and target identity.
- **PORT-1:** readiness requires verified endpoint ownership, not just an open port.
- **REG-1 / REG-ORDER-1 / LIVE-1:** one transactional per-slot SQLite registry
  owns shared mutable state with a total per-slot event order; liveness is
  reconciled against the OS before being reported.
- **GC-1 / GC-2:** cleanup is idempotent, crash-safe, path-confined, marker-gated,
  lease-gated, process-gated, and policy-gated.
- **PROC-1..3 / PROC-CAP-1:** every spawned process belongs to a runtime-owned
  process group; cancellation propagates to the whole group; a long-lived process
  counts as started only after a registry process record; admission fails if a
  service needs stronger containment than the host supports.
- **REDACT-1:** persistent output is redacted before write. (Secrets are not yet
  part of the contract — see deferred scope; the model carries no secret section.)
- **SURFACE-1:** the public command set (`model`, `schema`, `docs`, `capabilities`,
  `check`, `run`, `ps`, `down`, `clean`) is framework-owned, derived in the views,
  and listed in the capability descriptor — not user-declared in the model.
- **SHELL-1 / NIX-1:** shell cannot own graph/registry/summary/validation/liveness/
  cleanup semantics; Nix cannot own live supervision/liveness/cancellation/registry/
  cleanup.

The runtime error contract (stable codes/exit classes) — `MODEL_NOT_STORE_OUTPUT`,
`MODEL_INVALID`, `MODEL_ADMISSION`, `RUNTIME_ABI_MISMATCH`, `SOURCE_MISMATCH`,
`PLATFORM_UNSUPPORTED`, `CLOSURE_MISSING`, `PORT_CONFLICT`, `PORT_UNVERIFIABLE`,
`STATE_UNWRITABLE`, `STATE_UNOWNED`, `LEASE_STALE`/`LEASE_CONFLICT`, `PROC_ESCAPE`,
`READINESS_TIMEOUT`, `CANCELED`, `CLEANUP_REFUSED`, `REGISTRY_CORRUPT`, plus the
execution-class codes `TASK_FAILED`, `LIFECYCLE_FAILED`, `DEPENDENCY_UNAVAILABLE`
(admission passed, execution failed — `MODEL_ADMISSION` after admission is, by
construction, a leak) — is public API for the current ABI. `SECRET_UNAVAILABLE`
and `SECRET_LEAK_BLOCKED` are reserved for the deferred secrets scope and do not
exist in the runtime yet.

## Non-Negotiable Boundaries

- `model.json` is the only semantic seam; do not add a required `manifest.json`,
  `schema.json`, `capabilities.json`, or any other semantic authority.
- Generated views are disposable projections from `model.json`.
- `nixfied-runtime` must never invoke Nix in any form (SEAM-1).
- Do not preserve or recreate v1 layouts, commands, fixtures, sidecars, APIs, or
  compatibility shims; do not add migrations or compatibility layers for old models.
- Keep host-absolute paths out of `model.json`; materialise host placement in Rust.

## Project Map

```text
nix/modules/                 user-facing typed Nix declaration surface
nix/compiler/                resolve -> validate -> derive -> emit model/views
nix/spec/                    contract constants and model shape
nix/adapters/                Nix-side adapters (synthetic, postgres) + default.nix
nix/install/                 install/upgrade surfaces (shell embedded in .nix modules)
nix/packages/                host-Rust-free build of the binaries + the hermetic rust-workspace check
nix/gate.nix                 the framework gate: runs the example models + slots/negative/adoption/views
nix/dev.nix                  `.#check` / `.#test` / `.#ci` apps (framework dev loop)
nix/project-apps.nix         `lib.projectApps`: run/check/test/ci apps for an adopter's model
nix/lib/                     pure Nix helper functions
runtime/crates/nixfied-model serde model contract + structural validation
runtime/crates/nixfied-runtime
                             Nix-free admission, registry, state, services,
                             endpoint ownership, tasks, workflows, and controls
runtime/crates/nixfied-cli   ergonomic CLI: model/schema/docs/capabilities/install
examples/                    downstream-shaped examples: minimal, postgres,
                             workflow, polyglot-stack, downstream (the worked example)
```

There is no `tests/` directory: end-to-end behavior lives in the cargo floor
(`runtime/crates/*/tests`) and the gate (`nix/gate.nix`).

## Design Principles

- Modularity: each Rust crate should be usable as a library with minimal
  coupling. Keep `nixfied-model` as the shared serde contract, and keep runtime
  behavior out of it.
- Type safety: prefer strong types, enums, and typed errors over stringly
  contracts or broad dynamic dispatch.
- Explicit boundaries: keep public APIs, model fields, and CLI output schemas
  intentional. Do not leak registry, OS, or placement internals through stable
  surfaces unless they are part of the contract.
- Performance is part of correctness: avoid accidental allocation or cloning in
  hot paths such as admission, reconciliation, readiness, and registry loops.
  Measure before optimizing.
- Extensibility: use traits and generic types when they improve composability,
  but do not introduce abstractions that widen scope or create runtime adapter
  protocols by accident.
- Correctness and security first: fail closed for model admission, endpoint
  ownership, cleanup safety, process containment, and secret-related behavior.
- Output stability: treat CLI JSON/text formats and typed runtime error payloads
  as public API for the current exact `runtimeAbi` / `toolchainId`.

## Code Quality Policy

- Do not introduce hacks, monkey patches, fragile workarounds, partial
  solutions, or compatibility shims to make a task appear complete.
- If a request exposes missing support, either fix the underlying design
  properly or state clearly that the work is blocked until that support exists.
- Prefer correctness, clarity, maintainability, robust design, simplicity, and
  honesty over speed or short-term convenience.
- Backward compatibility is subordinate to correctness in this greenfield
  repository, but documented public contracts must be changed deliberately with
  matching docs and tests.
- After each change, report anything not fully verified and call out any
  remaining fragility directly.

## Implementation Rules

- Keep model structs in `nixfied-model`; runtime code should not invent ad hoc
  model schemas.
- Deny unknown model fields and prefer exact contract constants over compatibility
  defaults.
- Keep host-absolute paths out of `model.json`; materialise host placement in
  Rust at admission/runtime.
- Runtime admission must fail before process start for invalid origin, ABI,
  toolchain, target, source, closures, state policy, or unsupported features.
- Liveness reports must reconcile against the OS. The registry is durable
  evidence, not the liveness oracle.
- Cleanup must be marker-gated, path-confined, policy-gated, and safe to rerun.
- Port readiness requires endpoint ownership verification, not only a successful
  TCP probe.

## Rust Style

- Prefer explicit, readable code over cleverness.
- Avoid panics in library code. Return `Result` with typed errors.
- Keep public APIs documented and consistent in names, error behavior, and
  invariants.
- Avoid cloning in hot paths. Prefer borrowing, `&str`, and slices where they
  keep ownership clear.
- Use typed errors for stable, testable behavior. Reserve `anyhow`-style glue
  errors for binaries or local orchestration where a typed error adds little
  value.
- Preserve stable machine-readable error codes and avoid breaking JSON output
  schemas inside a given ABI/toolchain contract.

## Common Checks

One command runs the whole repo, fail-fast (source gate → test floor → gate) —
the local equivalent of CI:

```sh
nix run .#ci
```

Its stages also run on their own:

```sh
nix run .#check   # nix flake check + model admission sanity
nix run .#test    # the white-box cargo floor, pinned toolchain
nix run .#gate    # the framework gate (below)
```

`nix flake check` is the hermetic core of `.#check`: the `rust-workspace` check
runs `cargo fmt --check` + `cargo clippy --all-targets -D warnings` with vendored
deps (clippy's front end type-checks every target, so there is no separate `cargo
check`), plus every example model and the debug runtime build. The cargo
*test* floor is deliberately not a flake check (it binds ports / spawns process
groups), so `.#test` / `.#ci` run it outside the sandbox under the pinned
toolchain. The raw forms (identical, for a dev shell) are:

```sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test --workspace'
```

The gate (the third `.#ci` stage, and the final CI layer after the cargo floor and
the structural gates) exercises the runtime the way adopters do — it runs the
example models as ordinary top-level runs through the runtime under test, plus the
few checks a single run can't make on its own (`.github/workflows/checks.yml`):

```sh
# Builds the debug runtime + the example models from the working tree and runs
# them against a fixed state dir ($TMPDIR/nixfied-gate), wiped fresh each run and
# kept afterward; per-check artifacts land in $TMPDIR/nixfied-gate/artifacts/.
nix run .#gate
```

For each example (`minimal`, `postgres`, `workflow`, `polyglot`, `downstream`) the
gate runs the model + cleans it — the example is its own spec, so the run fails if
its services/tasks fail — and diffs the emitted views against the runtime's
re-derivation (`nixfied {schema,capabilities,docs} --model` ⟂ `<model>/views/*`).
Then `slots` runs two concurrent slots of `downstream` and asserts full isolation
(disjoint ports/instances/process-keys, separate Postgres data clusters, isolated
clean) — the one genuinely cross-run check; `negative` proves the gate fails closed
(an undeclared workflow is refused); `adoption` runs the real `#install` +
`#upgrade` against a throwaway repo. There is no self-model and no orchestrator
binary — it is all `nix/gate.nix`.

There are no e2e shell proofs: the cancellation/GC/lifecycle invariants are
white-box cargo tests, SEAM-1 (the runtime never invokes nix) is the
`runtime_drives_full_lifecycle_without_invoking_nix` cargo test, and the
view→model projection contract is the per-example `nixfied <view>` ⟂ emitted-view
diff in the gate.

**Build profiles.** `.#ci`, `nix flake check`, and the gate all build the fast
`debug` profile (`nix/packages/runtime.nix { buildType = "debug"; }`, exposed as
the `.#nixfied-runtime-debug` package and the `nixfied-runtime` check), so the
whole CI loop shares one profile and never runs release optimization. The
optimized `release` binary — `.#nixfied-runtime`, what `.#install` and
`lib.projectApps` ship to adopters — is built only on demand; CI verifies it
compiles in a final `nix build .#nixfied-runtime` step. Behavior is identical
across profiles (the runtime is I/O-bound), and a release-only compile break is
essentially impossible once clippy + debug pass.

## Deferred

Intentionally not implemented. Do not add without an explicit, deliberate,
contract-shaped reason (and the matching validation + runtime path + proof):

- ergonomic surfaces `up`, `logs`, `validate --deep`;
- multiple environments beyond `dev`;
- `until-idle` / `persistent-until-down` service lifetimes, service reuse, and
  borrower leases (only `run-scoped` is implemented);
- additional source modes (`snapshot`, `flake-input`) and non-`fail` port
  collision policies;
- `dirtyPolicy = "reject"`: the runtime cannot prove live-workspace cleanliness,
  so admission refuses it unconditionally; the Nix option exposes only
  `allow`/`warn` until the cleanliness proof exists (the wire enum and the
  fail-closed admission check remain);
- real secret injection (secrets are not part of the contract yet);
- a portable, non-semantic manifest envelope; a dynamic runtime adapter protocol.

## Git Hygiene

- Inspect `git status --short --untracked-files=all` before editing.
- Do not revert unrelated user changes.
- Prefer deleting incompatible old structure over wrapping it when a change
  intentionally replaces behavior.
- Keep commits focused on one architectural slice when commit-by-commit work is
  requested.
- Commit messages: a single concise line, no body, no author or other
  trailers.
- Before committing architecture changes, review the diff against the invariants
  and boundaries above, scope discipline, tests, and regressions.
