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

Run the Rust workspace checks:

```sh
cargo fmt --manifest-path runtime/Cargo.toml --all --check
cargo check --manifest-path runtime/Cargo.toml
cargo test --manifest-path runtime/Cargo.toml
```

Run the Nix and M0 proof checks:

```sh
nix flake check
tests/m0/prove-downstream-minimal.sh
tests/m0/prove-runtime-without-nix.sh
tests/m0/prove-install-scaffold.sh
tests/pre-m1/prove-view-surfaces.sh
tests/m1/prove-slot-isolation.sh
tests/m2/prove-cancellation.sh
tests/m2/prove-gc-hardening.sh
tests/m2/prove-lifecycle-ops.sh
```

The no-Nix proof builds and realises the model first, then places a failing fake
`nix` executable earlier in `PATH` to prove `nixfied-runtime` does not invoke
Nix after admission begins.

## What Comes Next

Slot isolation (M1) and cancellation/cleanup hardening (M2) are implemented. The
remaining RFC milestones are the Nix-side Postgres adapter (M3), workflow graphs
(M4), installable-wrapper upgrade hardening (M5), the polyglot example (M6), and
optional manifest or runtime adapter work only if future milestones prove they
are needed (M7+).

Do not treat those remaining milestones as implemented.
