# Nixfied

Nixfied v2 is a greenfield rebuild around one boundary: typed Nix emits one
semantic `model.json`, and a generic Rust runtime admits and executes that model.

Current status: **M0 through M2 are implemented**. The M0 spine runs from a
downstream-shaped Nix module to a Nix-store `model.json`, Rust admission, SQLite
registry state, a synthetic foreground service, endpoint ownership verification,
a dependent task, and `ps` / `down` / `clean` reconciliation. On top of that,
slot isolation (M1) and cancellation plus GC hardening with a generic lifecycle
operation contract (M2) are also implemented and proved.

Still to come: the Nix-side Postgres adapter (M3), workflow graphs (M4),
installable-wrapper upgrade hardening (M5 currently ships only the install
scaffold), the polyglot example (M6), and the optional manifest envelope (M7,
deferred). See `RFC_v2_implementation_plan_M1to7.md` for the current checkpoint.

## Core Rules

- `RFC_v2.md` is the product source of truth.
- `model.json` is the only semantic seam between Nix and Rust.
- Generated `schema`, `docs`, and `capabilities` outputs are views over
  `model.json`, not independent authority.
- `nixfied-runtime` must never call Nix, `nix-store`, `nix build`, or
  `nix eval`.
- There is no v1 compatibility promise and no model/runtime compatibility
  across toolchain or ABI changes.
- M0 does not implement workflows, adapters, service reuse, real secrets,
  multi-slot execution, SQLite migrations, or runtime adapter protocols.

## Repository Layout

```text
nix/                         typed modules, compiler passes, model spec
runtime/crates/nixfied-model shared serde contract for model.json
runtime/crates/nixfied-runtime
                             generic M0 runtime and control commands
runtime/crates/nixfied-cli   placeholder ergonomic CLI crate
examples/m0-minimal          downstream-shaped minimal example
tests/m0                     end-to-end M0 proof scripts
RFC_v2.md                    architecture source of truth
RFC_v2_implementation_plan_M0.md
                             completed M0 execution plan
```

## Prerequisites

- Nix with flakes enabled.
- Rust toolchain compatible with the workspace in `runtime/Cargo.toml`.
- SQLite development/runtime support available through the normal system or
  `nix develop`.

Use the development shell if you want the expected Rust and SQLite tools:

```sh
nix develop
```

## Install Into Another Project

From another project repository, scaffold the minimal Nixfied integration:

```sh
nix run github:willyrgf/nixfied#install
```

The installer creates `flake.nix` and `nixfied.nix` only when they are absent.
If `flake.nix` already exists, it refuses to edit it and prints the snippet to
merge manually. The generated project builds its model with:

```sh
nix build .#model
```

## Build The M0 Model

From the repository root:

```sh
nix build .#m0-minimal-model
```

The output contains:

```text
model.json
views/schema.json
views/docs.md
views/capabilities.json
```

There is intentionally no required `manifest.json`.

The downstream-shaped example can also build through its own flake:

```sh
nix build --no-link --print-out-paths ./examples/m0-minimal#model
```

## Run Runtime Checks

Build the runtime:

```sh
cargo build --manifest-path runtime/Cargo.toml -p nixfied-runtime
```

Admit a store model and print the computed model identity:

```sh
model_out="$(nix build --no-link --print-out-paths .#m0-minimal-model)"
runtime/target/debug/nixfied-runtime check --model "$model_out/model.json"
```

M0 runtime control commands are:

```sh
runtime/target/debug/nixfied-runtime check --model "$model_out/model.json"
runtime/target/debug/nixfied-runtime ps --model "$model_out/model.json"
runtime/target/debug/nixfied-runtime down --model "$model_out/model.json"
runtime/target/debug/nixfied-runtime clean --model "$model_out/model.json"
```

Use `NIXFIED_STATE_DIR` to place runtime state in a temporary directory while
testing.

## Verification

The end-to-end gate is the product testing itself: nixfied's own repo is a
nixfied project whose `conformance` workflow drives the framework through its own
surfaces. It is layered — a trusted `cargo test` floor, then `nix flake check`
structural gates, then the dogfood workflow run by the nix-built runtime.

```sh
# 1. Trusted floor (pinned toolchain via the dev shell).
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets --all-features -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test --workspace'

# 2. Structural gate: every model builds and the binaries compile.
nix flake check

# 3. Dogfood gate: nixfied runs its own conformance workflow.
rt="$(nix build .#nixfied-runtime --no-link --print-out-paths)/bin/nixfied-runtime"
self="$(nix build .#self-model --no-link --print-out-paths)/model.json"
"$rt" check --model "$self"
NIXFIED_CONFORMANCE_CHECKOUT="$PWD" \
  "$rt" run --model "$self" --workflow conformance --timeout-ms 600000
```

`.github/workflows/conformance.yml` runs exactly this. There are no residual e2e
shell proofs: lifecycle/cancellation/GC invariants are white-box cargo tests,
SEAM-1 (the runtime never invokes Nix — proven by poisoning `PATH` with a failing
fake `nix` and asserting it is never called) is the
`runtime_drives_full_lifecycle_without_invoking_nix` cargo test, and the
view→model projection contract is asserted inside every conformance capability
check.

## What Comes Next

Slot isolation (M1) and cancellation/cleanup hardening (M2) are implemented. The
remaining RFC milestones are the Nix-side Postgres adapter (M3), workflow graphs
(M4), installable-wrapper upgrade hardening (M5), the polyglot example (M6), and
optional manifest or runtime adapter work only if future milestones prove they
are needed (M7+).

Do not treat those remaining milestones as implemented.
