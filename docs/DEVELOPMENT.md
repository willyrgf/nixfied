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
The Nix gate runs `nix/checks/runtime-sources.sh`: it varies CLI, runtime, model,
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
bash nix/meta/regenerate.sh
```

This operation uses the locked Nix inputs and pinned rustfmt. Ordinary Cargo and
package builds compile checked-in generated files without regeneration or an
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
`nix/checks/reference-sources.sh` against two distinguishable current-source
copies and pinned downstream flakes, including poisoned project values and
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
include scopes. The single `bash nix/meta/regenerate.sh` operation formats them
with the pinned toolchain. Product builds compile checked-in files; the source
check compares freshly rendered files for both runtime and CLI. Its synthetic
structural fixture supplies its own file map to the same formatting step.

`nix/checks/syntax.nix` checks malformed declarations, help laziness, seven exact
help fixtures and an added optional argument without a formatter edit. Runtime
and CLI command tests exercise native parsing and encoding. The source check
also runs `nix/checks/upgrade-syntax.nix` against the packaged shell parser;
existing adoption and upgrade goldens cover its continuation. The gate's
`nix/checks/runtime-sources.sh` now varies CLI-generated sources as well as model,
runtime-generated, native crate and reference/declaration inputs. It requires
changes only in the owning Rust product's filtered source and derivation.

## Adopter API delivery audit

This audit records the five owner cutovers from the reviewed handoff at
`d5562f1a79d4f46af6ef3afde782e4c5d1152ba5`, whose production baseline is
`c3d9111c861d40b0885a49f3d62950f9871702e1`. The implementation was written from
that reviewed tree without consulting or recovering the abandoned implementation.
The committed install/upgrade golden archives remained verification fixtures,
as explicitly allowed by the handoff. No commit was made for this delivery.
The RFC remains the settled design and acceptance checklist; this section
records implemented owners and proof entry points, rather than another plan.

The corrective review retained the same owners and fixtures. Structural matching
now distinguishes `NixWire` from `DecoderLiteral`: nested emitted values obey
Nix omission policies, while outer constructor inputs still accept values to
omit. Decoder literals reject encountered native values and nonempty unique
lists containing record references; record literals require decoders. Empty
collections and optional absence/null remain supported. The existing structure
vectors cover these restrictions, including raw-distinct records that decode
identically. Default admission also uses the existing `isJson` helper to reject
non-JSON ignored members before generation; ignored JSON values remain accepted.

Option metadata is audited on native records and `getSubOptions` before Nixpkgs
renders it, preserving native `option.loc` through keyed mounts. The mounted
prefix and ignored-function regressions both failed before their boundary fixes.
Redundant rendered-entry checks were removed. Publication tests
observe resolver specialArgs and adapters with poisoned packages: two positive
audits and four added/missing-name negatives. The existing downstream seven-app
check remains the project-app proof. Both Rust generators share `rust-quote.nix`,
and the compiled structural fixture asserts independent bytes for an unusual
wire field name. Help padding has a minimum of one space; the existing budget
fixture uses a long metavar and checks native help and packaged reference.
Production generated Rust and the baseline help fixtures remain unchanged.

### Coverage and retained native owners

The checked reference contains 128 option paths; three library functions, three
adapter modules and four injected arguments; 23 package/check/shell identities;
15 root/project app identities; seven commands; 48 records; 23 closed
vocabularies; and all 27 error codes. Package aliases remain distinct exports.
Project verbs still derive directly from configured task names/descriptions.
`coverage.nix` accounts for every authored capability coordinate exactly once;
final export audits separately inspect the actual flake namespaces and app sets.

| Surface or inventory coordinates | Owner and reference route | Independent proof |
| --- | --- | --- |
| Native options, nested invocation mounts, module arguments, adapters, authoring algebra | Native modules/compiler; `docs options`, `docs api`, authoring/adapters/context topics | `option-metadata.nix`, `publications.nix`, compiler negatives and adopter fixtures |
| All 29 `primitive` records; model enums and signals | `meta/model.nix`, shared structure checker; model/derivation topics | Raw presence vectors, model validation, Nix/Rust derivation goldens, admission tests |
| All 19 result/error bindings: 13 Owned, five Borrowed, one MemberNamesOnly | `meta/outputs.nix`, same checker/renderer; record/error queries and outputs/errors topics | `output_structure.rs`, native main/output/process/status tests, real output and registry tests |
| `surface`, `surface-help`, run output modes, install and upgrade | `meta/commands.nix`, native parsers; command queries and commands topic | Seven exact helps, real parser/encoding/precedence vectors, packaged upgrade parser and adoption goldens |
| `model-version` | Native constants and model admission; model topic | Capability digest/snapshot, model and admission tests |
| `substitution` | Compiler and native execution lowering; placeholders topic | Compiler negatives, lowering vectors and service/endpoint tests |
| `endpoint-acquisition`, `endpoint-reuse` | Native service process/kernel/registry proof; runtime topic | Cross-root endpoint fixtures, exact reuse, listener ownership and readiness tests |
| `lease-authority` | Native registry and lifecycle; runtime/state topics | Open borrower/owner refusal, expiry, reservation and persistent/until-idle tests |
| `escape-settlement`, `escaped-port-reconciliation` | Native registry/process/control/cleanup; runtime/state topics | Atomic transition/row-count tests, unresolved escape, down and cleanup refusal tests |
| `output-schema run-summary-text`, `run-error-summary-text`, `run-task-output` | Native main/output/task finalization; outputs/runtime topics | Human/JSON/binary output, default selection, redaction, failure/replay and gate fixtures |
| State/secret directories, source roots and hermetic child context | Native placement, admission and execution; context/secrets topics | Isolated `context.rs`, actual file material, admission source tests, hermetic child tests and optional Linux port advisory |
| State epochs/markers, containment, liveness, cancellation, cleanup and recovery | Native state/registry/service/control; state/runtime/recovery topics | State, registry, lifecycle, service, endpoint and upgrade suites |
| Install/upgrade ownership, reports, transactions and preflight | Native CLI and upgrade script; authoring/recovery topics | CLI preservation/refusal cases and committed upgrade golden/adoption checks |
| Help/docs queries, source identity and fixed model seam | Native help/query/compiler view owners; discovery/model topics | Reference queries, poisoned dependencies, revision switching, source/closure isolation and model/help gate checks |

Known inventory gaps remain explicit: CheckOutput, DownReport, CleanupOutcome
and RegistryIdentityDiagnostic use local record identities; OutputStream and
ProjectionOperation remain native scalar serializers. Their public fields and
spellings are documented and tested without silently adding inventory entries.
Status transition/terminal policy, open error-detail production, host observations,
text/byte emission and cleanup decisions stay native. No context/text/byte
schema or behavior dispatcher was introduced.

### Replacement ledger

| Cutover | Removed ownership | Resulting owner |
| --- | --- | --- |
| Authoring/publication | Direct option-constructor imports and repeated invocation declarations; adapter export/reserved-name catalogs; separate public export/name/description attrsets | Checked native option wrapper, shared invocation fragment, provider/publication descriptors and actual final-export audit |
| Model structure | Handwritten model structs/enums and repeated native enum membership lists; Nix wire-shaping omission/filtering copies | One checked structure vocabulary, inventory-linked enum members, lazy per-field Nix construction and generated native Rust includes |
| Results/errors | Handwritten result/error payload structs, diagnostic object shapes, conflict envelope key, error/status enum definitions and status macro member lists | The same structure mechanism; borrowed temporary views at native serialization sites; native macro implementations and producer policy retained |
| Command syntax | Five runtime help constants, installer usage/default pin, upgrade syntax/default/help literals, duplicated parser/wrapper tokens and handwritten RunOutputMode | One syntax checker/projection and ordinary native help formatters; all seven native loops, acquisition rules and continuations retained |
| Reference/delivery | Disconnected option-only discovery and an implementation-start handoff | One static source-bound docs product, exact queries and native topics; this implemented-owner/verification audit |

The runtime's unused direct `thiserror` dependency was removed; the model's
existing dependency remains. No new Cargo/Nix input dependency, semantic seam,
mutable authority, compatibility reader or migration alias was added. All model
bytes still cross `model.json`; the runtime neither evaluates Nix nor consumes
the reference artifact. Generated files are checked in and freshness-checked;
ordinary product builds never regenerate or overlay them.

The separate non-UTF-8 environment-secret correction is the explicit baseline
amendment in the handoff. Its before-fix regression failed, and its four-mode
binary proof now checks status 33, unchanged error code/class, no rejected
material, and no child/state effects. This restores REDACT-1 without changing
capability bytes or versions. It is not attributed to structural generation.

### Maintenance traces and cost

| Exercise | Authored change sites | Derived outputs and independent evidence |
| --- | --- | --- |
| Ordinary option | Shared invocation option fragment fixture plus nested expected override | Task/start/probe native mounts and reference paths; no compiler or renderer edit |
| Shared bound | One fixture domain minimum changes from one to two | Both mounted native types/descriptions change; literal one-rejects/two-accepts vectors; unrelated runtime timeout domains stay separate |
| Required wire field | TaskSpec-shaped local fixture adds one boolean; constructor inputs and native use change | Nix construction, generated Rust and record reference; missing/null/wrong-type rejection and true/false use. A real inventoried amendment additionally requires ABI inventory/snapshot/producer/consumer updates |
| Command argument | Run fixture adds optional Unsigned64 syntax, native parsed state/branch and timeout continuation | Generated token/type/default/help/reference; actual native parser operand, overflow and repetition checks. The long metavar also exercises minimum row padding |
| Native behavior | Two native fixture failure-selection branches and independent byte expectations | Same generated error type and metadata, different primary/cause values with unchanged shape |
| Supported export | Publication descriptor and native binding fixture | Actual selected export/reference and audit observe the addition without checker or name-registry edits |

`nix/checks/maintenance.nix` implements the wire/argument/native examples in a
copied test workspace. Insertions reject native-source drift; fixture-only
sources are never installed into products. The reference builder accepts the
same checked metadata as normal delivery. The option/publication exercises live
in their existing boundary checks. This is executable change-site evidence,
not a promise that metadata proves native behavior.

Measured handwritten machinery footprint: 2085 lines across the checked
metadata/projection/reference/query/generation components listed in the delivery
receipt. Structural and command declarations: 1194 lines; publication
and native option declarations/bindings are additionally visible in their native
files. Checked-in generated Rust: 1346 lines. Fixture scripts and tests,
changed native code and prose are included in the gross diff, not hidden inside
the machinery estimate.

Gross implementation diff against the reviewed handoff: 113 files,
+10172/-1386 lines. Against the agreed production baseline, including the
three reviewed design documents: 114 files, +12071/-3407
lines. Counts include this audit and generated outputs. The largest growth is
explicit field policies/declarations, generated definitions and independent
boundary/OS/maintenance evidence. The structure checker (694 lines) and syntax
checker (247 lines) exceed their preliminary component estimates because they
check bounded reference/storage/presence and generated-name combinations. They
remain one structural pass and one syntax pass; no extra behavior engine or
parallel object graph accounts for the excess. Source/regeneration checks reuse
the same renderers and pinned formatter.

### Stable acceptance evidence

| Acceptance | Evidence entry point |
| --- | --- |
| A1 clean origin | Recorded reviewed HEAD/production baseline and per-unit frozen source receipts; abandoned implementation excluded |
| A2 complete coverage | 128-path native collection, actual final-export audits, `coverage.nix`, command/record/error queries and native-topic routing above |
| A3 native authoring | Option/module override/default/apply/laziness/provider fixtures and poisoned project dependencies |
| A4 structural ownership | Shared structure checker/renderer, independent presence/unknown-field/raw-byte tests, retained native domain checks and conversions |
| A5 independent admission | Malformed model/false derivation negatives, no-child admission tests, independent Nix and Rust goldens |
| A6 command preservation | All seven exact helps and section-7 native parser fixtures; upgrade continuation independently byte-compared after parsing |
| A7 native behavior | Output/registry/state/lifecycle/service/endpoint suites, isolated context and secret regression; metadata is not OS proof |
| A8 product delivery | Static docs root/project apps, exact queries, stateRefs/placeholder answers, source provenance and useful failure tests |
| A9 isolation/freshness | Reference revision/poison/closure tests; CLI/runtime/model/test-child/generated-source variation matrix; pinned regeneration diffs |
| A10 complete replacement | Ledger above, native owner scans and no competing implementation-start assignment |
| A11 demonstrated maintenance | Six executable traces above, full component and gross-diff accounting |
| A12 final verification | Frozen-tree `nix run .#ci -- --dirty`, all-system evaluation and affected release products, recorded with exact source/diff identities |

Execution and release validation cover aarch64-linux. aarch64-darwin and
x86_64-linux have evaluation coverage only; hosted CI and macOS execution are
not claimed. Final delivery receipts identify the exact checked tree and logs;
an earlier unit's green gate is not final acceptance.
