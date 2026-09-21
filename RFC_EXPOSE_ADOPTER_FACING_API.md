# RFC: Shared contract declarations and complete adopter reference

Status: architecture settled for engineer handoff. This supersedes both the original
migration handoff and the subsequent integer-only assignment. It specifies the
whole intended architecture; delivery is staged through complete owner cutovers.
It does not declare implementation complete or authorize restoring, committing,
or changing the product contract merely by editing this document.

The implementation must be written afresh against the baseline described in
section 2. The abandoned implementation is available only to architects and
designers for critical analysis. It is not an implementation reference, a source
of components to salvage, or a set of tests the engineer must satisfy.

The product problem is [FIXES_DOCS_REFS.md](FIXES_DOCS_REFS.md).
[CONTRACT.md](docs/CONTRACT.md) owns behavior and the model/runtime boundary;
[DERIVATION_SPEC.md](docs/DERIVATION_SPEC.md) owns independently derived facts;
[ARCHITECTURE.md](docs/ARCHITECTURE.md) explains the existing boundaries;
[DEVELOPMENT.md](docs/DEVELOPMENT.md) owns verification.
This RFC owns the proposed architecture, replacement plan, and acceptance gate.
Claims in abandoned working-tree documents are not accepted baseline changes.
Read [RFC_ENGINEER_HANDOFF.md](RFC_ENGINEER_HANDOFF.md) for the completed baseline
coverage audit, recovery receipt, and first implementation unit. That document
is a delivery audit, not another declaration source or architectural authority.

## 1. Outcome, invariant, and lessons

Starting from generated project help, an adopter must be able to discover the
supported authoring and operating surface, including constraints and limitations,
using reference content from the framework input selected by that project.
Ordinary authoring questions must not require reading framework internals.

The architecture also removes duplicated structural knowledge across native
declarations, wire construction, static Rust bindings, help, and reference
material. Nixfied remains the only intended consumer of this private machinery.
Adopters continue using ordinary Nix modules and existing library interfaces.

The invariant is:

> Every supported public surface has an explicit owner, revision-bound reference,
> and appropriate preservation evidence. Executable projections that share
> structural facts consume one authored definition of those facts. Native
> behavior retains its enforcing owner and independent evidence.

Complete coverage does not imply generating every implementation. Documentation
of byte replay, for example, requires its guarantees and limitations to be
discoverable; it does not require a generated replay wrapper.

The previous designs failed in different ways:

| Failure | Architectural correction |
| --- | --- |
| Promise complete coverage while postponing the constructor designs controlling total complexity. | Decide every surface's mechanism and native boundary before production migration. |
| Create a validator and generator for each presentation category. | Share structural machinery where semantics coincide; describe native behavior without wrapping it solely for participation. |
| Move local assertions behind rule identities and dispatch. | Keep checks beside their calculations and at their owning phase. |
| Retain prototype and production alternatives. | Test the final mechanisms; no prototype language in production generation. |
| Treat passing focused checks as whole-project completion. | Maintain one stable acceptance checklist and audit it at integration milestones. |
| Replace the goal with an integer fixture and arbitrary size limits. | Preserve the complete outcome; assess components, removed owners, and change sites. |

The framework can check structural integrity and required documentation. It
cannot prove English true, establish actual runtime reads from metadata edges,
or replace independent admission evidence with producer/consumer agreement.

## 2. Clean baseline and architect-only quarantine

The verified starting revision at this review is
c3d9111c861d40b0885a49f3d62950f9871702e1. It is the existing Nixfied framework,
not the failed unstaged implementation. "From scratch" means a fresh
implementation of this proposal, not a rewrite of baseline Nixfied.

Before handing implementation to an engineer, the architect responsible for
recovery must:

1. Preserve the complete abandoned work outside the implementation checkout:
   worktree contents, index state, binary-capable diff, added/deleted and
   untracked files, original RFC revisions, baseline identity, and relevant
   check results. Verify recovery is possible before discarding any work.
2. Identify unrelated user work and preserve it separately. Account for running
   checks explicitly; creating a clean checkout does not stop existing processes.
3. Quarantine the archive outside the engineer's workspace and handoff context.
   Do not create a repository archive directory, branch of abandoned code,
   linked worktree, fixture tree, or build input for the engineer to consult.
   Access separation is an operational responsibility, not a claim that a
   Markdown instruction provides filesystem isolation.
4. Prepare a clean checkout from the verified baseline, with the reviewed RFC
   and consistent problem statement. Exclude abandoned implementation, generated
   files, altered ABI inventory, implementation-progress claims, and old handoffs.
5. Verify the clean tree and baseline checks. Record any baseline defect as a
   separate issue with a behavioral reproduction and an owner.

The engineer receives the baseline, this replacement design, the normative
behavioral documents, and independent acceptance requirements. The engineer must
not inspect, copy, port, compare against, or selectively extract from the archive.
Architects may use it to understand failure modes; their findings must be
restated as reasoned architectural decisions or independent behavioral cases.
Do not hand over archived fixtures or generated output as an oracle.

A legitimate defect discovered during the failed attempt is not a reason to
retain its implementation. Specify and implement that correction separately
against the baseline, including any necessary contract/ABI changes. Record an
explicit baseline amendment if it must precede this work.

Baseline recovery and its verification are recorded in RFC_ENGINEER_HANDOFF.md.
Those preparation results establish the engineer's starting tree; they do not
establish implementation acceptance or authorize consulting the archive.

## 3. Scope and ownership

The scope covers all supported framework-owned public declarations, including
open option families. Public Rust visibility alone does not make an internal
item adopter API. Existing surfaces cannot be relabeled private to pass coverage.

| Surface | Authored fact and owner | Projections | Native responsibility and rejection boundary |
| --- | --- | --- | --- |
| Authoring options | Native option declarations, shared domain facts, descriptions, units, examples | Native module options and reference | Nixpkgs merges and checks configured values; compiler rejects relational invalidity before model publication |
| Model wire records | Structural declarations in Nix, tied to authored capability coordinates | Nix constructors, Rust data bindings, wire reference | Native lowering supplies values; Rust deserialization and validation independently reject untrusted models |
| Command and diagnostic JSON | The same structural declaration vocabulary | Rust serialization records/views and reference | Runtime collects observations, chooses failures, redacts, and writes |
| Commands | Syntax declarations: tokens, value domains, literal defaults, help | Rust/shell constants and value bindings, help, app tokens, reference | Native parsing, input encoding, acquisition/repetition/precedence, errors, contextual defaults, handlers, and effects |
| Public library and modules | Publication descriptors binding actual values and documentation | Public exports and reference | Native function/module evaluation and argument resolution |
| Apps and packages | Publication descriptors binding existing native products | Export sets, app descriptions, reserved project-app names, reference | Nix packaging; help keeps its current-flake source check |
| Artifact layout | Existing model package contract and native packaging | Documented model.json and disposable views/docs.md paths | Fixed native layout; no configurable layout language |
| Environment and host context | Normative context documentation and native resolver owners | Reference topics, with shared constants only where actually consumed | Native OS reads, empty/unset distinctions, precedence, platform decisions, and secret handling |
| Error vocabulary | Authored code/exit-class vocabulary, payload structure, per-code explanations | Typed vocabulary bindings and reference | Native exit-class and phase selection, causality, precedence, recovery decisions, and redaction |
| Human text and byte streams | Normative output contract and native renderers | Reference and independent literal/behavioral expectations | Native formatting, ownership, capture, replay, buffering, sink failures, and cleanup |
| Derivations and guarantees | Existing normative contract/specification and enforcing native owners | Linked reference topics and independent proof obligations | Direct phase-local Nix/Rust algorithms and checks |

The runtime retains exactly tasks, services, inline invocations, and static
composite DAGs. The proposal creates no runtime schema loader, documentation
dependency, required model sidecar, orchestration kind, compatibility reader,
daemon, or mutable authority. Host paths and secret values stay out of the model.

### 3.1 Authored ABI inventory

capability.txt remains the authored vocabulary and deliberate ABI-change record.
Inventory-linked declarations reference its coordinates; generated enum members
come from that owner rather than a copied list. Agreement is checked before
publication. The inventory does not become generated output.

The descriptor and richer structural declarations own different facts:
inventory membership versus representation, field policy, and explanation.
Agreement checks do not make them interchangeable authorities.

The baseline inventory is incomplete: CheckOutput, DownReport, and CleanupOutcome
are public JSON records without output-schema entries; OutputStream and
ProjectionOperation have no enum entries. Complete reference coverage must not
hide these surfaces or silently extend the descriptor to accommodate generation.
Use declaration-local identities for the uninventoried records and existing
native scalar bindings for the uninventoried enums, as specified in section 4.2.
Keep these known inventory gaps visible in the coverage audit. Any inventory
completion is a separate explicit ABI change, not a prerequisite or an implicit
side effect of this structural refactor.

An inventory member's trailing question mark is vocabulary notation, not an
executable omission policy. Remove it when matching the field's name; derive
neither decoding defaults nor null/omission behavior from it. For example,
ps-process.serviceInstanceId? and runtime-error.modelPath? serialize explicit
null in the baseline, while other marked fields omit absence. Structural
declarations and independent serialization evidence own that distinction.

Preserve baseline bytes and accepted/rejected behavior unless an explicit
contract change is identified. Internal relocation or generation alone does not
justify a numeric version bump. A real model/runtime contract change requires
the descriptor, ABI snapshot, producer, consumer, fixtures, and normative docs
to change together under AGENTS.md. Do not carry the abandoned ABI edits forward.

### 3.2 Coverage includes native behavior

Coverage is established from actual published options/exports, the capability
inventory, command declarations, and the normative behavioral documents.
A derived reference index may collect them, but there is no second authored
list of all public identities.

Coverage checks inspect final framework exports and the actual evaluated
framework option tree, not merely a descriptor's own rendered list. Negative
fixtures must append an undocumented raw export outside descriptor assembly and
introduce an undocumented option through an ordinary native module; each must
fail the relevant check. Adopter-added custom apps remain outside framework
coverage. Documentation checks do not claim to detect arbitrary concealed Nix
behavior; they verify the actual supported publication boundaries.

These final-export coverage checks are independent build/check assertions, not
inputs to the publication facade they observe. Static validation rejects invalid
supplied declarations before projecting them; the independent audit rejects a
manual bypass at the repository verification boundary. Neither mechanism claims
to seal arbitrary Nix code against later modification.

Behavioral explanations stay in their normative documents and authored guide.
Reference packaging includes those documents and an index of existing sections.
A small topic-to-document/anchor navigation map is presentation data, not a new
guarantee registry. It must not duplicate prose or claim to verify a native
implementation by checking that a filename or test name exists.

## 4. Shared mechanisms and supported forms

The design uses four cohesive mechanisms: native authoring declarations, shared
serialized structure, command syntax declarations, and publication descriptors.
Reference assembly consumes their checked metadata plus normative prose.
Presentation categories do not become additional declaration languages.

### 4.1 Native authoring declarations

Retain native Nix module functions and Nixpkgs option types. A private option
constructor checks required metadata and delegates to mkOption; it does not
implement a parallel module evaluator or recursive value interpreter.

Its accepted attributes are type, description, default, defaultText, example,
and apply, with their existing Nixpkgs meanings. Only type and description are
required; description must contain non-whitespace text. Unknown attributes
reject. Units and related guidance belong in the native description and linked
topics. There are no custom option annotations needing a parallel metadata tree.
Native evaluated option records are the sole source for mounted reference entries.
Do not encode native function bodies or Nix types into the reference JSON.

Use native forms for strings, booleans, bounded integers, enums, paths, packages,
lists/nonempty lists, attribute maps, nullability, scalar alternatives, and
nested submodules. Shared integer bounds and enum inventories may supply native
types and other projections, but a native package is never a wire scalar.
An authoring fragment is an ordinary Nix value or module function; reuse needs
no fragment registry, mounted-name language, or custom argument resolver.

Option paths remain segment lists internally. The module evaluator supplies
nested option metadata, including keyed submodule paths. Render names using the
existing native option documentation facilities, including literal <name>
segments; do not parse generated Markdown to recover paths.

Omitting default preserves required-value laziness. A supplied null is a value
accepted only by a nullable type. Native default/defaultText, apply, specialArgs,
imports, mkDefault, and mkForce continue to work normally. A contextual default
uses the ordinary module function's arguments and its native defaultText;
there is no expression interpreter or inferred callback signature.

Static checks validate the option declaration's metadata and type interface,
not every possible result of a lazy Nix expression. Native evaluation owns
literal/structured defaults and configured-value validation when demanded,
including post-merge checks. It must not demand an undefined required option
merely to publish its reference. The proof obligation for configured defaults
belongs to native evaluation tests, not a false promise of static totality.

Closed structural constraints such as an integer range use a shared definition
when multiple projections consume it. Graph checks, cross-field coherence,
placeholder scope, and executable selection remain direct native checks.
Do not add a rule registry, rule-id dispatch, or a generic phase-input envelope.

### 4.2 One serialized-structure vocabulary

Model records, public JSON results, and structured error payloads share one
normalizer, reference walker, name checker, and structural rendering backend.
"Model", "result", and "error" select boundary policies; they are not separate
recursive shape grammars.

The supported value forms are:

~~~text
Value =
    Text | Boolean
  | Integer { signed; bits; nonzero }
  | Enum { inventoryCoordinate }
  | List { element: Value; unique }
  | Map { value: Value }                 # JSON object, string keys
  | RecordRef { identity }
  | NativeDomain { id; wire: Value; explanation; producerCheck: predicate | None }
  | OpenJson { explanation }            # explicitly open diagnostic data only
~~~

Integer widths are those required by the baseline: signed 32/64 and unsigned
16/32/64. Reject impossible combinations. Nix constructor inputs must also be
representable as native Nix integers; this does not narrow Rust's admitted u64
domain. Do not convert large integers through floating point.

A native domain binds an existing Rust scalar representation through an explicit
backend binding. This covers typed identifiers, validated LoopbackHost, and existing
serde representations such as PathBuf, usize, OutputStream, and ProjectionOperation.
Its wire form and explanation describe the actual serialized value; it is not
a Rust source-code string supplied by a caller. The native type retains its
construction, validation, and serialization behavior. A scalar binding must not
hide an entire record whose fields require structural coverage.

producerCheck is a direct Nix predicate where that domain is Nix-produced;
output-only domains have no Nix producer check. It is not invoked while rendering
static reference. Preserve UniqueVec for unique model lists and ordered model
maps; generating plain Vec/String substitutes must not weaken deserialization.
Native domains cannot recursively disguise record graphs or implementation code.

Each record declares its identity, Rust binding name/owner, explanation,
ordered fields, and record decoder policy (RejectUnknown, IgnoreUnknown, or
NoDecoder). Each field declares its name, value form, explanation, field decode
policy, rustEncode, and nixEncode. Numeric units are explained in field metadata;
implementation storage is not part of that metadata. All policy fields are
explicit; there is no backend-specific fallback that silently changes meaning.
Field names/record identities are unique. References resolve to the appropriate
kind; structural record recursion is rejected for this delivery. The existing
non-recursive error-cause shape does not need general recursive-schema support.

A record identity is Inventory(coordinate) when the baseline descriptor contains
that record, or Local(name) when it does not. These are disjoint alternatives,
not two independent names for one record. Existing model records must retain
their inventory identities; Local is not an escape from available coverage.
CheckOutput, DownReport, and CleanupOutcome use Local identities. References and
the derived index use this same identity; no second record registry is authored.
In record queries render Inventory as family/owner and Local as local/name.
Inventory-linked records must cover their inventoried members exactly, after
normalizing member-name notation. Local records require the same metadata,
structural checks, and independent native-output evidence.

A field separates **accepted decoder input** from **producer emission**:

| Decode form | Meaning |
| --- | --- |
| Required(value) | Field required; null rejected |
| Nullable(value) | Field required; null accepted |
| Optional(value) | Missing or null means absence |
| Default(value, literal) | Missing means the checked literal; null rejected |

For NoDecoder records, these field forms describe the value's presence and
nullability for serialization only. They do not promise an accepted input
language or cause a deserializer to be generated. Decoder-default compatibility
checks apply only when a record actually has a decoder.

| Rust encode form | Meaning |
| --- | --- |
| Present | Emit the field, including null only where allowed |
| OmitAbsent | Omit absence for Optional fields |
| OmitEmpty | Omit an empty list/map |

Nix producer policy is separately explicit:

| Nix encode form | Constructor input and output |
| --- | --- |
| NotProduced | No Nix constructor for this output-only record |
| RequiredPresent | Require the named value and emit it; a decoder default does not excuse a missing producer input |
| RequiredOmitAbsent | Require an Optional-typed input; explicit null is omitted |
| RequiredOmitEmpty | Require a collection; omit it when empty |
| PreserveSupplied | Allow a missing key only when the field's decoder accepts omission; emit supplied values, including empty collections |

NotProduced must apply consistently to all fields of a record. Other policies
apply only to Nix-produced records. Every applicable encoding is checked against
the decoder: omission must be admitted, null must be admitted if emitted, and
OmitEmpty requires a collection with the matching empty default when decoding
exists. Serialization-only outputs do not acquire fictional decoder defaults.
OmitAbsent requires Optional; Nullable plus Present never means optional.
Default literals must satisfy the value form.

Required nullable decoding must reject missing while accepting explicit null;
a plain serde Option field alone does not prove this. Preserve the separate
baseline case Optional plus Present: missing and null both decode as absence,
and serialization emits explicit null.

The Nix constructor rejects unknown keys, missing Required inputs, and invalid
values. PreserveSupplied is a deliberate conditional-field boundary: native
lowering chooses which keys to supply, and the constructor validates that an
omission is allowed. For example, baseline leaf tasks supply empty ref lists,
while composites omit them; Rust may omit empties for both. servicesRequired
remains RequiredPresent even though Rust can default it when decoding.

Model decode rejects unknown fields. An output has NoDecoder unless a real
consumer needs deserialization; preserve that consumer's existing unknown-field
policy. There is no blanket ignore-unknown fallback. Normalize these policies
once, then both backends and reference rendering consume that representation.

Generated enums are closed sums sourced from inventory. Existing discriminator-plus-record
wire layouts, such as TaskSpec and ProbeSpec, remain unchanged. Native validation
rejects incoherent fields before execution lowering constructs the corresponding
closed Rust alternatives. This proposal does not redesign those wire layouts
or introduce a general conditional-field expression language.

ErrorCode uses this ordinary vocabulary mechanism with per-member reference
annotations containing description and recoveryTopic. Their keys must cover
the authored inventory exactly; topic references are checked. They supply the
error reference entries, not runtime dispatch or a separate payload grammar.
ErrorCode and ExitClass members come from the inventory; native RuntimeError
construction retains its baseline exit-class selection. Do not invent a per-code
exit mapping where the baseline uniformly selects the error class. Public error
payloads use the same record forms as other JSON, with native failure selection.
The process exit-status mapping is distinct from ExitClass and remains native.

The same vocabulary backend covers the five registry status inventories. Emit
inventory-driven invocations of the existing db_status! macro in its native
module scope, preserving enum/variant names and DbStatus::DOMAIN. Keep the
macro, database parsing/error conversion, classification sets, SQL, and
transitions native. This is vocabulary projection, not a state-machine language.
StopSignal consumes signal in unit 2; status and PortConflictReason bindings
land in unit 3; RunOutputMode consumes run-output-mode in unit 4. Native
RunOutputMode methods remain beside their callers. PortConflictReason replaces
the native diagnostic's reason strings with the same wire spelling.

Rust data generation preserves existing native type names and crate ownership.
Use generated owned records for wire data and generated serialization views
where native runtime objects already own the values. Lifetime/storage choices
belong to the Rust backend or a small native conversion, not the public schema.
A view may borrow native data; generating a second mutable runtime object graph
or deserializing generated JSON back into native objects is prohibited.

Generated views enter the existing native serialization/redaction/write pipeline.
For example, run/control output still converts to serde_json::Value, redacts it,
then formats and writes it. Do not serialize a view directly to a sink and bypass
that path. Preserve existing field/key ordering, pretty formatting, trailing
newlines, conversion failures, and native fallback/error selection at each public
boundary. Native path serialization stays native: replay diagnostics deliberately
use to_string_lossy, whereas other PathBuf fields retain their existing serde
behavior. Do not make all paths lossy or invent a formatting-policy language.

### 4.3 Command syntax facts and native parsers

Use one syntax descriptor vocabulary for the five hidden runtime commands,
installer, and upgrade. Generate the facts that have multiple consumers:
command/argument tokens, value-domain bindings, literal defaults, ordered help,
and reference entries. Retain the baseline native parsers and their typed option
structures. This is the final design choice, not an alternative for the engineer
to reconsider during migration.

A command descriptor has name, description, ordered arguments, help text/layout,
and references to its native behavior topic and output/error contracts.
An argument has token, valueDomain, initialValue, and help. valueDomain is Flag,
Text, Path, Unsigned(32|64), or Enum sourced from the shared vocabulary.
initialValue is Absent or Literal(value), checked against the domain.
help is Visible { text; metavar } or Hidden { explanation }; flag metavariables
must be absent. Reject unknown keys, duplicate tokens, invalid defaults, missing
explanations, and conflicting generated names.

Ordinary Nix composition shares common arguments. Native parser comparisons,
initializers, app wrappers, and help use the generated constants and value
bindings. Numeric type aliases and closed enum bindings provide actual native
types; keep native numeric parsing and its error handling. Paths remain lexical
values until the native owner resolves them. A command descriptor is not a Rust
source snippet or an untyped dictionary passed to handlers.

Native parsers own input collection, entry selection, operand acquisition,
repetition, help precedence, removed-alias rejection, and error routing.
These behaviors are specified in section 7 and documented from the owning
behavior topic; they are not independently authored executable policy fields
in the descriptor. A typed constant or enum does not prove that its native
consumer implements the documented sequence. Independent conformance tests do.

There is one small syntax projection with Rust and shell output formats.
The shell format supplies safely quoted constants/help to the existing parser;
it never emits eval or uses strings as executable fragments. No scanner
generator, parser AST, lexical-error callback registry, or command-family backend
is introduced. Existing shared native parsing helpers can be reused directly.

Do not add positionals, equals-form options, short-option clusters, terminator
semantics, aliases, or new accepted input forms to these baseline commands.
They are behavior changes, not incidental effects of adopting a parser library.
No new parser-library dependency is required.

Complete reference coverage still includes the native lexical behaviors and
effects. Sharing syntax facts does not claim to generate or statically prove
those behaviors. This boundary avoids a policy language whose main purpose
would be to re-express seven small native parsing loops.

### 4.4 Publication descriptors

A publication descriptor has an identity, nonempty description, lazy native
binding, and one of the following closed reference forms. The identity contains
kind, scope, and name; reuse of a name in another scope is not a collision.
Unknown keys and fields belonging to another alternative reject.

| Form | Scope and required reference fields |
| --- | --- |
| Function | Library; input description, result description, usage |
| Module | Adapter; usage and contributed declaration explanation |
| Argument | Module arguments; value-domain description, provider description, usage |
| App | Root or project; invocation/usage, effects explanation, command reference or explicit native-behavior topic |
| Package | Root, check, or devShell; usage and result/artifact explanation |

Each form may include a list of checked references to other published entries,
option paths, record identities, vocabulary coordinates, or topic anchors. A reference check establishes
identity/kind only, not native behavior. Framework public exports cannot opt out
by choosing an undocumented/private audience tag.

Function signatures describe existing ordinary Nix functions; they do not
introduce a second argument checker. Argument describes injected values such
as pkgs, system, adapters, and nixfiedLib without pretending all are modules.
Module binds an actual importable adapter; App binds an existing app value;
Package binds an existing derivation under packages, checks, or devShells.
The native consumer checks the actual
bound value when it is demanded. Static publication validation does not force
a package build, function body, or injected context value.

Actual lib, adapter, module-argument, package, and framework-app outputs derive
from these descriptors. Their names cannot be separately authored in both an
export attrset and a documentation list. Derive reserved project-app names from
the same app descriptors. Adopter-exported verbs remain config-derived ordinary
apps, not framework declarations.

Check and development-shell descriptors use the same Package form and disjoint
scope identities. Check derivation bindings remain lazy, including checks that
audit final exports; inspecting their declared metadata must never build or
evaluate the result of the check itself. Do not omit documented developer
outputs from coverage or promote private transitive products to public exports.

Fixed artifact layout, environment resolution, and behavior are documented
through native owners and topics. They acquire no Layout, ContextResolver,
TextOutput, ByteOutput, or Guarantee execution grammar.


## 5. Compiler structure, dependencies, and evaluation order

The compiler is private construction machinery. Nix values are not opaque types:
private constructors and checked publication prevent ordinary misuse, while
coverage checks audit actual exports. Do not claim Nix provides language-level
sealing or that arbitrary Nix code can be statically verified.

The declaration-only option evaluation imports raw framework declaration modules
and uses their private lazy native providers for pkgs, system, adapters, and
nixfiedLib. These are the same provider values bound by publication descriptors,
not a second argument registry. This evaluation imports neither the checked
publication facade nor an adopter module. After metadata validation, checked
projections expose the native values for project use.

Independent Nix checks then inspect final flake exports and evaluated framework
options. Those checks may depend on checked projections; projections must not
depend on those checks or on final exports to reconstruct their own providers.
This separates static declaration validation from the audit of manual bypasses
and prevents a self-dependent publication graph.

| Pass | Input | Output and required rejection |
| --- | --- | --- |
| Construct/normalize | Authored declarations and native option metadata | One normalized representation per mechanism; reject unknown keys, missing metadata, impossible policies, malformed literals |
| Collect/resolve | Normalized declarations, mounted option paths, capability inventory, topic map | Derived index and resolved references; reject duplicate identities, name collisions, missing/wrong-kind references, unsupported cycles |
| Check declarations | Resolved declarations and static metadata | Checked projections; reject invalid declared exposure/inventory bindings and inconsistent backend bindings |
| Project | Checked representation only | Native modules/exports, Nix wire constructors, Rust/shell bindings, structured reference |
| Package | Projected source/content and supplying source identity | Product-specific build inputs, reference package, docs app |
| Audit final exports | Actual final framework outputs/options and their checked declarations | Independent repository checks reject undocumented exports/options or missing final coverage; this audit is not a publication dependency |

Normalization happens once per assembly. Constructors may check immediate
arguments; backends do not reparse declarations into independent internal
languages. Local helpers may differ, but ownership and pass order are fixed.

Demanding a published projection must force static validation of all declarations
supplied to that assembly, including an unrelated malformed declaration. Use a
validation barrier over explicit metadata, not deep evaluation of the complete
object containing lazy functions and derivations. Lazy implementation bindings,
project configuration, native default expressions, package results, and host
inputs remain unforced until their owning native phase needs them. Native
declaration-module functions do run to obtain static metadata.

Framework reference generation may evaluate declaration modules with the pinned
Nixpkgs and target-system context to obtain native options. It must not evaluate
an adopter module, compile a project model, realize runtime closures, or read
secrets. Required configured values remain lazy. Poisoned-dependency fixtures
must prove these boundaries.

Project compilation remains:

~~~text
ordinary adopter module
  -> native module evaluation and merging
  -> native relational validation
  -> native derivation/lowering with direct phase-local checks
  -> checked construction of wire values
  -> existing closure realization and model package emission
~~~

Rust consumes generated static bindings and model.json. Its existing
deserialization, structural/relational validation, independent graph derivation,
host admission, and execution remain in their existing phases.

Dependencies are one-way:

- The common compiler depends on library facilities and declaration inputs, not
  native Nixfied algorithms or product-specific runtime channels.
- Native declarations may reference implementation values lazily. Native
  algorithms receive checked values/constructors directly; they do not import
  the assembled reference or resolve themselves by a string identifier.
- Backends consume checked structure. No backend parses another backend's
  generated source, help, or Markdown to learn meaning.
- Documentation may depend on metadata and authored prose, not runtime binaries.
- Runtime builds depend on their generated source, not documentation artifacts
  or generated source for unrelated products.
- Nix and Rust graph derivations remain independently implemented.

### 5.1 Source organization and generated artifacts

Keep native owners in their existing directories. Do not move all definitions
into one large definitions.nix merely to obtain a central review location.
Native module declarations can remain under nix/modules; native algorithms stay
under nix/compiler and runtime crates.

Use nix/meta for the private compiler, shared serialized declarations, and
syntax declarations where their Nix authorship serves several projections.
Organize by demonstrated responsibility, not one folder per reference category.
Publication descriptors can live beside the owning flake/module assembly.
The composition entry point imports cohesive bundles; it contains no repeated
catalog of every option, command, record, or exported name.

Generated Rust lives in clearly marked generated modules in the consuming crate.
Retain checked-in Rust projections so ordinary editor/Cargo workflows work.
Provide one explicit regeneration operation using pinned Nix and rustfmt.
Repository checks compare freshly generated product-specific projections with
the checked-in files and fail on staleness. Product builds compile those same
checked-in sources; they do not regenerate implicitly or compile an overlay
different from the developer's tree. A bare Cargo or package build is not
freshness evidence; passing the freshness checks is required for delivery.

Freshness checks enter with the first generated production consumer. Preserve
the existing Cargo partitions: CLI alone, runtime with model, and test-child
alone. A product check compares only its expected generated files; do not add
the entire framework checkout, docs output, or other products' projections to
that product's filtered source. Shared declaration/compiler inputs can affect
generation checks, but unchanged generated bytes must not invalidate unrelated
Rust product sources. Prove this with the existing source-variant checks.

Keep the existing checked docs/OPTIONS.md snapshot for its established repository
and upgrade-reference role. The full new API index/reference is a disposable
build artifact; do not add a second checked-in full API dump. Generated artifacts
are never edited by hand. No generator service, cache, daemon, new public
maintenance app, or prototype-generated runtime module is introduced.

## 6. Worked architecture cases

These are requirements for fixtures written afresh from baseline behavior and
this specification. They are not permission to copy archived implementation or
expected output. Constructor spelling is private; the forms and boundaries in
section 4 are binding.

### 6.1 Nested reuse and native module semantics

Define the existing invocation option fragment once. Reuse it at a leaf task's
invocation, service start, and nullable exec probes inside keyed service maps.
The reference enumerates the mounted paths, while the actual options retain
their native submodule types and merge behavior.

A shared positive integer domain and the invocation default of 30000 feed both
native acceptance and reference. A task override does not alter service defaults.
Zero and wrong-type values reject when demanded. Missing required tools/run
remain lazy during reference construction and fail in project compilation.
A malformed description at an unrelated static option blocks publication.

Changing the shared minimum in a synthetic fixture changes native acceptance
and every mounted type description without altering framework code. Independent
expected values assert the change; the test must not ask the declaration to
calculate its own expected result.

The same domain does not imply the same default or meaning: ready timeout is
per attempt, stop timeout bounds shutdown, and invocation timeout bounds its
invocation. Their option explanations and defaults remain distinct.

### 6.2 Contextual defaults and native transformations

The target.system option keeps its default equal to the native module argument
system, with a symbolic native defaultText. Evaluate it under two supported
systems and with an explicit override. Rendering must display the expression
rather than accidentally freeze one evaluator's configured value.

codebases.main.sourceIdentity retains its path-or-nonempty-string input type,
default "live", and native apply = toString transformation. Prove path/string
acceptance, rejected wrong types, retained store-path context where required,
and the resulting lowered string. Do not add a transformation AST or move the
conversion into a generic rule interpreter.

### 6.3 Authoring packages and invocation wire data

Authoring invocation.tools accepts closure ids or packages. Native Nix lowering
synthesizes package closures and resolves run[0] against the declared tool set.
The wire Invocation record contains closure ids and the resolved executable,
not packages or callbacks.

Native lowering directly rejects missing tools, unresolved executables, and
declared env.PATH. Generated construction checks the resulting record's
structural fields; Rust independently checks closure references and re-derives
executable selection. A malformed serialized record and a structurally valid
record carrying a false executable must be separate rejection tests.

Do not rename baseline InvocationSpec in Rust merely because the inventory
coordinate is Invocation. The Rust binding is an implementation name; the
coordinate is the wire identity.

### 6.4 Missing, null, empty, and defaulted fields

Use these baseline cases, with separately authored raw JSON and expected bytes:

| Field | Required preservation |
| --- | --- |
| Invocation.timeoutMs | Required positive u64; zero, null, omission, and overflow reject |
| Lifecycle.prepare | Missing/null means absent; absence serializes omitted |
| ServiceSpec.endpoints | Missing means empty, null rejects, Rust omits an empty map |
| TaskSpec.servicesRequired | Missing means empty, null rejects, Rust emits the field even when empty |
| TaskSpec.defaultOutput | Missing means summary; unknown enum member/null reject |
| TaskRun.exitCode | Missing/null both decode as absence; Rust serialization emits explicit null (Optional plus Present) |
| RunOutput.nodes | Empty list is omitted by serialization; this output has no decoder |
| ProcessObservation.serviceInstanceId/serviceLifetime | Absent values serialize as explicit null, despite question-mark inventory notation |
| RuntimeError.modelPath/computedModelHash | Absent values serialize as explicit null; do not infer omission from the inventory |

A synthetic Required-Nullable field must accept explicit null and reject a
missing key, independently of the baseline Optional-plus-Present example.
Also compare Nix-produced model bytes and Rust serialization separately. They
need not have identical omission choices merely because both decode to the same
value. Preserve the baseline producer's supplied-empty behavior explicitly.

Identifier newtypes retain namespace separation; baseline lexical ID checks
remain in native model validation, not in their permissive string constructors.
LoopbackHost continues to reject non-loopback endpoint hosts on construction.
Task/probe discriminator coherence remains a separate native rejection:
structural construction alone does not establish an admissible execution plan.

Also exercise CheckOutput.rawLen as native usize, native PathBuf fields, and
native output enum leaves without inventory entries. Compare actual public
output bytes through the existing redaction/write path, not just direct serde
serialization of a generated view. Include secret-bearing observed strings,
pretty-output ordering/newlines, and platform-appropriate non-UTF-8 path cases
that distinguish native serde failure from intentionally lossy replay diagnostics.

### 6.5 Native guarantee without generated behavior

For task-output replay, the reference describes redaction before replay,
independent ordered streams, bounded concurrent copying, replay before cleanup,
and cleanup continuation after projection failure.

The native replay ticket remains the owner of open evidence and consume-once
replay. No byte-output descriptor, generated ticket wrapper, runtime channel
allowlist in the compiler, or forwarding-binding proof is added.

Independent Rust tests exercise sink failure, concurrency, retained sources,
redaction, and cleanup ordering. Compile-time ownership evidence belongs on the
native type. Merely linking the contract paragraph to that type is not proof of
its behavior.

### 6.6 Publication, discovery, and an original adopter question

Mount an adapter through the existing adapters module argument and publish
compileModel, projectApps, and seq through their existing library interfaces.
The reference describes those actual exports without forcing adapter execution
or a release runtime build. Exporting an undocumented descriptor fails.

The exact stateRefs option entry must explain that its string labels are not an
enum of storage backends or selectable state roots. They remain serialized model
data but do not select a different state directory or alter service reuse
identity. Native lowering discards them. Distinguish syntactically accepted
strings from meaningful runtime choices. Explain sibling descriptive ref fields
against their actual baseline behavior; removing them is separate contract work.

A downstream project pinned to source A must expose source A's reference even
when another checkout/source B is available. Invalid project configuration must
not contaminate the standalone supplying framework's docs package.

## 7. Command preservation and native continuations

The shared syntax projections and native integration must cover all seven
existing option-parser surfaces. The following baseline behavior belongs to
native parsers and independent conformance tests. It is preservation scope for
this refactor, not a recommendation to introduce these quirks into new interfaces.

| Property | Runtime commands | Installer | Upgrade |
| --- | --- | --- | --- |
| Input collection | Existing UTF-8 std::env::args collection | Existing args_os and branch-specific UTF-8 conversion | Shell byte strings with LC_ALL=C |
| Entry selection | No args selects check; unknown command rejects | Existing root usage/help routing | Packaged command receives options directly |
| Help order | Any help token after a recognized command wins before parsing/admission | Any help token after install wins before option validation | Sequential; earlier errors win |
| Option-like operands | Next token is consumed as value | Next token, including empty, is consumed | Empty or double-hyphen-prefixed values reject; -h may be a value |
| Repetition | Flag set, ordinary last-wins, task/output reject after value acquisition/validation | Last-wins | Last-wins values, idempotent flags |
| Effects after parsing | Model/host resolution and execution remain native | Metadata validation/inference and file creation remain native | Source inspection, Nix calls, preflight, plan/apply remain native |

Runtime arguments:

| Token | Commands | Initial value | Operand/repetition behavior |
| --- | --- | --- | --- |
| --model | check, run, ps, down, clean | Absent | Last-wins; trailing flag clears an earlier path; required-model rejection remains native |
| --allow-non-store-model | All five | false | Set true; hidden framework escape hatch |
| --state-base | run, ps, down, clean | Absent | Last-wins; trailing flag clears override; native fallback follows |
| --slot | All five | Absent | Parse each occurrence as u32; missing rejects; last-wins |
| --timeout-ms | run, down | 5000 | Parse each occurrence as u64; missing rejects; last-wins |
| --task | run | Absent | Acquire value before rejecting duplication |
| --output | run | Absent | Parse summary/json/both/task-output before rejecting duplication |
| --purge | clean | Standard cleanup | Set purge mode |

Installer declares --root (default "."), --project-id (absent), --name (absent),
and --nixfied-url (the baseline default pin). All are last-wins values; native
metadata inference and validation occur afterward. Upgrade declares --root
(default "."), --nixfied-url (empty default meaning preserve current selection),
--plan (false), and --no-lock (default lock enabled).

Independent conformance cases must include:

- run with a valid output followed by an invalid repeated output reports invalid
  output, not duplicate output; a repeated task with no operand reports missing
  value before duplicate selection.
- A trailing model/state-base flag preserves the baseline clearing behavior.
- Integer parsing covers leading plus, whitespace, negative, zero, and overflow.
- Removed output aliases retain their specific failure behavior; equals-form
  options, positionals, and an unsupported terminator remain rejected.
- Runtime early error projection remains a native scan using the declared output
  token/vocabulary. Its last operand-bearing output occurrence chooses projection;
  a trailing operand-less occurrence does not clear an earlier projection.
- Runtime non-UTF-8 input currently fails during input collection before help.
  Do not silently introduce lossless paths or prescribe unstable panic text.
- Installer missing/non-UTF-8 root command retains its existing successful usage
  response. Invalid standalone token encoding and invalid operand encoding retain
  their distinct errors. Root help routing remains native.
- Upgrade "--root --help" fails value acquisition; "--root -h" consumes -h.
  Preserve its existing trailing-newline stripping through command substitution
  unless a separate behavior change is explicitly designed.
- Runtime help avoids admission, source/state materialization, and child
  execution. Signal installation precedes help in the baseline; do not claim
  absolutely no process effects.

These distinctions remain native behavior, not reasons to invent
runtime-command, installer-command, and upgrade-command grammars or a policy
interpreter. Native error construction and parsing helpers remain direct calls.
Shared syntax constants eliminate duplicate tokens/defaults; they do not certify
acquisition or precedence. Conformance tests must exercise the real parsers.

The new docs query app, contextual help, and developer gate wrappers remain
native presentation/orchestration programs. Their interfaces are fully documented
through App descriptors and topics. Their positional query syntax and underlying
tool delegation do not expand syntax-fact projection into a universal parser.
Full reference coverage does not require universal parser generation.

## 8. Reference product and revision binding

The reference is a public integration feature, independent of project execution.
Reserve docs alongside the existing project control/discovery names, add it to
generated project apps and root framework apps, and export the documentation
package. Update scaffold guidance and final-app discovery tests together.

The public package is packages.<system>.docs; both root and project docs apps
invoke its bin/nixfied-docs. The package contains share/nixfied/reference/API.md
as a readable reference and the static lookup content needed by that command.
The command and readable reference are the supported interfaces. Lookup-index
encoding and other support files are private presentation details, not a
promised external JSON API. There is one content builder, not separate sources
for the package and the app. This Nix-only addition updates VERB-1/SURFACE-1 and
documents the docs behavior without changing model/runtime bytes or requiring
an ABI rotation.

The supported query interface is:

~~~text
docs                              index, supplying source, next-step examples
docs -h | --help                  query usage
docs options [prefix]             canonical option paths
docs option <exact-path>          native type/default, explanation, examples, links
docs topic <name>                 authored guide/contract material for that topic
docs api [kind]                   list reference kinds or entries of one kind
docs api <kind> <exact-id>         one public API entry
docs source                       supplying framework source identity
~~~

The API kinds are function, module, argument, app, package, command, record, and error.
Canonical lookup IDs are library/name for functions, adapter/name for modules,
module-argument/name for arguments, root/name or project/name for apps, and
root/name, check/name, or devShell/name for packages. Command IDs are the seven
command names; error IDs are exact inventory code spellings. Record IDs follow
section 4.2 (for example primitive/Model, output-schema/run-json, local/CheckOutput).
List entries in canonical ID order. Derive the index from actual descriptors;
there is no separately maintained list of allowed API names. Fixed package
layout, context resolution, and behavioral guarantees are navigable topics
linked from the appropriate entries, not new executable declaration families.

With no arguments, print a concise index rather than every document. Exact
lookups do not approximate names or silently fall back to unrelated material.
Prefix lookup matches a complete path or namespace boundary, not a substring.
Unknown/empty queries and unmatched prefixes fail nonzero with useful guidance.
Content goes to stdout, errors to stderr; no browser, pager, or network is
required after the app has been built.

Package existing authored documents as the topic content, with a small navigation
map identifying the relevant owning document/sections. A topic may print its
owning document; no custom Markdown parser or duplicate rewritten topic prose
is required. The index must cover authoring limits, tasks/services, state,
endpoints/placeholders, secrets, adapters, context resolution, outputs/errors,
and operation/recovery. Improve the owning source text when an explanation is
missing, rather than maintaining an alternative explanation in a topic catalog.

The original stateRefs question and task-versus-service placeholder rules are
acceptance cases. Explain primary endpoint selection, direct declared references,
endpoint-less service restrictions, secret substitution scope, and descriptive
refs against baseline behavior. Never infer a closed domain from an example.

Content comes from the same supplying framework source that creates the project
apps. Package source identity at build time, with revision when available.
A path/dirty source must report its actual supplying identity without pretending
it has a committed revision. Reading docs never invokes Nix or the runtime,
loads model.json, admits a project, or observes runtime environment/secrets.

Selecting a project flake app still requires surrounding flake evaluation and
dynamic exported names. Recovery uses docs from the explicit supplying framework
source when project evaluation fails; do not silently substitute an unpinned
latest revision. Contextual help retains its separate native current-flake
source check and reference-neutral presentation.

Static reference packaging must not retain or build the runtime/CLI products
merely to extract metadata. A disposable index.json in the reference package is
presentation data, not another required model artifact or semantic seam.

## 9. Replacement plan

This plan describes reviewable change units, not authorization to create Git
commits. AGENTS.md's explicit-commit rule remains in force. Preparation is an
architect/recovery responsibility; engineering starts only in the clean tree.

RFC_ENGINEER_HANDOFF.md contains the architect's finite coverage matrix from the
clean baseline's option tree, exports, command parsers, inventory, and normative
sections. It records the chosen mechanism, native owner, replacement site, and
proof obligation, including public outputs absent from the inventory. The
engineer records actual replacement and evidence against this matrix. It is an
audit artifact, not a source registry or a build/runtime input. Any newly found
surface outside its cases returns to architectural review before a new
mechanism is implemented; an ordinary additional declaration uses the settled
mechanism without another design stage.

| Unit | Implement and migrate | Remove or replace in the same unit | Preservation proof |
| --- | --- | --- | --- |
| 0. Architect preparation | Quarantined archive, verified clean baseline, reviewed RFC, consistent problem statement, independently specified baseline cases | Abandoned material from engineer workspace/context; competing handoffs and progress claims | Clean-tree/import inventory and baseline checks; archive recovery verified separately |
| 1. Authoring and publication | Checked native option metadata in nix/modules; actual lib, adapter, module-argument, app/package descriptors; reference lookup and revision-bound docs app | Duplicate export-name/description lists in flake.nix and nix/project-apps.nix; reserved-name literals in compiler validation; replace option-reference wiring without replacing native merging | Nested reuse, default/apply/override/laziness cases; publication coverage; original adopter question; pinned downstream discovery and standalone docs isolation |
| 2. Shared model structure | Single structure checker, Nix constructors, Rust wire projection; migrate baseline model records/enums in nixfied-model and producer construction in derive.nix | Handwritten migrated record/enum definitions in types.rs and matching Nix shape/omission duplication; retain native newtypes and algorithms | Independently authored raw JSON; baseline producer bytes; Rust decode/serialize; invalid domains/coherence; Nix/Rust derived-fact vectors and admission |
| 3. Result/error structure | Reuse the same structure mechanism for public JSON, error/exit vocabulary, and registry status bindings; migrate serializers/views at their native output boundaries | Superseded field lists, serde records, ad hoc JSON reconstruction, and duplicated vocabulary spelling at migrated sites; retain native process exit mapping and status transitions | Existing public field/omission behavior, observed values, redaction, phase and failure precedence; no new structural backend |
| 4. Command syntax | Shared syntax-fact projection; route all seven native parsers, their help, and generated app tokens through it | Duplicated tokens, enum/default definitions, and help literals in runtime main, CLI InstallOptions::parse, upgrade's initial syntax definitions, and project-app wrappers; retain native parsing loops | Independent section 7 vectors; actual parser conformance; help before admission; installer/upgrade ownership and existing report fixtures; no effect reordering |
| 5. Complete coverage and audit | Finish native behavioral/context explanations in their owning docs; integrate reference links, source boundaries, developer instructions, and all final projections | Temporary fixtures/scaffolding that duplicate production paths; stale owner claims and obsolete generated readers | Whole acceptance gate in section 11, maintenance traces, complete replacement ledger, final affected-product and cross-layer checks |

Units 2 and 3 reuse one structural representation; unit 3 does not add an output
grammar. Unit 4 uses the syntax-fact boundary already specified here; it does not
generate scanners or develop three command frameworks in sequence. Unit 5 is
coverage/integration work, not a place to defer architectural decisions or repair
unbuildable slices.

Each unit must be buildable and preserve its native boundaries before the next.
Where a unit affects producer and consumer, cut them over together. There is no
compatibility migration, dual reader, staged reinterpretation of existing bytes,
or source overlay masking an incomplete transition.

Each unit closes references and covers its participating surfaces completely.
Unmigrated surfaces retain their baseline implementation and remain explicitly
incomplete in the delivery audit; they do not need placeholder declarations.
An early App descriptor may reference its existing native-behavior topic, then
gain a checked command reference when that command declaration lands. Never
accept a dangling future reference, success-returning stub, or permanent
ignore-missing mode to keep an intermediate unit buildable. These units are
review snapshots, not claims of final reference completeness. The final gate
requires all units and complete coverage, with no migration registry in code.

A synthetic fixture may exercise the final generic mechanism, including cases
production declarations cannot safely mutate. It lives under checks, does not
define another language, and is never imported by production assembly. Temporary
scaffolding has a named consumer and a removal point within its introducing
unit; "remove after all migrations" is not sufficient.

### 9.1 Concrete retained native owners

The ledger must explicitly preserve these responsibilities:

- nix/compiler/resolve.nix: native module evaluation and specialArgs.
- nix/compiler/validate.nix and derive.nix: direct relational checks, graph
  algorithms, executable resolution, placement calculations, and lowering.
- nix/compiler/emit-model.nix and views.nix: fixed model package emission and
  disposable configured-model documentation.
- nixfied-model identifier/collection/domain implementations and validation:
  native constructors, serde checks, and structural/coherence validation.
- Runtime admission/execution/state/service/registry/output owners: every OS,
  lifecycle, containment, redaction, replay, and cleanup effect.
- CLI native installation and upgrade's body beginning with project inspection:
  quoting, file ownership, preflight, concurrency, mutation, and recovery.

Generated record/view integration must preserve those boundaries. It must not
pull native behavior into nixfied-model simply because its data is shared.

### 9.2 Scope and design change control

Ordinary implementation choices within these mechanisms remain engineering
decisions. A new declaration family, backend, execution dispatcher, semantic
seam, mutable authority, or changed rejection phase is an architectural change,
not a routine migration detail. Record the concrete unmet requirement and
alternatives for architectural review before dependent implementation proceeds.

An implementation finding cannot authorize itself by expanding this RFC's
acceptance checklist. Preserve the agreed checklist; track proposed changes
separately until reviewed. Adjacent defects get independent issues and fixes,
not automatic inclusion in the current unit.

## 10. Maintenance evidence and cost model

There are no line-count or file-count acceptance caps. Counts are evidence of
maintenance burden, not a definition of correctness or an allowance to consume.
Do not compress code to meet an estimate.

The following are preliminary planning ranges for the new maintained machinery,
derived from the selected components. They are estimates, not measured results
or implementation authorization. They exclude product declarations, existing
native code, generated outputs, and tests, which are accounted for separately.

| Component | Reason for existence | Initial handwritten LOC estimate |
| --- | --- | --- |
| Native option metadata/domain helpers | Metadata rejection and shared constraints while retaining Nixpkgs semantics | 120–250 |
| Shared structure normalization, linking, and Nix construction | One shape traversal and field-policy owner for model/results/errors | 350–650 |
| Rust structural/vocabulary projection | Static records/enums and serialization views using native domains | 250–500 |
| Command normalization and shared checking | Shared syntax facts for all seven native parsers | 100–180 |
| Rust/shell syntax projection | Tokens, numeric/enum bindings, literal defaults, and help; no scanners | 120–240 |
| Publication and reference assembly | Actual export bindings, identity checks, native option/reference joins | 200–400 |
| Docs lookup and provenance packaging | Static query experience, authored prose, source binding | 180–320 |
| Regeneration/freshness/build wiring | One explicit generation path with product-specific checks | 100–200 |

This suggests roughly 1,400–2,700 handwritten machinery lines, before native
integration and declarations. It is not a promised total diff. The estimate
must be revised from actual component evidence at each completed unit.

Account separately for:

- Structural/command/publication declarations, including explanations: budget
  provisionally 1,500–3,000 authored lines, then replace the estimate using the
  baseline inventory and concrete examples. Existing option declarations remain
  native and their edits/moves must still be counted.
- New independent fixtures and tests: provisionally 1,200–2,400 lines, organized
  around boundary properties rather than one harness per presentation family.
- Native integration, changed prose, generated Rust/reference output, new
  dependencies, affected products, file count, and removed definitions.
- Gross additions and deletions against the agreed baseline, as well as final
  maintained footprint. Moving code, splitting units, or excluding fixture
  scripts must not hide the cost.

Unexpected growth requires identifying its source: more facts, necessary backend
policy, duplicated traversal, or a new mechanism. An estimate overrun alone
does not justify weakening proofs; a proliferating mechanism requires revisiting
the design before continuing. Passing CI does not waive this review.

The required maintenance traces are:

| Representative change | Authored edits | Derived outputs and independent evidence |
| --- | --- | --- |
| Add an ordinary option | Native option declaration; owning guide only if meaning needs explanation | Native option/reference index; focused configured-value case; no compiler/backend change |
| Change a shared bound | Its shared constraint definition; explicitly separate bounds stay separate | Native type and applicable wire/command projections; independently changed boundary expectations |
| Add a wire field | Structural declaration, capability record, native producer value and consumer semantics | Generated Rust/Nix constructor/reference and ABI digest; raw-byte/coherence/admission expectations |
| Add a command argument | Syntax declaration, native parser branch and handler logic where needed | Shared type/token/default/help/reference bindings; independent parser precedence and no-effect cases |
| Change native behavior without shape changes | Native owner, normative explanation, behavioral tests; capability descriptor if runtime contract changes | Reference prose and ABI snapshot where applicable; ordinarily no schema/compiler/backend edit |
| Add an export of a supported kind | Its publication descriptor and native implementation | Actual export and reference entry; existing coverage check observes it |

Record the actual edit path after implementing these changes in a small fixture
or a reviewed demonstration. Expected results must not be generated from the
same declaration under test. A routine supported addition that needs a new
parser, validator, generator, registry, or proof harness fails the reuse claim.

## 11. Stable whole-project acceptance gate

The following checklist defines completion. Progress through units is evidence
toward it, not a substitute for it.

- **A1 — Clean origin:** implementation started from the recorded baseline and
  reviewed handoff. No archived implementation, fixture, generated file, prototype
  path, or abandoned ABI edit was supplied to or reused by the engineer.
- **A2 — Complete coverage:** every supported baseline public surface and the new
  docs feature is accounted for through actual options/exports, inventory,
  command declarations, or native normative topics. Missing required metadata,
  duplicate identities, dangling references, and wrong-kind bindings reject.
  Final-export audits also reject manual publication bypasses; known inventory
  omissions do not exclude public outputs from coverage.
- **A3 — Native authoring preserved:** nested reuse, overrides, required-value
  laziness, contextual defaults, native transformations, and module arguments
  retain baseline behavior. Static reference does not force project values.
- **A4 — Structural ownership:** model, JSON result, and structured error payloads
  share one structural mechanism. Field/presence/default/unknown-field policies
  are explicit. Native domains, serializers, and cross-field rejection remain
  enforced; inventory punctuation does not select field policy.
- **A5 — Independent admission:** malformed raw input and falsely derived facts
  reject at their owning boundary. Invalid admission starts no child. Independent
  Nix/Rust derivation evidence remains independent.
- **A6 — Command preservation:** all section 7 cases pass against native parsers.
  Syntax-fact integration changes no encoding, precedence, default-resolution,
  handler, or effect phase. No generated scanner or parser-policy interpreter
  is introduced. Any deliberate behavior change is separately recorded and proven.
- **A7 — Native behavior:** output/replay, context, secrets, errors, containment,
  lifecycle, and cleanup retain native ownership and appropriate behavioral
  evidence. No metadata integrity check is presented as proof of an OS fact.
  Generated views do not bypass native value conversion, redaction, formatting,
  failure handling, or writes.
- **A8 — Product delivery:** project help exposes docs; exact option/API lookup,
  topic navigation, useful failures, and source provenance work. The original
  stateRefs question and placeholder-scope questions are answered accurately.
- **A9 — Revision and dependency isolation:** pinned downstream sources use their
  own content; docs do not require model/runtime realization. CLI/runtime/test
  product source boundaries remain intact. Generated sources are fresh.
- **A10 — Replacement complete:** the ledger names actual deleted definitions.
  No superseded readers, duplicated schemas, rule dispatcher, context/text/byte
  generator, prototype implementation, or competing active handoff remains.
- **A11 — Maintenance demonstrated:** the section 10 traces and cumulative cost
  report explain authored edits, generated outputs, added dependencies, and
  removed owners. Ordinary supported additions reuse existing machinery.
- **A12 — Verification complete:** relevant focused checks and final cross-layer
  checks pass on the exact delivered tree, with platform/release limitations
  explicitly reported. No claim of completion rests on an earlier partial tree.

Use the narrowest proof first, then widen under DEVELOPMENT.md:

1. Native Nix metadata/evaluation and structural mutation vectors.
2. Independent wire/command expected cases and generated binding compilation.
3. Affected crate tests in the pinned fixture-backed environment, reference and
   package builds, and applicable source-isolation checks.
4. Cross-layer integration checks when a coherent coupled cutover exists.
5. At final integration, nix run .#ci -- --dirty and affected release/package
   checks required by DEVELOPMENT.md; separately report hosted/macOS or other
   platform coverage not exercised locally.

Do not repeatedly launch full CI while the relevant producer/consumer cutover
is incomplete. Record the tree/source identity for check results. A prior run
against a different tree is historical evidence, not final acceptance.

For this RFC-only revision, verification is document review, baseline source
cross-checking, link/format checks, and inspection that unrelated files were
preserved. It does not claim any implementation acceptance item has passed.

## 12. Architectural review and handoff

The architectural review covers the worked examples, baseline command matrix,
and replacement ledger. RFC_ENGINEER_HANDOFF.md records the completed preparation
and remaining implementation proofs. No difficult family is delegated to
"design it when that migration begins."

The handoff package is the clean baseline, this RFC, the baseline normative
documents, and independently specified acceptance cases. The archive is excluded
from workspace, instructions, and test dependencies. Architects remain responsible
for translating any further archive analysis into explicit requirements without
passing its implementation through to the engineer.

At completion, move enduring architectural guidance into the appropriate existing
developer/architecture documents and mark this RFC implemented with its evidence.
Do not leave two competing grammar authorities or turn the RFC into a running
implementation journal.
