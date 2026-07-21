# Developing Nixfied

This guide owns repository layout, test placement, and verification commands.
Read [`CONTRACT.md`](CONTRACT.md) before changing the model/runtime boundary and
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the rationale behind it.

## Repository map

```text
nix/modules/                   typed adopter declaration surface
nix/compiler/                  resolve -> validate -> derive -> emit model/docs
nix/spec/                      Nix-side model and ABI constants
nix/adapters/                  Nix-side domain adapters
nix/install/                   install and upgrade programs
nix/lib/                       pure Nix helpers
nix/docs/                      private generated-reference builders
nix/packages/                  reproducible Rust builds and source checks
nix/project-apps.nix           reserved controls + adopter-exported task apps
nix/gate-runtime/nixfied.nix   adopter-shaped runtime integration gate
nix/gate-nix.nix               Nix compiler/install integration gate
nix/gate.nix                   thin gate coordinator
nix/dev.nix                    local check, test, and ci app definitions
runtime/crates/nixfied-model/   serde contract, validation, capability descriptor
runtime/crates/nixfied-runtime/ admission and impure runtime behavior
runtime/crates/nixfied-cli/     install CLI
examples/                       downstream-shaped example models
```

There is no root-level `tests/` directory. Rust model/runtime behavior lives in
`runtime/crates/*/tests` and crate-local unit tests. Pure Nix derivation vectors
live in `nix/checks`; Nix compiler/install integration cases live in
`nix/gate-nix.nix`; adopter-shaped runtime integration lives in
`nix/gate-runtime/nixfied.nix`.

## Canonical local checks

Run the full local repository gate with:

```sh
nix run .#ci
```

It is fail-fast and runs these local stages:

1. `.#check` — `nix flake check`, then admission of a realised example model.
2. `.#test` — the white-box Cargo workspace floor under the pinned toolchain.
3. `.#gate` — the runtime-shaped gate followed by the Nix-layer gate.

Run a stage directly while iterating:

```sh
nix run .#check
nix run .#test
nix run .#gate
```

Use `.#test` for the complete Cargo floor. It injects the realised Postgres model
required by `interrupt_and_recover_adopts_orphaned_postgres`; a raw
`cargo test --workspace` without `NIXFIED_TEST_POSTGRES_MODEL` intentionally
skips that case.

For a targeted Rust iteration inside the pinned development environment:

```sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-model'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test admission'
```

`nix flake check` is the hermetic source/build core. Its `rust-workspace`
derivation first proves that [`OPTIONS.md`](OPTIONS.md) matches the typed Nix
modules, then runs rustfmt and Clippy with `-D warnings`; Clippy type-checks all
targets. The flake checks also build the debug runtime and minimal example model,
and run the Nix derivation golden vectors. The process- and port-using Cargo tests
run outside the Nix sandbox through `.#test`.

Regenerate the checked option reference after changing `nix/modules/`:

```sh
generated="$(nix build --impure --no-link --print-out-paths --expr '
  let
    flake = builtins.getFlake (toString ./.);
    system = builtins.currentSystem;
  in
  import ./nix/docs/options.nix {
    inherit system;
    inherit (flake.inputs.nixpkgs) lib;
    pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
  }
')"
cp "$generated" docs/OPTIONS.md
```

The generator reuses the compiler's module evaluator. `OPTIONS.md` is a checked
repository reference, not a flake app/output or an additional model authority.

## Gate composition

`nix/gate.nix` is a thin sequential coordinator:

- `gate-runtime` is a first-class Nixfied model. It exercises example runs, the
  emitted model/docs contract, concurrent slot isolation, runtime-layer refusal
  cases, endpoint coordination, and the state/service lifecycle matrix.
- `gate-nix` exercises the Nix compiler and install tooling: evaluation
  negatives, immutable-source admission, and real install/upgrade adoption in
  throwaway repositories.

The Nix-layer cases are ordinary shell around Nix because they test the compiler
and installer through open-ended Nix builds and external fetches. Putting them
in the runtime task graph would blur which layer is under test and exceed the
bounded role of those framework tasks. See the ownership boundary in
[`CONTRACT.md`](CONTRACT.md) for SEAM-1's exact scope.

The interrupt-and-recover lifecycle scenario remains a white-box Cargo test. It
requires registry access and precise process timing that a bounded leaf task
cannot provide.

The installer executed by `.#gate` is built from the working tree. By default,
however, the generated downstream project pins its Nixfied dependency to the
current Git `HEAD`, so it does not consume other uncommitted framework changes.
When that downstream project must use the working tree, run:

```sh
nix run .#gate -- --dirty
# or run all local stages and pass --dirty to the final gate
nix run .#ci -- --dirty
```

Dirty mode uses a path pin and therefore re-derives the downstream closure.

## Local versus hosted CI

`nix run .#ci` is the canonical full local gate, not a byte-for-byte copy of the
hosted workflow. `.github/workflows/checks.yml` separately runs the raw Cargo
floor, flake checks, and gate; it also runs macOS-specific endpoint observer
tests and builds the optimized release runtime as a final safety net:

```sh
nix build .#nixfied-runtime --no-link
```

The local check/gate path uses the debug runtime to keep iteration fast. The
release package is what install and generated adopter apps ship. Because the
hosted Linux Cargo step is raw, it currently lacks the realised model fixture and
skips the interrupt-and-recover test described above; `.#test` remains the full
fixture-backed local floor.

## Change-specific verification

Use the smallest proof that covers the change, then widen for shared contracts:

| Change | Minimum focused proof |
| --- | --- |
| Rust formatting/lint only | rustfmt + Clippy commands above |
| `nixfied-model` shape/validation | model crate tests + `.#check` |
| Runtime admission or lifecycle | focused runtime test + `.#test` |
| Nix resolution/validation/derivation | `nix flake check` + affected Nix vectors |
| Generated docs or public output | affected model build + `.#gate` |
| Adapter or example | build the affected model + `.#gate` |
| Contract or cross-layer change | `.#ci`; use `--dirty` when the generated project must consume the working tree |

Report any platform, release, or integration coverage that was not run.
