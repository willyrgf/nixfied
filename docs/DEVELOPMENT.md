# Developing Nixfied

This guide owns repository layout, test placement, and verification commands.
Read [`CONTRACT.md`](CONTRACT.md) before changing the manifest/runtime boundary and
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the rationale behind it.

## Repository map

```text
nix/modules/                   typed adopter declaration surface
nix/compiler/                  resolve -> validate -> derive -> emit manifest/docs
nix/spec/                      Nix-side manifest and ABI constants
nix/adapters/                  Nix-side domain adapters
nix/install/                   install and upgrade programs
nix/install/upgrade-helper/    private Rust URL editing and guarded file application
nix/lib/                       pure Nix helpers
nix/docs/                      revision-bound reference and native query dispatcher
nix/meta/                      private option/publication checking machinery
nix/packages/                  reproducible Rust builds and source checks
nix/packages/runtime-source.nix package-specific filtered Cargo roots
nix/help-*.nix                 private generated app catalog
nix/project-apps.nix           discovery/controls + adopter-exported task apps
nix/gate-runtime/nixfied.nix   adopter-shaped runtime integration gate
nix/gate-nix.nix               Nix compiler/install integration gate
nix/gate.nix                   thin gate coordinator
nix/dev.nix                    local check, test, and ci app definitions
runtime/crates/nixfied-manifest/   serde contract, validation, capability descriptor
runtime/crates/nixfied-runtime/ admission and impure runtime behavior
runtime/crates/nixfied-cli/     install CLI
runtime/crates/nixfied-test-child/ private process/socket test fixture
runtime/locks/                  package-specific Cargo locks for light products
examples/                       downstream-shaped example manifests
```

There is no root-level `tests/` directory. Rust manifest/runtime behavior lives in
`runtime/crates/*/tests` and crate-local unit tests. Pure Nix derivation vectors
live in `nix/checks`; Nix compiler/install integration cases live in
`nix/gate-nix.nix`; adopter-shaped runtime integration lives in
`nix/gate-runtime/nixfied.nix`.

Choose tests by the behavior they can disprove. Prefer a complete use case through
the existing compiler, adapters, runtime commands, or registry API, with an
independent expected result: returned child output, durable history, isolation,
recovery, or rejection before effects. Reuse existing examples before adding
hand-authored manifests or test builders. Do not add helpers that reproduce
production derivation or validation to calculate the expected answer.

Do not test field access, derived equality, enum round trips, or fixture validity
in isolation when compilation and existing boundary tests already cover them.
Keep focused deterministic algorithm proofs (graph derivation, redaction), wire
rejection tests, and controlled OS/registry fault tests: a valid Rust value does
not prove that untrusted bytes, concurrent processes, or damaged state are safe.
Remove a test only after identifying the type guarantee or remaining behavioral
proof that replaces it; fixture size alone is not evidence of redundancy.

## Canonical local checks

Run the full local repository gate with:

```sh
nix run .#ci
```

The private `nix/install/upgrade-helper` Cargo package is isolated from the
installer and process runtime. Building `.#upgrade` checks source edits and
concurrent-write handling, formatting, and Clippy. Nix owns resolution and
reporting; the helper owns source edits and guarded application. The Nix gate
tests public upgrade behavior, including actual interruption and rollback.

It is fail-fast and runs these local stages:

1. `.#check` — `nix flake check`, then admission of a realised example manifest.
2. `.#test` — the white-box Cargo workspace floor under the pinned toolchain.
3. `.#gate` — the runtime-shaped gate followed by the Nix-layer gate.

Use `.#test` for the complete Cargo floor. It injects the realised Postgres manifest
required by `interrupt_and_recover_stops_orphan_and_starts_fresh_postgres`; a raw
`cargo test --workspace` without `NIXFIED_TEST_POSTGRES_MANIFEST` intentionally
skips that case.

Rust integration tests that exercise spawned process and socket behavior use a
private Nix-built child executable. Run those tests through `nix develop` or
`.#test`; raw Cargo outside that environment has no `NIXFIED_TEST_CHILD` fixture
and fails loudly rather than skipping coverage. Lifecycle fixtures also receive
`NIXFIED_TEST_SLEEP` and `NIXFIED_TEST_FIXTURES` from the same Nix environments.
`nix/checks/runtime-fixtures.nix` packages the small shell programs; Rust supplies
their arguments rather than authoring shell source. All declared fixture programs
are realised store closures.

The same `.#test` floor runs the Rust socket suite in
`nix/checks/reth-peer-probe.rs`. Controlled responses test adapter validation
and safe failures; pinned Reth tests the packaged HTTP, WebSocket, JWT, and
native peer integration.
JSON-RPC IDs compare by numeric value; booleans and mismatched IDs reject.
A native-command witness checks rejection before effects and planned endpoint
selection. Missing or malformed local secrets reject before networking; the
real node independently verifies the signer and rejects a wrong key.
`nix/checks/reth-peer-probe.nix` packages both the fixtures and the actual adapter
probe; the source gate checks formatting and compilation without binding ports.

Runtime library tests that launch workloads require the sibling runtime binary.
Before running `cargo test -p nixfied-runtime --lib` alone, run
`cargo build -p nixfied-runtime` in the same Cargo target directory. The full
workspace test command builds this binary through its integration targets.

For a targeted Rust iteration inside the pinned development environment:

```sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-manifest'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test admission'
nix build .#nixfied-cli --no-link
nix build .#nixfied-runtime --no-link
nix build .#install --no-link
```

`nix flake check` is the hermetic source/build core. Its `rust-workspace`
derivation first proves that [`OPTIONS.md`](OPTIONS.md) matches the typed Nix
modules, checks product-specific generated Rust freshness and structural policy
vectors, then runs rustfmt and Clippy with `-D warnings`; Clippy type-checks all
targets. The flake checks also build the debug runtime and minimal example manifest,
and run the Nix derivation golden vectors. The process- and port-using Cargo tests
run outside the Nix sandbox through `.#test`.

The installer wraps only the CLI; generated adopter apps use the release
runtime. The debug runtime and
`nixfied-test-child` are private transitive inputs of checks, gates, the dev
shell, and the fixture-backed test wrapper. `runtime-source.nix` creates a real
Cargo workspace root for each product, so CLI, runtime/manifest, and test-child
source changes do not invalidate unrelated product derivations. The Cargo lock
and vendor derivation are package-specific for the dependency-free CLI and the
libc-only test child. The runtime/manifest product intentionally retains the
canonical full-workspace lock because it owns the complete runtime dependency
closure. Rust toolchain and workspace-metadata changes can still invalidate
multiple products; package-specific lock changes are now isolated to the product
that owns them. The filtered roots carry the lock selected by their package map,
and `cargo metadata --locked --offline` validates that no transient lock
generation is needed.
The source-isolation matrix in `nix/gate-nix.nix` varies CLI, runtime, manifest,
generated-manifest and test-child sources independently and compares filtered-source
and product derivation identities. Reference and declaration-only variations keep
generated bytes fixed and must leave every Rust product identity unchanged.

Manifest wire declarations live in `nix/meta/manifest.nix`; runtime output wire
descriptions and error/status vocabularies live in `nix/meta/outputs.nix`.
Manifest boundary types and closed vocabularies are generated; runtime output
structs and borrowed views are written in their native Rust owner modules.
Declarations name only a generated type and its owner file; `nix/meta/rust.nix`
applies one fixed visibility and derive convention to every generated type.
The structural checker retains wire coverage, Nix construction and reference
checks without prescribing private output layout. Closed enum members come
from `capability.txt`. Native identifier,
loopback, graph, admission and execution rules remain in their existing owners.
Regenerate checked-in Rust after changing the shared structure or inventory:

```sh
nix run .#regenerate
```

This app is published only by the Nixfied flake, not by adopter `projectApps`.
Its generated-file input uses the locked Nix inputs and pinned rustfmt; run it
from the Nixfied repository root. Shell orchestration lives in Nix-packaged applications with declared tools,
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
repository reference, not a flake app/output or an additional manifest authority.
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
checks the complete metadata assembly and references for documentation and
release audits. Native binding projections independently check binding kinds
and duplicate names. `nix/modules/providers.nix` supplies module arguments
directly to evaluation; declaration-only presentation uses that same evaluator.
`nix/meta/manifest-structure.nix` validates the manifest bundle for compilation;
`nix/meta/default.nix` validates all bundles for generation and reference checks.
Final-export name audits run through `rust-workspace`. The Nix gate poisons a
documentation topic and output wire declarations separately, requiring valid
PostgreSQL compilation while documentation and release checks reject each fault.

This replaces the direct option-constructor imports in the four declaration
modules, the adapter export attrset, the compiler's hand-maintained reserved-app
list, framework-app attrsets and duplicate help-description ownership, and the
five manually named flake export attrsets. Native app scripts, config-derived
verbs, package builders and module/compiler behavior remain in their owners.

`packages.<system>.docs` and both docs apps use `nix/docs/reference.nix`.
`nix/docs/topics.nix` selects ordered `{ file, heading }` fragments and relevant
API entries and options. Fragments can compose explanations from different
owning documents without copying their prose. Topic output combines them with
bounded definition summaries and exact-query links. Keep explanations and
examples in their authored document; use the native definitions for types,
defaults, names and descriptions.

Each selected section must make sense without its surrounding document. Include
its necessary context and examples together, and use explicit topic commands or
section links instead of positional directions such as "see below" when the
referenced section is outside the selection. The renderer preserves selected
prose; it does not infer missing context or rewrite navigation sentences.
The builder includes selected sections' subsections, ignores headings in fenced
examples, and rejects missing or ambiguous headings and invalid selectors or
reference targets before packaging.

The private presentation index retains checked references from publications,
commands and structures alongside topic selections. Forward links and backlinks
derive from the same relationships; add a missing topic association once rather
than maintaining a second reverse list. These links describe related reference
entries. Execution prerequisites and behavioral guarantees remain with their
native owners and independent tests. Option and record guidance follows their
actual relationships instead of a fixed list of generic topics.

Presentation data comes from native option normalization, checked metadata,
authored prose and the supplying source identity. The
serialized presentation boundary discards string context; native values and
executable bindings retain it. Static docs do not import an adopter module or
realise manifest/runtime products. `docs/OPTIONS.md` retains its checked snapshot
and upgrade-report role; the full API reference is a disposable build artifact.
The upgrade documentation report still compares source README/docs files; it
does not compare built references or metadata source files. Replace removed manual
inventories with concise explanations and exact docs commands; metadata-only
reference changes need not appear in that source diff.

The existing `rust-workspace` check includes `nix/checks/option-metadata.nix`,
`publications.nix`, and the private `reference.nix` check. They cover native
mounts/overrides/defaults/apply, malformed metadata, hidden/manual bypasses,
whole-assembly validation, lazy bindings, scope identities, exact docs queries,
and presentation-context/closure isolation. Reference checks must independently
assert each topic's relevant inclusions and exclusions, forward/backlink
agreement, rejection of invalid selections, and propagation of definition changes
to both exact queries and topic summaries. A relationship does not prove native
behavior. The Nix gate runs
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
but do not change the lowered service contract.

## Gate composition

`nix/gate.nix` is a thin sequential coordinator:

- `gate-runtime` is a first-class Nixfied manifest. It exercises example runs, the
  emitted manifest/docs contract, concurrent slot isolation, runtime-layer refusal
  cases, endpoint coordination, and the state/service lifecycle matrix.
- `gate-nix` exercises the Nix compiler and install tooling: generated-app
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
archive is installed with its historical `#install`. Disposable adopter flakes
expose that historical compiler output as `manifest` for checked evaluation; the
archives and recorded documentation remain unchanged. The current checkout's
`#upgrade` then resolves the new archive and must reproduce `expected.diff`
byte-for-byte on both `--plan` and apply. The same fixed trees are reused for
Git and path identity checks, while tarball and unavailable-source cases retain
separate locked-identity coverage; an unsupported locked scheme is also
required to fail in plan and force before emitting a partial report or mutating
the project. Incompatible declarations and old package/API names must still
permit nonmutating inspection; checked apply rejects evaluation failures, while
explicit force repins without editing declarations or creating runtime state.
A distinguishable throwing-output fixture independently proves that plan and
force never evaluate outputs, including plan/force order and repetition. Both
apply policies cover no-op results, concurrent conflicts and interrupted rollback.
The packaged `nix/checks/upgrade-selection.nix` suite runs in the same Nix gate
against a local Git repository. It proves that a moving branch discovers new
commits without extra flags, an explicit input revision remains fixed with its
complete reference and actionable guidance, and a one-time switch to tracking
allows subsequent upgrades without an override. Plan preserves project files,
and throwing source/project outputs prove that selection never requires their
evaluation. These fixtures cover source selection; the historical archives
continue to own exact documentation-diff coverage.
Missing-lock and resolution failures still reject force. The
checked-in scope patches and `scope.expected.diff` separately prove README
changes plus documentation additions and deletions.

These archives, provenance, patches and stdout goldens remain byte-identical for
the inspection/force cutover; update current status and exit assertions only.
Refresh these fixtures only for an intentional source-diff behavior change. Export
each source revision into an empty directory, then create the archive with
sorted names, epoch timestamps, numeric zero ownership, and `gzip -n` (the
normalization is recorded in the manifest). Recompute the archive SHA256 and
NAR hash, regenerate the exact stdout golden from the current upgrade command,
regenerate `scope.expected.diff` if the deterministic scope overlays change,
and update `scope-old.patch`, `scope-new.patch`, `expectedDiffSha256`, and
`scopeExpectedDiffSha256` together with the fixture. Verify that the historical
installer still produces the list-form declaration used by the rejection case,
that the compatible case still passes manifest evaluation, and that plan/apply
stdout remains identical. The fixture freezes source versions and report
bytes; it does not make the nested Nix dependency closure offline.

The interrupt-and-recover lifecycle scenario remains a white-box Cargo test. It
requires registry access and precise process timing that a bounded leaf task
cannot provide.

## Verify uncommitted downstream changes

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
hosted workflow. `.github/workflows/checks.yml` runs the fixture-backed `.#test`
floor on Linux and macOS, including the macOS decoder tests. The Linux job then
runs flake checks and the gate, and builds the public CLI, installer, and optimized
release runtime as a final safety net:

```sh
nix build .#nixfied-cli .#install --no-link
nix build .#nixfied-runtime --no-link
```

Endpoint tests require positive managed-process socket FD inspection on both
platforms. macOS generates private Rust bindings from the Nix-selected SDK with
the pinned unwrapped bindgen tool. Generated layout assertions and the Rust
decoder check full record layouts and returned lengths; missing SDK/tool inputs
fail the build rather than selecting fallback definitions. It does not consume
a PCB inventory. Every Nix build that compiles the runtime, including isolated
generated-source Cargo fixtures, must supply both the selected `SDKROOT` and
`rust-bindgen-unwrapped`.
Denied or unsupported inspection must refuse, never silently pass. The runtime
unit observer checks exact IPv4/IPv6 and replacement; endpoint integration tests
check complete rounds, nonprimary failure, replacement during the final probe,
and TIME_WAIT restart. Service tests parse raw witness events and inject failures
at every coupled success write. Run these on macOS as well as Linux; a prototype
or a Linux pass does not establish macOS release coverage.

The local check/gate path uses the debug runtime to keep iteration fast. The
release package is what install and generated adopter apps ship. Both hosted
platforms use the same fixture-backed floor, including the realised Postgres
manifest needed by interrupt-and-recover coverage.

## Change-specific verification

Use the smallest proof that covers the change, then widen for shared contracts:

| Change | Minimum focused proof |
| --- | --- |
| Rust formatting/lint only | rustfmt + Clippy commands above |
| `nixfied-manifest` shape/validation | manifest crate tests + `.#check` |
| Runtime admission or lifecycle | focused runtime test + `.#test` |
| Live presentation and output sealing | `cargo test -p nixfied-runtime --test output` + `.#gate -- --dirty` |
| Nix resolution/validation/derivation | `nix flake check` + affected Nix vectors |
| Package/build change | CLI/runtime/install builds |
| Upgrade inspection/apply policy | packaged upgrade build + packaged `upgrade-syntax.nix` parser check, then `.#ci -- --dirty` |
| Static docs, selectors or reference relationships | focused metadata/reference checks + `.#gate -- --dirty` for downstream/source isolation |
| Generated manifest docs or public output | affected manifest build + `.#gate` |
| Adapter or example | build the affected manifest + `.#gate` |
| Contract or cross-layer change | `.#ci`; use `--dirty` when the generated project must consume the working tree |

Report any platform, release, or integration coverage that was not run.

## Command syntax ownership

`nix/meta/commands.nix` declares the shared command syntaxes; list their current
identifiers with `nix run .#docs -- api command`.
`syntax.nix` checks domains, defaults, visibility, references and generated names
without invoking native help functions. `command-help.nix` supplies ordinary row
and compact-usage formatting; `syntax-project.nix` emits Rust constants/type
aliases and quoted shell constants. The existing runtime, installer and upgrade
parsers retain input collection, entry routing, acquisition, repetition, errors
and all effects. Generated app wrappers consume the same tokens.

`generated-files.nix` routes structure and syntax fragments to existing native
include scopes. `nix run .#regenerate` formats them with the
pinned toolchain. Product builds compile checked-in files; the source
check compares freshly rendered files for both runtime and CLI. Its synthetic
structural fixture supplies its own file map to the same formatting step.

`nix/checks/syntax.nix` checks malformed declarations, help laziness, seven exact
help fixtures and an added optional argument without a formatter edit. Runtime
and CLI command tests exercise native parsing and encoding. The source check
also runs `nix/checks/upgrade-syntax.nix` against the packaged shell parser;
existing adoption and upgrade goldens cover its continuation. The gate's
source-isolation matrix in `nix/gate-nix.nix` varies CLI-generated sources as well
as manifest, runtime-generated, native crate and reference/declaration inputs. It requires
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
| Native internal workload-gate protocol | `tests/launch.rs`: independent raw frames, rejection before effects, signal/descriptor hygiene, Unix bytes, and bounded waiting |
| Native output/error views | `output_structure.rs`, main/output/registry/process tests and real redaction/write boundaries |
| Shared command facts and native parsers | `syntax.nix`, seven exact helps, native parser/encoding tests and packaged upgrade parser |
| Static revision-bound reference | `reference.nix`, downstream source switching, context/closure isolation and exact queries |
| Product source isolation and generated freshness | `gate-nix.nix`, `generated.nix`, packaged `nixfied-regenerate` |

The retired fixture-token scan and handwritten error/exit inventories are replaced
by the following checks. Token occurrence in one fixture cannot prove per-record
coverage, and an exhaustive generated enum is not independent wire evidence.

| Retired assertion | Current owner and independent proof |
| --- | --- |
| Fixture field tokens occur somewhere in the descriptor | `nix/meta/structure.nix` requires each inventory record's exact field set, unique wire/Rust names, and valid references; `nix/checks/structure.nix` supplies malformed declaration and literal producer vectors. `coverage.nix` requires exact whole-inventory coverage without duplicate owners. |
| Historical vocabulary is absent | Exact current inventory/declaration coverage and generated freshness reject drift. Raw `manifest_contract.rs` cases independently reject unknown top-level, invocation, secret, probe, and clean fields, including removed `docs` and `cacheEnv` bytes. |
| Every handwritten error/exit enum appears in descriptor tokens | Vocabulary members come from `capability.txt`; structure validation checks variant collisions and exact error annotation coverage. `generated.nix` checks emitted Rust against declarations. `output_structure.rs`, error/main unit tests, and command integration tests retain literal error/exit JSON, safe cause details, and process exit results. |
| Fixed record/vocabulary counts | Exact inventory set equality and per-record field equality, plus explicit routing for native surfaces. Counts cannot establish those relationships. |
| Assigned raw length and default Error::source readback | The compiled record/error types and adjacent literal JSON tests; no forwarding-accessor test is needed. |

Raw decoder coverage is a policy matrix, not an assertion that every record has
an independent unknown-field test. `wire_presence.rs` covers required positive
full-u64 values (missing/null/type/range rejection), optional omission and null
absence, empty-map omission with null rejection, enum defaults and unknown
variants, and unique-list rejection. Its Lifecycle unknown-field case represents
`RejectUnknown`; the additional raw manifest cases above exercise nested owners.
`output_structure.rs` covers `IgnoreUnknown`, optional explicit null, numeric
range, native path serialization failure, and open details. Error unit tests cover
omitted empty causes and safe nonrecursive cause projection. Pair these literal
boundary proofs with manifest decoder freshness and compiled exhaustive native
lowering. The independent literal `runtime_abi_snapshot` still requires deliberate
acknowledgment of every authored descriptor change. Keep the required-field
maintenance exercise below when changing declaration or generation machinery.

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
unsupported. Manifest decoders reject unknown fields; output records have no
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
