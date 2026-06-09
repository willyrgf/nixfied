# AGENTS.md

This repository is the Nixfied v2 greenfield implementation. Keep work small,
RFC-shaped, and explicit.

## Authority

- Read `RFC_v2.md` before architectural work. It is the source of truth.
- `RFC_v2_implementation_plan_M1to7.md` records the executed plan: the capability
  line (M1-M6; M7 deferred) and the consolidation phase (C1 self-hosted
  conformance gate, C2 de-milestoning) are implemented. `RFC_v2_implementation_plan_M0.md`
  is the earlier Milestone 0 plan and is historical.
- If the RFC and existing code disagree, stop and make the disagreement clear
  before changing architecture.

## Non-Negotiable Boundaries

- `model.json` is the only semantic seam between Nix and Rust.
- Do not add required `manifest.json`, `schema.json`, `capabilities.json`, or
  any other semantic authority.
- Generated views are disposable projections from `model.json`.
- `nixfied-runtime` must never invoke Nix, `nix-store`, `nix build`, or
  `nix eval`.
- Do not preserve or recreate v1 layouts, commands, fixtures, sidecars, APIs, or
  compatibility shims.
- Do not add migrations or compatibility layers for old models.
- Workflows, the Postgres adapter, multi-slot, multi-service-per-run, and the
  generic declaration surface are now implemented. Still deferred (do not add
  without an explicit, RFC-shaped reason): real secret injection, service reuse
  beyond exact-match identity, SQLite migrations, runtime adapter protocols, and
  the optional manifest envelope (M7).

## Project Map

```text
nix/modules/                 user-facing typed Nix declaration surface
nix/compiler/                resolve -> validate -> derive -> emit model/views
nix/spec/                    contract constants and model shape
nix/adapters/                Nix-side adapters (synthetic, postgres) + default.nix
nix/install/                 install/upgrade surfaces (shell embedded in .nix modules)
nix/packages/                host-Rust-free build of the runtime/conformance binaries
nix/lib/                     pure Nix helper functions
nixfied.nix                  the framework's self-project: the `conformance` workflow
runtime/crates/nixfied-model serde model contract + structural validation
runtime/crates/nixfied-runtime
                              Nix-free admission, registry, state, services,
                              endpoint ownership, tasks, workflows, and controls
runtime/crates/nixfied-cli   placeholder CLI crate
runtime/crates/nixfied-conformance
                              per-check conformance closure (`--check <name>`) run as
                              the task nodes of the self-hosted conformance workflow;
                              `goldens/` holds the schema/docs/capabilities snapshots
examples/                    downstream-shaped examples: minimal, postgres,
                              workflow, polyglot-stack, downstream (the worked example)
```

There is no `tests/` directory: end-to-end behavior lives in the cargo floor
(`runtime/crates/*/tests`) and the self-hosted conformance workflow.

## Design Principles

- Modularity: each Rust crate should be usable as a library with minimal
  coupling. Keep `nixfied-model` as the shared serde contract, and keep runtime
  behavior out of it.
- Type safety: prefer strong types, enums, and typed errors over stringly
  contracts or broad dynamic dispatch.
- Explicit boundaries: keep public APIs, model fields, and CLI output schemas
  intentional. Do not leak registry, OS, or placement internals through stable
  surfaces unless the RFC makes them part of the contract.
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

The cargo floor runs under the pinned toolchain, so it is identical on every host
and in CI. Use the dev shell (it provides the pinned cargo/clippy/rustfmt):

```sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets --all-features -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test --workspace'
nix flake check                    # every model builds + the binaries compile
```

The end-to-end gate is the product testing itself. It is layered (trusted cargo
floor first, then structural gates, then the dogfood workflow) and is exactly
what CI runs (`.github/workflows/conformance.yml`):

```sh
# 3. Dogfood gate: nixfied runs its own `conformance` workflow. The one-liner
#    rebuilds the runtime + self-model from the working tree, smoke-checks, then
#    runs the workflow in a private throwaway state dir. Run from the repo root.
nix run .#gate                     # forward args after `--`, e.g. -- --timeout-ms 120000

# The `gate` app is only a launcher; the expanded form (what CI runs) is:
rt="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"
self="$(nix build .#self-model --no-link --print-out-paths)/model.json"
"$rt" check --model "$self"                                   # fast launch sanity
NIXFIED_CONFORMANCE_CHECKOUT="$PWD" \
NIXFIED_CONFORMANCE_ARTIFACTS="$PWD/conformance-artifacts" \
  "$rt" run --model "$self" --workflow conformance --timeout-ms 600000
```

The workflow's nodes are per-check conformance closures: capability checks drive
each example model (`minimal`, `workflow`, `polyglot`, `postgres`, `downstream`)
and `slots` through the nix-built runtime, `adoption` runs the real `#install` +
`#upgrade` against a throwaway repo, and `negative` proves the gate fails closed.
Each writes a ground-truth verdict to `$NIXFIED_CONFORMANCE_ARTIFACTS`. To refresh
the golden view snapshots after an intended view change, run a capability check
with `--update-goldens`.

There are no e2e shell proofs left: the cancellation/GC/lifecycle invariants are
white-box cargo tests, SEAM-1 (the runtime never invokes nix) is the
`runtime_drives_full_lifecycle_without_invoking_nix` cargo test, and the
view→model projection contract is asserted inside every capability check.

## Git Hygiene

- Inspect `git status --short --untracked-files=all` before editing.
- Do not revert unrelated user changes.
- Prefer deleting incompatible old structure over wrapping it when a requested
  milestone intentionally replaces behavior.
- Keep commits focused on one architectural slice when commit-by-commit work is
  requested.
- Before committing architecture changes, review the diff against `RFC_v2.md`,
  scope discipline, tests, regressions, and accidental compatibility
  preservation.
