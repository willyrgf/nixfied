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
nix/packages/runtime-source.nix package-specific filtered Cargo roots
nix/checks/package-boundaries.nix package/source/public-output boundary proof
nix/help-*.nix                 private contextual app catalog
nix/project-apps.nix           discovery/controls + adopter-exported task apps
nix/gate-runtime/nixfied.nix   adopter-shaped runtime integration gate
nix/gate-nix.nix               Nix compiler/install integration gate
nix/gate.nix                   thin gate coordinator
nix/dev.nix                    local check, test, and ci app definitions
runtime/crates/nixfied-model/   serde contract, validation, capability descriptor
runtime/crates/nixfied-runtime/ admission and impure runtime behavior
runtime/crates/nixfied-cli/     install CLI
runtime/crates/nixfied-test-child/ private process/socket test fixture
runtime/locks/                  package-specific Cargo locks for light products
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

Use `.#test` for the complete Cargo floor. It injects the realised Postgres model
required by `interrupt_and_recover_adopts_orphaned_postgres`; a raw
`cargo test --workspace` without `NIXFIED_TEST_POSTGRES_MODEL` intentionally
skips that case.

Rust integration tests that exercise spawned process and socket behavior use a
private Nix-built child executable. Run those tests through `nix develop` or
`.#test`; raw Cargo outside that environment has no `NIXFIED_TEST_CHILD` fixture
and fails loudly rather than skipping coverage.

For a targeted Rust iteration inside the pinned development environment:

```sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-model'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test admission'
nix build .#nixfied-cli --no-link
nix build .#nixfied-runtime --no-link
nix build .#install --no-link
nix build --impure .#checks.$(nix eval --impure --raw --expr builtins.currentSystem).package-boundaries --no-link
```

`nix flake check` is the hermetic source/build core. Its `rust-workspace`
derivation first proves that [`OPTIONS.md`](OPTIONS.md) matches the typed Nix
modules, then runs rustfmt and Clippy with `-D warnings`; Clippy type-checks all
targets. The flake checks also build the debug runtime and minimal example model,
and run the Nix derivation golden vectors. The process- and port-using Cargo tests
run outside the Nix sandbox through `.#test`.

The public package surface contains `nixfied-cli` and the release
`nixfied-runtime`; `install` wraps only the CLI. The debug runtime and
`nixfied-test-child` are private transitive inputs of checks, gates, the dev
shell, and the fixture-backed test wrapper. `runtime-source.nix` creates a real
Cargo workspace root for each product, so CLI, runtime/model, and test-child
source changes do not invalidate unrelated product derivations. The Cargo lock
and vendor derivation are package-specific for the dependency-free CLI and the
libc-only test child. The runtime/model product intentionally retains the
canonical full-workspace lock because it owns the complete runtime dependency
closure. Rust toolchain and workspace-metadata changes can still invalidate
multiple products; package-specific lock changes are now isolated to the product
that owns them. The filtered roots carry the lock selected by their package map,
and `cargo metadata --locked --offline` validates that no transient lock
generation is needed.
The focused boundary command is impure so its source-variant matrix runs on the
native Linux target; pure cross-system flake evaluation keeps the structural
root/output proof without trying to realise a foreign test copy.

Regenerate the checked option reference after changing `nix/modules/`:

```sh
generated="$(nix build --impure --no-link --print-out-paths --expr '
  let
    flake = builtins.getFlake ("git+file://" + builtins.getEnv "PWD");
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
The Git flake URL deliberately excludes ignored build artifacts from the source
snapshot while retaining changes to tracked module files.

## Gate composition

`nix/gate.nix` is a thin sequential coordinator:

- `gate-runtime` is a first-class Nixfied model. It exercises example runs, the
  emitted model/docs contract, concurrent slot isolation, runtime-layer refusal
  cases, endpoint coordination, and the state/service lifecycle matrix.
- `gate-nix` exercises the Nix compiler and install tooling: final-app
  discovery, evaluation negatives, immutable-source admission, and real
  install/upgrade adoption in throwaway repositories.

The Nix-layer cases are ordinary shell around Nix because they test the compiler
and installer through open-ended Nix builds and external fetches. Putting them
in the runtime task graph would blur which layer is under test and exceed the
bounded role of those framework tasks. See the ownership boundary in
[`CONTRACT.md`](CONTRACT.md) for SEAM-1's exact scope.

The upgrade adoption cases use the pinned source fixtures in
[`nix/fixtures/upgrade-golden/`](../nix/fixtures/upgrade-golden/) rather than
constructing ad hoc documentation trees at test time. `manifest.json`
records the historical source commits, deterministic archive checksums, NAR
hashes, archive normalization, and the checksum of `expected.diff`. The old
archive is installed with its historical `#install`; the current checkout's
`#upgrade` then resolves the new archive and must reproduce `expected.diff`
byte-for-byte on both `--plan` and apply. The same fixed trees are reused for
Git and path identity checks, while tarball and unavailable-source cases retain
separate locked-identity coverage; an unsupported locked scheme is also
required to fail before emitting a partial report or mutating the project. The
checked-in scope patches and `scope.expected.diff` separately prove README
changes plus documentation additions and deletions.

Refresh these fixtures only for an intentional upgrade-behavior change. Export
each source revision into an empty directory, then create the archive with
sorted names, epoch timestamps, numeric zero ownership, and `gzip -n` (the
normalization is recorded in the manifest). Recompute the archive SHA256 and
NAR hash, regenerate the exact stdout golden from the current upgrade command,
regenerate `scope.expected.diff` if the deterministic scope overlays change,
and update `scope-old.patch`, `scope-new.patch`, `expectedDiffSha256`, and
`scopeExpectedDiffSha256` together with the fixture. Verify that the historical
installer still produces the list-form declaration used by the rejection case,
that the compatible case still passes model preflight, and that plan/apply
stdout remains identical. The fixture freezes source versions and report
bytes; it does not make the nested Nix dependency closure offline.

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
floor, flake checks, package-boundary check, and gate; it also runs macOS-specific
endpoint observer tests and builds the public CLI, installer, and optimized
release runtime as a final safety net:

```sh
nix build .#nixfied-cli .#install --no-link
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
| Task-output replay/projection | `cargo test -p nixfied-runtime --test output` + `.#gate -- --dirty` |
| Nix resolution/validation/derivation | `nix flake check` + affected Nix vectors |
| Package/source/public output boundary | `package-boundaries` check + CLI/runtime/install builds |
| Generated docs or public output | affected model build + `.#gate` |
| Adapter or example | build the affected model + `.#gate` |
| Contract or cross-layer change | `.#ci`; use `--dirty` when the generated project must consume the working tree |

Report any platform, release, or integration coverage that was not run.
