# Distribution build problems and data

Status: measured baseline and follow-up design notes. This is an engineering
evidence ledger, not a second behavioral contract. The normative runtime and
adopter rules remain in [`docs/CONTRACT.md`](docs/CONTRACT.md).

Measured on 2026-08-02 on `aarch64-linux` with 8 CPUs and approximately 15 GiB
of RAM. The commands ran through the pinned development environment with
`nix develop -c cargo ...`. The isolated Cargo target directories were empty for
the cold Cargo measurements; the Nix store already contained the Rust toolchain
and registry inputs. Therefore “cold” below means cold compilation, not a clean
machine or a clean Nix store.

## Executive view

The runtime is not slow because it has an unusually large Rust dependency graph.
The release build is slow because `rusqlite` uses the `bundled` feature, which
causes `libsqlite3-sys` to compile SQLite from C. The measured release build is
about 49 seconds, and the release timing report attributes about 43.4 seconds
to that build script. Runtime Rust library and binary compilation together are
only about 6.6 seconds in the same report.

The relevant conclusions are:

- The runtime release product is the expensive product: about 48.7 seconds in
  an empty Cargo target on the measured host.
- The workspace lock contains 42 package records; the shipped runtime product
  uses 40 of them: two workspace packages and 38 external registry packages.
- The CLI has no production dependencies and compiles in about one second.
  It should not inherit the runtime’s lock/vendor graph or runtime package.
- The debug profile is about 10.6 seconds, but it is an internal check/build
  product. Adopter applications must continue to use the release runtime.
- A Linux-only experiment that forced system SQLite reduced the isolated release
  build to about 9.2 seconds. That is promising, but it is not enough evidence
  to change the default because the runtime uses SQLite WAL, locking, and a
  persistent registry across the supported systems.
- The adopter’s first model/check/smoke times are dominated by Nix evaluation and
  realization of the declared closure. This is a separate phase from compiling
  the runtime binary, and PREPARE-1 deliberately realizes every declared child
  closure before the runtime starts.

The current direction is therefore a layered solution: keep the source/product
split, isolate the dependency-light products with package-specific locks and
vendor inputs, keep the runtime lock canonical, and treat SQLite linkage as a
measured follow-up decision.

## Cargo dependency inventory

The workspace lock contains 42 packages in total: four workspace crates and 38
external registry packages.

| product | direct normal dependencies | production package closure | external packages in closure |
| --- | ---: | ---: | ---: |
| `nixfied-cli` | 0 | 1 | 0 |
| `nixfied-model` | 5 | 25 | 24 |
| `nixfied-runtime` | 8 | 40 | 38 |
| `nixfied-test-child` | 1 (`libc`) | 2 | 1 |

The runtime’s eight normal direct dependencies are `hex`, `libc`,
`nixfied-model`, `rusqlite`, `serde`, `serde_json`, `sha2`, and `thiserror`.
The native build branch is:

```text
nixfied-runtime
└── rusqlite
    └── libsqlite3-sys
        └── bundled SQLite C build
```

The source size also gives useful context. The runtime library has 40 Rust
source files and about 19,345 lines; the model has about 1,620 lines; the CLI
has 535 lines; and the test child has 541 lines. The runtime is substantial,
but its Rust compilation is not the dominant cold-release phase.

## Isolated Cargo measurements

Each cold run used a separate empty target directory. Wall time includes the
development-shell command setup; the Cargo profile time is the time Cargo
reported for the build itself.

| command shape | wall time | Cargo profile time | peak RSS |
| --- | ---: | ---: | ---: |
| runtime, debug | 10.62 s | 10.04 s | 545,636 KiB |
| runtime, release | 48.67 s | 48.57 s | 560,744 KiB |
| runtime, release with timing instrumentation | 52.60 s | — | 557,308 KiB |
| runtime + CLI, release, old combined shape | 50.29 s | 49.68 s | 549,320 KiB |
| CLI, release | 0.96 s | — | about 201 MiB |
| runtime, warm target rebuild | 0.16 s | 0.05 s | — |

The combined runtime-plus-CLI result is the important boundary comparison: a
combined workspace build does not make the runtime’s empty-target compile
materially faster. Splitting the products is still valuable because it prevents
CLI-only changes from invalidating and rebuilding the runtime, and it makes the
CLI’s independent dependency closure observable. It is not, by itself, a fix
for the runtime’s SQLite build script.

The fresh debug timing report showed approximately:

| compilation unit | reported time |
| --- | ---: |
| `libsqlite3-sys` bundled build script | 4.3 s |
| runtime library | 2.2 s |
| runtime binary | 0.9 s |
| `nixfied-model` | 0.6 s |
| `rusqlite` | 0.4 s |

The release timing report showed approximately 43.4 seconds for
`libsqlite3-sys v0.35.0 build-script (run)`, 4.8 seconds for the runtime
library, 1.8 seconds for the runtime binary, 0.7 seconds for the model, and
0.6 seconds for `rusqlite`. Timing instrumentation itself adds variation, so
the ordinary release wall time is the baseline for comparisons.

## Nix distribution layers

The product compile numbers above do not describe all first-use distribution
time. The measured Nix inputs and outputs were:

| item | observed size/shape | interpretation |
| --- | --- | --- |
| pinned `rust-minimal-1.96.0` output | about 130.4 MiB NAR; about 1,012.8 MiB recursive closure | potentially large first-use toolchain acquisition |
| shared Cargo vendor derivation, baseline | about 10.4 KiB direct NAR; about 37.4 MiB recursive closure; 40 entries | build-time registry input for the full workspace lock |
| `nixfied-cli` output | 537,856 bytes NAR; about 57.0 MiB retained closure | small shipped binary, with normal dynamic runtime references |
| `nixfied-runtime` output | 5,111,376 bytes NAR; about 61.4 MiB retained closure | shipped runtime binary and runtime references; build-only Cargo inputs are not retained |
| `install` wrapper | about 60.9 MiB retained closure | installer surface, now wrapping the CLI only |

The retained output closure is not the same as the build input closure. In
particular, the Rust compiler, Cargo registry, and SQLite build toolchain can be
large without being shipped to an adopter. Conversely, a system-linked SQLite
choice can reduce compile cost while adding a runtime shared-library dependency.

The first downstream-shaped adopter run was also decomposed:

| operation | first observed time | warm/repeated observation |
| --- | ---: | ---: |
| `nix run .#install` | 0.73 s | — |
| `nix flake lock` | 6.37 s | — |
| downstream model build | 5.26 s | — |
| first `model-check` | 7.30 s | 0.10 s |
| first `smoke` | 6.04 s | 0.33 s |
| actual smoke runtime result | about 231 ms | — |

The downstream closure retained about 226.3 MiB in this measurement. That is a
consequence of the declared adopter graph and PREPARE-1 realization policy, not
the size of the runtime executable alone. The minimal emitted `model.json` was
3,097 bytes; its direct NAR was about 4.4 KiB, while the current retained Nix
closure for the model build was about 226.3 MiB. A synthetic helper used by the
measurement accounted for about 138.7 MiB of retained closure.

These adopter measurements were warm-store observations with first-use
realization effects. A clean-store, uncached, cross-system benchmark is still
needed before making claims about a new adopter’s total download/build latency.

## SQLite linkage experiment

The runtime currently declares:

```toml
rusqlite = { version = "0.37", features = ["bundled"] }
```

For one isolated `aarch64-linux` release build, the environment forced
`libsqlite3-sys` to use Nix’s SQLite through pkg-config:

```text
LIBSQLITE3_SYS_USE_PKG_CONFIG=1
```

The result was about 9.17 seconds wall time with a peak RSS of 538,240 KiB,
versus about 48.67 seconds for the bundled baseline. The resulting binary was
about 2,985,496 bytes and dynamically referenced `libsqlite3.so`; the bundled
binary was about 5,116,264 bytes and carried SQLite internally, with only the
usual libc/libgcc-style dynamic references. This explains the build-time win:
the expensive C compilation moved from the package build into an already
available system input.

This experiment is not yet a default change. Before changing linkage, prove all
of the following on each supported target family:

- identical SQLite schema, WAL, locking, busy/error, and migration behavior;
- reproducible builds and a stable, declared runtime shared-library closure;
- correct runtime availability in installed/adopter environments;
- supported Linux and Darwin behavior, including cross compilation and Nix
  sandbox inputs;
- release and gate coverage for the actual linked binary, not only Cargo
  metadata;
- whether the smaller binary and faster build outweigh the added dynamic
  dependency and platform-specific packaging risk.

Until that proof exists, bundled SQLite remains the safer reproducibility and
deployment default. The experiment is strong evidence for a follow-up
optimization, not evidence that the current contract is wrong.

## Work executed

The distribution boundary work is now represented by two layers:

1. Commit `88774d1` split the public CLI and release runtime products, created
   package-specific source roots, made the installer CLI-only, and added the
   package/source/output boundary check.
2. The follow-up in this working change adds checked package-specific Cargo
   locks for the dependency-free CLI and libc-only test child. The runtime
   continues to use `runtime/Cargo.lock`; `runtime-source.nix` writes the
   selected lock into the filtered root; and `package-boundaries.nix` validates
   each root with `cargo metadata --locked --offline` and checks its external
   package set.

This changes build-input ownership, not the model, runtime command/output,
capability descriptor, lifecycle behavior, or ABI. No Rust source, model type,
or runtime contract change is intended.

The package-specific lock decision trades a small amount of lock maintenance for
two concrete properties:

- CLI builds no longer need to import the full 38-package registry graph merely
  because they share a workspace root.
- Test-child builds no longer need the runtime/model graph; their lock records
  only `libc`.

The runtime remains intentionally coupled to the complete canonical lock. It is
the product that actually needs `rusqlite`, hashing, serialization, and the
model crate, so reducing its lock would only obscure or weaken the reproducible
dependency boundary.

## Architect discussion and decisions

The architecture review converged on these points:

- Source filtering and package-specific lock/vendor inputs solve different
  problems. Source filtering controls derivation identity and invalidation;
  package locks control Cargo metadata and build-time registry inputs.
- The package split is a phase-one boundary correction. It prevents unrelated
  source changes and dependency-light products from dragging the runtime build
  along, but it cannot remove runtime compilation cost.
- The runtime’s release/debug distinction is intentional. Debug is useful for
  framework checks; release is required for adopter-facing generated apps.
- SQLite linkage is the highest-value optimization candidate, but it should be
  a separate measured change after cross-platform and behavioral proof.
- A benchmark must separate Cargo target warmth, Nix store warmth, dependency
  acquisition, Nix evaluation, derivation realization, and actual runtime
  execution. A single end-to-end stopwatch hides those decisions.
- The earlier temporary benchmark scaffold was removed because its “clean” mode
  did not isolate the Nix store, it realized framework products before the
  adopter, and its shell-function command wrapper failed at the lock step. The
  measurements in this file came from direct commands instead; no misleading
  benchmark helper remains in the tree.

## Remaining questions and recommended order

1. Run a real clean-store benchmark for CLI, runtime, install, and a minimal
   adopter on each supported system. Record download bytes, evaluation time,
   realization time, Cargo compile time, peak RSS, and runtime execution time
   separately.
2. Measure the effect of downstream `nixpkgs.follows` convergence. This can
   remove duplicate Nixpkgs closure materialization in adopters, but it should be
   measured rather than assumed.
3. Keep validating the package-specific locks whenever workspace manifests or
   dependencies change. The boundary check is the proof that reduced roots stay
   locked and offline-valid.
4. Run the SQLite system-link experiment through the supported Linux and Darwin
   build paths and the registry/lifecycle tests before considering a manifest
   change.
5. Only after those measurements evaluate Rust profile tuning, compiler/toolchain
   changes, or further Nix evaluation deduplication. These may improve seconds,
   but they do not address the separate clean-store closure question.

## Verification record

The following evidence was collected before or during this follow-up:

- `cargo metadata` and `cargo tree` were run through `nix develop -c cargo`.
- Isolated debug/release Cargo builds, release timing output, warm rebuilds,
  package-size inspection, and the SQLite A/B build were run on `aarch64-linux`.
- The direct adopter workflow was exercised through installer, lock, model,
  model-check, and smoke operations.
- The source/product split was reviewed and committed in `88774d1`.
- The obsolete benchmark helper was removed in `8448aa8`.

After the package-specific lock changes, the focused package-boundary check,
public CLI/runtime/install builds, flake evaluation, `.#check`, `.#test`, and
`.#gate --dirty` all passed in the current worktree. The ordinary `.#gate`
invocation was also run, but it intentionally pinned `HEAD` rather than the
dirty worktree and failed its package-boundary assertion; it is not evidence
against this change. The aggregate `.#ci` wrapper was not rerun because its
three component stages were run individually. The full boundary build may
require several gigabytes of uncached Nix inputs, so any interrupted or
unavailable realization must be reported as unverified rather than treated as
a passing result.

References: [`nix/packages/runtime.nix`](nix/packages/runtime.nix),
[`nix/packages/runtime-source.nix`](nix/packages/runtime-source.nix),
[`nix/checks/package-boundaries.nix`](nix/checks/package-boundaries.nix),
[`runtime/Cargo.toml`](runtime/Cargo.toml),
[`runtime/crates/nixfied-runtime/Cargo.toml`](runtime/crates/nixfied-runtime/Cargo.toml),
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md), and
[`docs/CONTRACT.md`](docs/CONTRACT.md).
