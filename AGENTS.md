# AGENTS.md

This repository is the Nixfied v2 greenfield implementation. Keep work small,
RFC-shaped, and explicit.

## Authority

- Read `RFC_v2.md` before architectural work. It is the source of truth.
- `RFC_v2_implementation_plan.md` records the selected Milestone 0 execution
  plan. M0 is complete; do not silently widen it.
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
- Do not implement workflows, adapters, service reuse, real secrets, multi-slot
  execution, SQLite migrations, or runtime adapter protocols unless a later
  milestone explicitly asks for them.

## Project Map

```text
nix/modules/                 user-facing typed Nix module surface
nix/compiler/                resolve -> validate -> derive -> emit model/views
nix/spec/                    M0 constants and model shape
nix/lib/                     pure Nix helper functions
runtime/crates/nixfied-model serde model contract
runtime/crates/nixfied-runtime
                              Nix-free admission, registry, state, services,
                              endpoint ownership, tasks, and controls
runtime/crates/nixfied-cli   placeholder CLI crate
examples/m0-minimal          downstream-shaped M0 example
tests/m0                     end-to-end proof scripts
```

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
  but do not introduce abstractions that widen M0 or create runtime adapter
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
- Deny unknown model fields and prefer exact M0 constants over compatibility
  defaults.
- Keep host-absolute paths out of `model.json`; materialise host placement in
  Rust at admission/runtime.
- Runtime admission must fail before process start for invalid origin, ABI,
  toolchain, target, source, closures, state policy, or unsupported M0 features.
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

Use the narrowest relevant checks for the change, then broaden when touching
shared behavior.

```sh
cargo fmt --manifest-path runtime/Cargo.toml --all -- --check
cargo clippy --manifest-path runtime/Cargo.toml --workspace --lib --examples --tests --benches --all-features -- -D warnings
cargo check --manifest-path runtime/Cargo.toml
cargo test --manifest-path runtime/Cargo.toml
nix flake check
tests/m0/prove-downstream-minimal.sh
tests/m0/prove-runtime-without-nix.sh
tests/m0/prove-install-scaffold.sh
```

Focused runtime tests:

```sh
cargo test --manifest-path runtime/Cargo.toml -p nixfied-runtime --test m0_service
cargo test --manifest-path runtime/Cargo.toml -p nixfied-runtime --test m0_state
```

## Git Hygiene

- Inspect `git status --short --untracked-files=all` before editing.
- Do not revert unrelated user changes.
- Prefer deleting incompatible old structure over wrapping it when a requested
  milestone intentionally replaces behavior.
- Keep commits focused on one architectural slice when commit-by-commit work is
  requested.
- Before committing architecture changes, review the diff against `RFC_v2.md`,
  M0 scope discipline, tests, regressions, and accidental compatibility
  preservation.
