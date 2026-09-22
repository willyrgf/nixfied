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
nix/docs/                      revision-bound reference and native query dispatcher
nix/meta/                      private option/publication checking machinery
nix/packages/                  reproducible Rust builds and source checks
nix/packages/runtime-source.nix package-specific filtered Cargo roots
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
```

`nix flake check` is the hermetic source/build core. Its `rust-workspace`
derivation first proves that [`OPTIONS.md`](OPTIONS.md) matches the typed Nix
modules, checks product-specific generated Rust freshness and structural policy
vectors, then runs rustfmt and Clippy with `-D warnings`; Clippy type-checks all
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
The source-isolation matrix in `nix/gate-nix.nix` varies CLI, runtime, model,
generated-model and test-child sources independently and compares filtered-source
and product derivation identities. Reference and declaration-only variations keep
generated bytes fixed and must leave every Rust product identity unchanged.

Model wire declarations live in `nix/meta/model.nix`; public runtime records and
error/status vocabularies live in `nix/meta/outputs.nix`. Their structural checker,
Nix constructors, Rust renderer and static reference share the same normalized
records; closed enum members come from `capability.txt`. Native identifier,
loopback, graph, admission and execution rules remain in their existing owners.
Regenerate checked-in Rust after changing the shared structure or inventory:

```sh
nix develop --command nixfied-regenerate
```

The development shell supplies this Nix-packaged application. Its generated-file
input uses the locked Nix inputs and pinned rustfmt; run it from the repository
root. Shell orchestration lives in Nix-packaged applications with declared tools,
not standalone shell files. Ordinary Cargo and package builds compile checked-in
generated files without regeneration or an
overlay. They do not prove freshness; the `rust-workspace` check does. The Nix
gate also mutates one inventory vocabulary in an isolated current-source copy
and checks the actual native option domain, Rust decoder/encoder and reference.

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

## Authoring and publication ownership

Native option declarations use `nix/meta/options.nix` to check metadata before
calling Nixpkgs `mkOption`. `nix/modules/invocation.nix` is an ordinary shared
option fragment, mounted at task, start and probe invocation sites. Native types,
merging, defaults, `apply`, required-value laziness and compiler relational
validation keep their existing owners. The positive-integer domain uses the
native `types.ints.positive`, making its bound visible in the reference.

Publication descriptors own actual library, adapter, injected-argument, app,
package, check and shell names and descriptions. `nix/meta/publications.nix`
checks the complete metadata assembly and references before projecting lazy
native bindings. `nix/modules/providers.nix` supplies the same raw bindings to
declaration-only evaluation before the checked facade exists. Final-export
name audits run through `rust-workspace`, never as projection dependencies.

This replaces the direct option-constructor imports in the four declaration
modules, the adapter export attrset, the compiler's hand-maintained reserved-app
list, framework-app attrsets and duplicate help-description ownership, and the
five manually named flake export attrsets. Native app scripts, config-derived
verbs, package builders and module/compiler behavior remain in their owners.

`packages.<system>.docs` and both docs apps use `nix/docs/reference.nix`.
Its private presentation data comes from native option normalization, checked
publication metadata, existing prose and the supplying source identity. The
serialized presentation boundary discards string context; native values and
executable bindings retain it. Static docs do not import an adopter module or
realise model/runtime products. `docs/OPTIONS.md` retains its checked snapshot
and upgrade-report role; the full API reference is a disposable build artifact.

The existing `rust-workspace` check includes `nix/checks/option-metadata.nix`,
`publications.nix`, and the private `reference.nix` check. They cover native
mounts/overrides/defaults/apply, malformed metadata, hidden/manual bypasses,
whole-assembly validation, lazy bindings, scope identities, exact docs queries,
and presentation-context/closure isolation. The Nix gate runs
the reference-source checks in `nix/gate-nix.nix` against two distinguishable
current-source copies and pinned downstream flakes, including poisoned project values and
caller-directory independence. Run `nix run .#gate -- --dirty` when these fixtures
must consume uncommitted sources.

Maintenance examples live in the option check: adding a documented string to
the actual shared invocation fragment yields both mounted reference entries and
native override behavior without a renderer change. Changing one fixture bound
from one to two changes native acceptance and both mounted type descriptions;
literal expected values independently assert the result. The publication check
similarly adds supported descriptors without extending the checker or a name
registry. Runtime lowering separately proves descriptive refs remain serialized
but do not change service reuse identity.

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
floor, flake checks, and gate; it also runs macOS-specific endpoint observer tests
and builds the public CLI, installer, and optimized release runtime as a final
safety net:

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
| Package/build change | CLI/runtime/install builds |
| Generated docs or public output | affected model build + `.#gate` |
| Adapter or example | build the affected model + `.#gate` |
| Contract or cross-layer change | `.#ci`; use `--dirty` when the generated project must consume the working tree |

Report any platform, release, or integration coverage that was not run.

## Command syntax ownership

`nix/meta/commands.nix` declares the seven baseline command syntaxes.
`syntax.nix` checks domains, defaults, visibility, references and generated names
without invoking native help functions. `command-help.nix` supplies ordinary row
and compact-usage formatting; `syntax-project.nix` emits Rust constants/type
aliases and quoted shell constants. The existing runtime, installer and upgrade
parsers retain input collection, entry routing, acquisition, repetition, errors
and all effects. Generated app wrappers consume the same tokens.

`generated-files.nix` routes structure and syntax fragments to existing native
include scopes. `nix develop --command nixfied-regenerate` formats them with the
pinned toolchain. Product builds compile checked-in files; the source
check compares freshly rendered files for both runtime and CLI. Its synthetic
structural fixture supplies its own file map to the same formatting step.

`nix/checks/syntax.nix` checks malformed declarations, help laziness, seven exact
help fixtures and an added optional argument without a formatter edit. Runtime
and CLI command tests exercise native parsing and encoding. The source check
also runs `nix/checks/upgrade-syntax.nix` against the packaged shell parser;
existing adoption and upgrade goldens cover its continuation. The gate's
source-isolation matrix in `nix/gate-nix.nix` varies CLI-generated sources as well
as model, runtime-generated, native crate and reference/declaration inputs. It requires
changes only in the owning Rust product's filtered source and derivation.

## Adopter API maintenance

The original delivery audit and recovery receipts are preserved in commit
`48fc137`. Current checks grade the working tree; a historical receipt is not
acceptance evidence for later changes.

| Owner | Proof |
| --- | --- |
| Native options and shared invocation fragment | `option-metadata.nix`: mounted suboptions, overrides, bounds, lazy defaults and context |
| Publication metadata and actual exports | `publications.nix`, final flake audits, poisoned bindings and downstream app-set checks |
| Structural fields and closed inventory vocabularies | `structure.nix`, `coverage.nix`, compiled projection fixtures, raw `wire_presence.rs` and independent Nix/Rust derivation vectors |
| Native output/error views | `output_structure.rs`, main/output/registry/process tests and real redaction/write boundaries |
| Shared command facts and native parsers | `syntax.nix`, seven exact helps, native parser/encoding tests and packaged upgrade parser |
| Static revision-bound reference | `reference.nix`, downstream source switching, context/closure isolation and exact queries |
| Product source isolation and generated freshness | `gate-nix.nix`, `generated.nix`, packaged `nixfied-regenerate` |

`nix/meta/declarations.nix` contains ordinary shared value/field constructors.
Fields select coherent presence alternatives; the checker derives decoder and
Rust emission facts:

| Presence | Decoder meaning | Rust emission |
| --- | --- | --- |
| Required | Member required | Present |
| Optional | Missing or null means absence | Present, including null |
| OptionalOmitted | Missing or null means absence | Omit absence |
| Empty | Missing means empty collection | Present |
| EmptyOmitted | Missing means empty collection | Omit empty |
| EnumDefault(member) | Missing means the named inventory member | Present |
| OmitEmpty | Serialization-only collection; no decoder | Omit empty |

Defaults are limited to empty collections and explicit enum members. Empty defaults
use native Default; the enum wire default remains independent of convenience
Default. There is no recursive decoder-default interpreter or runtime JSON parsing
of defaults. Arbitrary literal/record defaults and required-nullable fields are
unsupported. Required OpenJson rejects a missing member and accepts null.

Records declare producer ownership (`Nix` or `None`); only Nix-produced fields
carry a Nix policy. `RequiredPresent` requires and emits an input even when the
decoder has a default. `RequiredOmitAbsent` requires an optional input and omits
null; `RequiredOmitEmpty` requires a collection and omits empties.
`PreserveSupplied` permits missing input only when decoding accepts omission,
and retains explicitly supplied values, including empties. Outer constructors
accept pre-emission values; nested record values must already obey their Nix
emission policies. Neither boundary reconstructs or defaults a child record.

Record identities are either `Inventory(coordinate)` or `Local(name)`. Local
identities cannot evade existing inventory coverage. References and generated
names must resolve without collisions; recursive structural record graphs are
unsupported. Model decoders reject unknown fields; output records have no
decoder unless an actual consumer needs one, with its explicit unknown-field
policy retained.

The raw option audit uses native `option.loc` and `type.getSubOptions`, including
keyed mounts, before documentation filtering. Framework options cannot hide
behind visibility settings; native `_module` exclusions remain explicit.
Metadata inspection preserves lazy defaults, examples, configured values and
bindings. Nixpkgs still owns option-documentation normalization.

Shared command arguments retain one token/type/default binding across native
consumers. The checked symbol names also drive collision checks and projection.
Native parsers retain acquisition/repetition/encoding/help precedence and effects.

`maintenance.nix` proves an added required wire field and optional command argument
reach generated definitions, small native consumers and the packaged reference.
The command consumer compiles separately within the native test scope; it never
rewrites the production parser. Actual parser goldens remain in their native tests.
The option-bound/export exercises remain in their existing boundary checks.
`cargo-fixture.nix` shares only isolated fixture-build plumbing, not product behavior.

The raw option audit and independent no-child, redaction, cleanup and OS ownership
proofs remain required. Reductions in generated formatting or historical prose
must be accounted separately from maintained production/test code.
