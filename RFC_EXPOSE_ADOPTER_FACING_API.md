# RFC: Define Nixfied through a private contract meta-framework

Status: proposed architecture; this document authorizes no implementation or
change to the active contract by itself.

Problem statement: [FIXES_DOCS_REFS.md](FIXES_DOCS_REFS.md).
Normative boundaries: [docs/CONTRACT.md](docs/CONTRACT.md).
Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
Derived facts: [docs/DERIVATION_SPEC.md](docs/DERIVATION_SPEC.md).

## 1. Decision and intended outcome

Define Nixfied's supported public contract through a private Nix meta-framework:
a small contract compiler whose declarations require executable structure,
explanations, and relationships appropriate to each public surface.

Nixfied is the first and only intended consumer. The meta-framework is internal
implementation machinery, with no new adopter DSL or independently versioned
plugin interface. Adopters continue writing ordinary Nix modules.

The same authored contract supplies:

- Nix option declarations, public library exports, adapters, and flake products;
- command grammar, conventional help, environment resolution policies, public
  result shapes, and error contracts;
- phase-specific rules, derivation interfaces, and behavioral explanations;
- the model's wire structure and constructors used to produce `model.json`;
- generated Rust contract bindings compiled into the runtime;
- a connected, revision-bound reference exposed through `.#docs`.

Rust remains the admission and execution implementation. It consumes the
compiled model and statically generated bindings, independently validates
untrusted input, and enforces host and runtime invariants. It does not author or
extract the framework reference, load a documentation graph, or invoke Nix.

The central invariant is:

> A supported public declaration cannot be exposed through Nixfied's publication
> mechanism without its required contract information. Execution and reference
> generation consume the same structural facts; native behavior has an explicit
> owner and independent evidence.

This defines the whole public contract through one construction discipline.
Native Nix algorithms and Rust runtime mechanisms remain implementations of that
contract. The meta-framework does not attempt to express every algorithm in a
new symbolic language.

## 2. Problem and evidence

An adopter asked what alternatives exist to `stateRefs = [ "slot" ]`. Answering
required seven hops through a pinned store checkout, including compiler and
runtime internals. A supplied reference should answer ordinary authoring
questions without that reconstruction.

The current option pipeline is a useful foundation:

- [options.nix](nix/docs/options.nix) uses the existing module evaluator and
  `pkgs.nixosOptionsDoc`.
- The pinned generator exposes `optionsNix` and `optionsJSON`, with canonical
  option paths, descriptions, types, formatted defaults, and examples.
- The current evaluation produces 128 public option entries. Exact lookup can
  reuse these records without parsing Markdown or replacing submodule traversal.

It also exposes the limits of the current representation:

- `stateRefs` is described as participating in service identity, but
  [identity.rs](runtime/crates/nixfied-runtime/src/service/identity.rs) excludes it
  and [lower.rs](runtime/crates/nixfied-runtime/src/execution/lower.rs) discards it.
- Invocation timeouts use a positive-integer predicate, while the generated type
  description says only "signed integer". A native check has lost a fact that a
  structured range declaration could retain.
- [validate.nix](nix/compiler/validate.nix) has 35 check entries, with additional
  public constraints enforced in [derive.nix](nix/compiler/derive.nix).
- Runtime flags, help, defaults, and error projection are spread across native
  parsing branches. Output contracts combine serialized structs with dynamic
  JSON construction.

The correct current `stateRefs` answer must remain explicit: it accepts a list
of strings and defaults to `[ "slot" ]`. These strings select neither storage
backends nor alternative runtime state roots. They do not change service reuse
identity. They remain serialized model data and appear in the model view, so
changing them changes the raw model hash; execution lowering discards them.
The sibling service/task ref fields need the same accuracy review.

Generation establishes agreement between projections. Independent behavioral
proofs establish whether an authored explanation matches execution.

## 3. Scope and guarantees

### 3.1 Public surface

The target is every supported framework-owned public declaration, including open
schema families. Internal Rust `pub` items and framework-only escape hatches are
not automatically adopter API; existing supported surfaces cannot be hidden to
obtain a passing coverage count.

| Surface | Required coverage |
| --- | --- |
| Authoring | Options, nested domains, defaults, transformations, constraints, and supported extension points. |
| Nix integration | Library functions, supplied module arguments, adapter modules, public packages/apps, and artifact layout. |
| Commands | Grammar, cardinality, defaults, effects, baked-in arguments, outputs, and failure behavior. |
| Invocation context | Environment inputs, source resolution, platform conditions, and resolution precedence. |
| Results | Public serialized fields, variants, units, omission conditions, and explicitly open data. |
| Failures | Error vocabulary, phases, detail contracts, exit behavior, redaction, and recovery guidance. |
| Behavior | Derivations, lifecycle, admission, state, containment, cleanup, and execution guarantees. |

Adopter task names and custom flake apps remain project facts. Nixpkgs and child
programs retain their own contracts. Framework schema coverage does not claim to
enumerate arbitrary Nix programs or child behavior.

### 3.2 What is enforceable

| Guarantee | Owner and rejection boundary |
| --- | --- |
| Required public metadata exists. | Declaration compiler rejects incomplete kind-specific records before publishing a reference or executable surface. |
| Public structural facts have one definition. | Option, export, grammar, and wire generators consume their owning declarations. |
| Relationships resolve and respect scope. | Contract compiler rejects duplicate identities, dangling targets, wrong target kinds, and invalid bindings. |
| Rules retain their execution phase. | Existing compiler/runtime owners invoke declared rules with their phase's inputs. |
| Model construction is complete and coherent. | Nix wire construction and existing validation reject malformed lowered values before model publication. |
| Untrusted models remain independently admitted. | Rust validates structure, references, ABI, host facts, and independently derived facts before relevant side effects. |
| Reference content is static and revision-bound. | Packaging binds declarations and provenance to the supplying framework input. |

Nix constructors provide evaluation/build guarantees, not language-level opaque
types. The public facade and coverage checks must catch manual bypasses.

The compiler can reject an absent explanation. It cannot prove that arbitrary
English is true, that a native predicate implements that English, or that a
reader understands it. Coverage, relationship integrity, and behavioral evidence
are separate acceptance dimensions.

### 3.3 Non-goals

- Replace Nix parsing, module evaluation, merging, or package realization.
- Add a universal rule interpreter, runtime schema loader, daemon, or dynamic
  orchestration language.
- Add documentation, implementation callbacks, or reference metadata to the
  model, registry, or runtime admission inputs.
- Generate both independent graph algorithms from one implementation.
- Remove unused model fields, redesign state policy, or change placeholder
  grammar as a prerequisite for documentation.
- Turn the meta-framework into a public framework for unrelated products.

## 4. Architecture: two compilations from one contract

```mermaid
flowchart TD
    D[Owned framework declarations and normative explanations] --> C[Private Nix contract compiler]
    W[Authored capability inventory] --> C
    C --> A[Authoring modules and public exposures]
    C --> R[Validated reference graph]
    C --> B[Generated static Rust bindings]
    C --> S[Wire constructors and serialization rules]
    R --> H[Docs app and readable reference]
    A --> E[Nix module evaluation]
    P[Adopter module] --> E
    E --> V[Native validation and derivation]
    S --> M[Construct and emit model.json]
    V --> M
    B --> X[Rust runtime build]
    M --> X2[Independent admission and execution]
    X --> X2
```

### 4.1 Framework compilation

Framework compilation validates declarations and relationships, generates the
public authoring interface and static bindings, and builds reference content.
Its inputs include the supplying framework source and supported evaluation
context such as system and pinned Nixpkgs. It requires no adopter configuration,
model, executable realization, runtime state, or secret values.

Declarations separate a checked static payload from delayed implementation
bindings. Reference validation must not recursively force callback bodies,
project-derived defaults, package builds, or configured project values. An
implementation receives its context only when its owning phase invokes it.

The normalized declaration index and reverse links are private generated data.
They are neither a second authored catalog nor a runtime semantic artifact.

### 4.2 Project compilation

Project compilation uses the generated module declarations with Nixpkgs'
existing evaluator, then follows the existing pipeline:

```text
adopter module
  -> resolve and merge
  -> validate resolved declarations
  -> derive and lower with phase-local checks
  -> construct declared wire values
  -> realize closures and emit model.json
```

The meta-framework supplies contracts and construction mechanisms to this
pipeline. Native algorithms still compute closure resolution, graph facts,
logical placement, and other derived values. Merely serializing the static
reference graph cannot produce an executable project.

### 4.3 Proposed organization

Keep the kernel small and definitions distributed by responsibility:

```text
nix/contract/                 private constructors, reference checks, projections
nix/modules/                 authoring declarations using those constructors
nix/compiler/                phase-specific rules, lowering, derivation, emission
nix/spec/                    wire shapes bound to the capability inventory
nix/adapters/                adapter exposures and their module implementations
nix/docs/                    reference rendering, lookup, and packaging
runtime/crates/*/             generated static bindings plus native mechanisms
```

The directory sketch is private organization, not a new public path contract.
The compiler imports the owning declarations; it must not require a second
registration file that repeats all public names.

## 5. Declaration model

### 5.1 Closed declaration families

Use distinct constructors with kind-specific required fields. A single record
with interchangeable optional fields would hide invalid combinations.

| Family | Authored contract | Generated or bound implementation |
| --- | --- | --- |
| Domain | Value structure, bounds, alternatives, and relevant units. | Nix type/check and, where representable, wire/native binding. |
| Option | Domain, presence/default policy, explanation, examples, relationships. | Existing `mkOption` surface and structured reference. |
| Exposure | Function/module/product alternative, arguments/results or usage contract, explanation. | Public export or module binding with its existing caller interface. |
| Command | Argument grammar, repetition/default policy, handler identity, results/errors. | Native parser bindings, conventional help, app wiring, reference. |
| Context input | Input name, resolution policy, consuming operations, explanation. | Constants and typed resolver bindings to native implementation. |
| Rule/derivation | Phase, input/output references, executable relation or native implementation, explanation. | Phase-local invocation and connected reference. |
| Guarantee | Normative statement, enforcing owner/phase, relationships, evidence. | Referenced behavioral explanation and proof obligations. |
| Wire/result/error | Serialized structure or explicit open shape, vocabulary references, conditions and meaning. | Model constructors, Rust bindings, result contracts, reference. |

These are compile-time declaration kinds. They add no semantic task/service kinds
to the runtime model.

### 5.2 Reuse existing Nix domains

The initial domain vocabulary needs only primitives, bounded values, enums,
lists, maps, products, and explicit alternatives that existing surfaces use.
Nix-only packages, paths, modules, and functions remain native authoring domains.
Their lowering is explicit; a package does not become a JSON value simply
because it is accepted by a Nix option.

Domain constructors delegate merging and evaluation to Nixpkgs. Add a structured
constraint when it can generate an existing predicate and explain the same fact.
For native checks, require an explanation and preserve their existing mechanism.

Presence policies distinguish required input, optional absence, and a supplied
default. A default of `null` is a real default. Wire presence additionally
records whether absence and explicit null are accepted and what serialization
emits. Nix defaults, wire decode defaults, and command fallback policies are
separate facts unless an explicit relation connects them.

### 5.3 Example: timeout

Illustrative private syntax; constructor spelling is not an adopter API:

```nix
timeoutMs = api.option {
  domain = api.integer { min = 1; };
  default = api.defaultValue 30000;
  unit = "milliseconds";
  explanation = "Maximum invocation duration before cancellation.";
};
```

The domain generates the positive-integer check and the documented lower bound.
The option reference continues to use the existing generator's canonical path,
formatting, and visibility behavior. No separate list of option names is added.

The corresponding model field has its own wire contract and lowering relation.
Its Rust binding must preserve the existing validated domain rather than admit
an unchecked integer and rely on documentation to describe positivity.

### 5.4 Exposure discipline

Generate public functions, adapters, supplied module arguments, products, and
framework app names from their declarations. Derive reserved project-app names
from the same framework exposure set; adding `docs` updates that set once.

Preserve existing caller interfaces, including `lib.seq`'s distinct-task
restriction and ordinary module override semantics. Evaluated adapter defaults
are contextual examples; they are not the effective configuration of every
adopter or a separate adapter option namespace.

Check actual framework-owned exported surfaces against declared entries. Nix
allows manual attrset extensions, so two projections of the same incomplete
list cannot prove that no export bypassed the construction mechanism.

## 6. Relations, rules, and premises

### 6.1 References and scope

Derive option and export identities from their exposure paths. Author stable
identities for rules and behavioral concepts that lack such a path. References
resolve to those entries; a string that happens to look like a path is not an
unchecked escape hatch.

A small relation vocabulary covers current needs: `references`, `constrainedBy`,
`defaultsFrom`, `derivedFrom`, `lowersTo`, `overrides`, `produces`, `mayFailWith`,
and `guaranteedBy`. Each relation has allowed endpoint kinds and any necessary
scope binding. Forward and reverse navigation derive from the same edge.

Open families require distinct bindings even when their rendered segments use
`<name>`:

- A task's `requires` names keys in the global service map.
- A step's `dependsOn` names siblings in that same composite task.
- A service's primary endpoint names a key in its own endpoint map.

The reference compiler checks these schema scopes. Project validation checks
actual instance references. Reference links can be cyclic; they do not define
an execution DAG, phase schedule, or runtime dependency algorithm.

### 6.2 Executable structural rules

For a simple relation, one declaration can generate both validation and its
explanation:

```nix
slotWithinBounds = api.rule {
  phase = "validate";
  relation = api.betweenInclusive {
    value = slotPolicy.default.ref;
    lower = slotPolicy.min.ref;
    upper = slotPolicy.max.ref;
  };
};
```

This removes duplicated option associations and comparison logic. The three
options retain their owning declarations. Preserve existing diagnostics and
evaluation order unless an explicit contract change authorizes a difference. The diagnostic renderer may
remain owner-specific even when the predicate is generated.

### 6.3 Native rules and derivations

Graph, executable, and cross-field rules that need native algorithms retain
those algorithms. Their declarations require phase, relevant inputs/outputs,
explanation, relationships, and an implementation binding. Metadata extraction
never calls the predicate against an incomplete project.

Keep checks where sufficient information exists. In particular, migrate rules
from both `validate.nix` and `derive.nix`: endpoint forms, probe/effect coherence,
executable resolution, narrowing gates, and endpoint capacity are public rules
although some execute during lowering or derivation.

The existing phase owner invokes a rule at its existing position. The reference
graph does not choose invocation order. A missing native binding must fail
construction or compilation rather than silently turn the rule into prose.

Derived facts such as `servicesRequired` and `operationBindings` keep their
native Nix algorithms and independent Rust implementations. Their specifications
remain in `docs/DERIVATION_SPEC.md`, linked by stable identities.

### 6.4 Behavioral premises and evidence

Cleanup confinement, service reuse, redaction, and failure precedence are owned
guarantees. Their declarations bind a normative explanation to the enforcing
owner and relevant public surfaces. A guarantee is not a predicate evaluated by
the documentation builder.

Keep normative prose in `docs/CONTRACT.md` and derivation prose in
`docs/DERIVATION_SPEC.md`. The meta-framework may import identified sections;
requiring a declaration does not require copying all prose into Nix strings.
When explanation ownership moves, replace the former copy with a generated
projection or reference in the same cutover.

Evidence references must point to real checks or fixtures. Their existence is
traceability, not proof of sufficiency. Independent accepted/rejected cases and
runtime tests remain the evidence for important effects. Reuse existing tests
and executable examples rather than generate a second set of illustrative
examples that can drift.

### 6.5 Example: task dependencies

A `requires` entry must lead to the distinct owned facts that:

1. Every element names a declared service.
2. Dependencies participate in readiness, lifetime, and derived service closure.
3. Endpoint-less required services remain valid dependencies.
4. Task-side named endpoint placeholders use required service IDs and select
   their primary endpoints; task-side endpoint IDs are not a separate namespace.
5. Bare endpoint placeholders select the first required service and fail when
   it has no endpoint; they do not skip to a later dependency.

Reference validity, addressability, derivation, and runtime lifecycle keep their
own rules. Their connected presentation answers the adopter's question without
moving all enforcement into one layer.

## 7. Generating model.json

### 7.1 Authoring and wire contracts are distinct

The meta-framework owns declared wire shapes as well as authoring declarations,
with explicit lowering relationships between them. They are not one identical
schema:

| Authoring input | Model construction |
| --- | --- |
| Package-valued invocation tool | Realized closure metadata and tool reference. |
| Singular `endpoint` sugar | Normalized endpoint map and primary endpoint. |
| Tasks, steps, and service dependencies | Static primitives plus independently verifiable derived facts. |
| Exported verb descriptions | Nix app metadata; no model field. |
| `stateRefs` | Serialized current field, explicitly unused during execution lowering. |

This distinguishes transformations, derived values, and Nix-only information.
Reference relationships explain them; native lowering performs them.

### 7.2 Close the producer boundary

Generate wire constructors and serialization policies from the validated wire
declarations. Existing lowering supplies their values. Producer construction
rejects undeclared record members, missing required members, invalid values, and
incoherent alternatives. Dynamic map keys follow their declared domains.

Generated Rust decoding must preserve the existing accepted wire inputs,
including unknown-field, omission, null, and decode-default policies. Equivalent
producer output does not prove equivalent consumer acceptance. Replacing a
discriminated record and its coherence checks with a tagged alternative requires
proof of the same accepted/rejected inputs or an explicit contract change.

A wire field addition must require an explicit construction/lowering decision.
Neither the Nix constructor nor generated Rust bindings may silently supply
zero, null, or an ignored field as a universal fallback. Declared defaults remain
possible only where the existing contract defines them.

Preserve exact emitted names, omission/null behavior, enum encodings, ordering,
and raw model bytes for behavior-preserving migration fixtures. Because the
runtime hashes raw bytes, serialization differences need explicit review even
when decoded values compare equal.

### 7.3 Preserve realization and admission

[emit-model.nix](nix/compiler/emit-model.nix) currently retains `derived.packages`
as build inputs. The new construction path must preserve that closure realization
obligation. Documentation generation must not inherit it.

`model.json` remains the only required per-project semantic artifact. The
existing model-derived `views/docs.md` remains disposable. No contract graph,
schema descriptor, docs path, callback, secret value, or host-absolute runtime
placement is added to the model.

Generated wire structure is not a certificate that a model is safe. Rust still
parses and validates untrusted bytes, checks exact ABI/toolchain and model origin,
proves references, independently re-derives graph facts, and enforces host and
phase-specific runtime invariants.

## 8. Rust as a runtime consumer

### 8.1 Generate bindings before the Rust build

The Nix-owned contract compiler emits static Rust source for the facts Rust must
consume: command grammar and help, field/variant structure, exact vocabulary,
and explicit bindings to native handlers, resolvers, and validated domains.
It emits only the necessary executable projection, not the reference graph.

Rust compilation checks those bindings. For example, a generated command enum
is matched exhaustively by native dispatch; a missing command handler fails to
compile. Domain references bind to existing private/fallible constructors,
newtypes, and validation traits. A generator must not replace `UniqueVec`,
nonzero values, confined paths, or validated endpoint hosts with weaker raw
values merely to simplify emission.

Native Rust owns admission, execution, reconciliation, lifecycle, containment,
state, secrets, output/redaction, and cleanup. Those implementations and their
independent tests remain ordinary Rust. No Rust documentation extractor or
build-host runtime invocation is needed.

### 8.2 Commands and environment resolution

Generate parsing facts and help from command declarations. Native handlers retain
cross-field checks and their typed failures. Framework-injected arguments such
as model paths and exported task selections are distinct from caller-selectable
arguments and unstable framework test switches.

The declaration vocabulary must represent existing behavior before replacing
parsers: repeated flags, missing/invalid values, help precedence, unknown
arguments, numeric parsing, and error output selection. Current commands do not
all have the same repetition or help policy. Preserve those differences unless
a reviewed ABI change deliberately replaces them.

For `run --output`, connect accepted mode vocabulary, repeat handling, the root
task's `defaultOutput`, and the direct-selection restriction checked after model
admission and before slot/state side effects.
For `run --slot`, connect explicit selection, `slotPolicy.default`, inclusive
bounds, and the selected model placement. Default resolution that needs an
admitted model remains a runtime operation.

Environment declarations supply names, consuming operations, precedence, empty
versus unset handling, platform conditions, and explanation. Generated bindings
must be consumed by native resolvers; a detached list of environment names
would leave the original duplication intact. Host paths and secret values are
resolved only at runtime.

### 8.3 Outputs and errors

Generate structural output bindings with exact serialized names, variants,
units, null/omission policies, and relationships. Runtime implementations supply
observed values and enforce redaction and write behavior.

A field implemented as arbitrary JSON requires an explicit open contract. Do
not infer a closed schema from examples. Moving currently dynamic output into a
closed shape requires review of the actual existing output and failure behavior;
where the implementation does not enforce closure, document that limitation.
Fixed public fields assembled with `json!` still need declared contracts. The
absence of a Rust struct does not make their shape open; declaring every JSON
value open solely to pass coverage is not an acceptable migration.

Error declarations require code, meaning, applicable phases, detail contract,
exit behavior, and recovery guidance. Native failure precedence, compound causes,
redaction, and cleanup continuation remain runtime mechanisms with independent
proofs. A generated error enum alone cannot enforce those properties.

### 8.4 Installation tooling

The public install/upgrade contracts also become meta-framework declarations.
The existing installer lives in `nixfied-cli`; upgrade uses Nix-packaged shell.
Those are delivery mechanisms, distinct from `nixfied-runtime`.

For this RFC, keeping Rust runtime-only means Rust mechanisms consume the
authored contract and own no reference-authoring or extraction layer. It does
not silently require rewriting the existing installer in another language.
Generate its grammar/help bindings at its existing owner, preserve its package
isolation, and keep installation behavior outside the runtime model.

## 9. Documentation and build delivery

### 9.1 Adopter interface

The first delivery preserves the concrete discovery interface:

```sh
nix run .#docs
nix run .#docs -- options
nix run .#docs -- options nixfied.services
nix run .#docs -- option 'nixfied.services.<name>.stateRefs'
nix run .#docs -- topic state
nix run .#docs -- topic placeholders
```

`<name>` is a literal canonical schema segment. Namespace listing discovers the
key before exact lookup. The index progressively exposes listable command,
function, adapter, environment, output, error, rule, and guarantee categories.
Their final query syntax belongs to their implementation cutover.

Entries present their explanation, accepted structure, defaults, constraints,
effects, examples, and typed relationships as applicable. Topics cover authoring
limits, tasks/services, state, endpoints/placeholders, secrets, adapters, and
operation/recovery. Topics remain readable without following source-code links.

Unknown names and malformed queries fail nonzero with lookup guidance. Successful
content goes to stdout; diagnostics go to stderr. No browser or interactive pager
is required. The app performs static lookup and formatting only.

### 9.2 One content builder, no Rust build dependency

The app, full readable reference, and any public docs package use one Nix-owned
content builder. Reuse the current option generator's `optionsNix`/`optionsJSON`
and full renderer. Preserve its canonical paths, visibility filtering, formatted
Nix expressions, and distinction between absent and null defaults.

Contract declarations are upstream inputs to sibling documentation and
runtime-binding projections. Building or running the reference must not require
rustc, Cargo, SQLite,
the runtime binary, project executables, or a compiled project model. The
realized docs app invokes neither Nix nor Rust. Build-time rendering tools are
ordinary Nix dependencies and should be measured separately from its runtime
closure.

### 9.3 Generated source and snapshots

Generate Rust bindings deterministically from Nix values. No import-from-derivation
is needed: Nix evaluation must not read a generated artifact back to discover
contract structure. No Cargo build script invokes Nix.

Keep generated Rust modules checked in as disposable source projections so
fixture-backed Cargo and editor workflows continue to use ordinary source.
Regeneration checks compare them byte-for-byte with fresh generator outputs.
Nix product builds verify the relevant projection before compiling the checked
sources; release builds cannot ship stale bindings through an unchecked path.

Only executable contract bytes enter each product's filtered Cargo source.
Reference prose, reverse links, examples, and provenance are excluded. Conventional
help or recovery text deliberately compiled into a product is an executable
projection and can legitimately rebuild it. Preserve separate CLI, runtime/model,
and test-child source boundaries. Freshness checks consume the relevant generated
bytes and checked sources, without adding the full framework source or reference
tree to every product's build dependencies. Prose-only changes outside embedded
text must preserve product derivation identities as well as generated bytes.

Keep a deterministic human projection of the complete public reference under
`docs/`, checked for drift, so the existing README/docs upgrade report includes
explanations authored outside that directory. Exact lookup and this projection
must agree. Generated outputs are edited through their owning declarations.

### 9.4 Revision binding and evaluation availability

Construct the generic docs app from the framework input supplying `projectApps`.
Expose the same generic app on the framework's own flake. Packaged provenance
records that source identity and a revision when available, without assuming Git
or resolving a default branch at runtime.

Keep provenance outside deterministic checked reference/source snapshots.
Embedding the changing source revision into those snapshots would create
regeneration churn and can create self-referential source identities.

There are two separate availability guarantees:

1. **Content independence:** reference generation uses no adopter configuration,
   model validation, project executable builds, admission, or state.
2. **Project app selection:** `.#docs` remains usable when the surrounding flake
   and its project app names can evaluate, even if later model validation fails.

The current flat app attrset merges fixed apps with dynamically named task apps.
Computing those names can force `surface.verbs`. Putting `docs` on either side of
that merge cannot make selection survive a failure while computing the names.
This RFC preserves the current caller interface and states that limit explicitly.
The framework app selected through an explicit supplying source remains the
recovery entry point; it must never silently choose a different framework pin.
A stronger project-app selection guarantee requires a separate integration change.

Contextual `.#help` continues to reflect final project app metadata, including
custom apps and composition. Generic `.#docs` describes the supplying framework
contract. The framework reference, compiled project view, and live observations
retain these distinct scopes.

## 10. Capability inventory and contract ownership

[capability.txt](runtime/crates/nixfied-model/capability.txt) remains authored wire
inventory whose exact bytes derive `runtimeAbi`. It is an input to the
meta-framework, not an output or documentation catalog.

Use a small strict reader for its existing vocabulary records. Rich declarations
reference those members and supply their shape, parsing policy, explanation,
and relationships. Enum alternatives derive from the inventory. Wire fields and
output/error members are checked for exact coverage; references to inventory
members are not a second independently maintained vocabulary list.

This keeps facts complementary:

| Fact | Authored owner |
| --- | --- |
| Exact inventoried wire vocabulary and ABI record | `capability.txt`. |
| Shape, presence, parsing, projection, and exposure contracts | Owning meta-framework declarations. |
| Normative behavior and derivation explanations | Identified sections in CONTRACT/DERIVATION_SPEC. |
| Native implementation and independent proof | Existing Nix/Rust enforcing owners and tests. |

The inventory is not a complete semantic fingerprint. Changing a type,
condition, or algorithm does not automatically change its bytes. Every admitted
vocabulary, model, command, output, error, or behavioral ABI change must still be
recorded deliberately in the descriptor under the current atomic procedure.
The compiler must not claim to infer every such change. Inventory gaps discovered
by coverage work need an explicit contract decision; hiding a supported surface
or silently normalizing the descriptor is unacceptable.

Documentation prose stays outside the descriptor. The raw-byte digest algorithm,
including its treatment of current comments, remains unchanged. Numeric versions
change only when their defined semantics require it.

Adding `docs` is a Nix-only surface change. Its implementation updates VERB-1,
SURFACE-1, namespace rejection, generated apps/help, scaffold pointers, README,
guide, and development instructions together. Constructor migrations alone need
no ABI rotation when admitted behavior and emitted bytes are preserved. Replacing
the capability inventory's authored authority is outside this RFC.

## 11. Proof strategy

### 11.1 Establish the architecture with representative cases

Before a broad migration, demonstrate all four cases through the same kernel:

1. **Timeout:** one range declaration produces a positive-integer option and an
   accurate reference; zero is rejected, the default remains 30000, and lowering
   preserves the field's existing wire domain.
2. **Slot bounds:** one inclusive relation connects three real options and
   supplies the Nix check; independent boundary cases cover both endpoints.
3. **Service cycles:** an owned native rule keeps its phase and diagnostic
   behavior; metadata projection never evaluates its algorithm or a project.
4. **`run --slot`:** a generated native parser binds to Rust resolution, while
   docs links the Nix default, bounds, and placement; independent runtime tests
   prove the omitted and explicit selection behavior.

These cases exercise structural generation, relationships, native algorithms,
and cross-language consumption. Passing only an option-rendering example is
insufficient to establish the proposed architecture.

Kernel prototypes are private proof artifacts until their participating
production owner cuts over. They must not introduce a second live parser. Use
the complete command grammar as the production cutover boundary; the slot case
can exercise the generated/native interface in a focused fixture beforehand.

### 11.2 Required evidence

| Guarantee | Proof |
| --- | --- |
| Complete declared entry | Reject missing/empty required information and invalid family alternatives, while accepting genuine optionality. |
| Real public coverage | Audit actual generated module, export, app, command, result, and error surfaces; include a bypass negative. |
| Relationship integrity | Reject duplicate/dangling/wrong-kind references and invalid schema scopes; verify reverse links. |
| Shared structural facts | Change an accepted range or enum in its owner and verify every affected projection changes together. |
| Phase preservation | Existing native checks retain accepted/rejected behavior, order, diagnostics, and side-effect boundary. |
| Model construction | Reject missing/extra fields and incoherent forms; compare preserved model bytes and closure dependencies. |
| Independent admission | Rust preserves accepted-input behavior and rejects malformed wire values, mismatched ABI, forged derived facts, unresolved references, and invalid host facts. Cover omitted defaults, null versus absence, unknown fields, discriminator coherence, and map-key domains. |
| Safe generated bindings | Compile failures expose missing handlers/lowering decisions; native newtype construction and deserialization remain fail closed. |
| Parser fidelity | Characterize repetitions, help precedence, numeric edge cases, missing values, unknown flags, and error projections per command. |
| Behavioral truth | Independent tests cover effects such as state identity, placeholders, output restrictions, cleanup, and failure precedence. |
| Readable reference | Adopter-question cases cover choices, limitations, relationships, and recovery, including `stateRefs`. |
| Pinning and independence | Distinguishable sources produce matching provenance/content; poison project/model/runtime dependencies and test the stated selection boundary. |
| Build direction | Docs has no Rust/project build dependency; raw Cargo uses checked bindings; product builds reject stale bindings; no IFD or Nix-in-Cargo path. |
| Distribution consistency | Lookup, full reference, packaged content, and upgrade-visible snapshots agree; non-embedded prose edits preserve executable projection bytes and runtime/model/CLI derivation identities, including freshness-check dependencies. |

Generated producer and consumer agreeing with one another is not independent
proof. Retain fixed vectors and mutation negatives authored from the normative
contract, especially for DERIVE-1. Binding/evidence references alone do not prove
coverage or correctness of a native algorithm.

Follow [DEVELOPMENT.md](docs/DEVELOPMENT.md): focused Nix vectors and generation
checks first, relevant Rust tests next, then affected models and adopter gates.
Use `.#test` for the fixture-backed Cargo floor and `.#ci -- --dirty` for the
cross-layer cutover. Report unverified platforms, release builds, and integration
coverage explicitly.

## 12. Delivery sequence

Each stage replaces the participating representation atomically. Generated and
hand-authored implementations of the same fact must not become permanent dual
paths. Earlier stages can ship value while later public categories remain
explicitly incomplete.

1. **Prove the kernel.** Specify declaration families and scope rules, implement
   the four representative cases, establish metadata/project separation and
   generated-source freshness, and measure build dependencies and LOC.
2. **Deliver discovery.** Migrate the necessary option declarations, correct
   inaccurate explanations, reuse the existing option renderer, and ship docs,
   topics, pinning, namespace rejection, help/scaffold pointers, and acceptance
   fixtures as one Nix-only public-surface change.
3. **Complete Nix coverage.** Move public exposures and rules from every enforcing
   Nix phase into declared construction, cover adapters and products, and enforce
   actual-surface coverage without a permanent omissions allowlist.
4. **Generate model construction and bindings.** Bind wire declarations to the
   authored capability inventory; cut over Nix constructors and Rust structural
   bindings together with byte-equivalence and independent admission proofs.
5. **Complete command/context/result coverage.** Cut over native parser/help and
   resolver bindings, output/error projections, installation tooling contracts,
   and remaining guarantee links. Preserve or explicitly revise existing ABI
   behavior; keep every active public surface accounted for.

The prototype of a category is not completion of that category. The full RFC is
complete only when every supported public declaration is covered, all shipped
projections are checked, and the appropriate independent proofs pass.

## 13. Complexity, alternatives, and acceptance

### 13.1 Cost envelope

Planning estimates for net additional implementation/test LOC, excluding moved
code, authored prose, and generated files:

| Work | Estimated LOC |
| --- | ---: |
| Kernel, family constructors, reference validation, projections | 400–900 |
| Nix declaration migration, docs delivery, export integration | 800–1,600 |
| Model constructors and generated Rust structure/parser/resolver/output bindings | 1,400–3,200 |
| Additional focused cross-layer proof infrastructure and cases | 700–1,500 |
| Total | 3,300–7,200 |

Expect further authored explanations and relationships, approximately 1,000–3,000
lines as an initial allowance. Generated Rust/reference size is not independently
maintained authority, but its readability and review cost still matter. The
native-binding backend is the least certain estimate; re-estimate after the four
architecture cases. This is a planning range, not a measured implementation.

Recurring complexity consists of declaration forms, the existing inventory
reader, projection backends, scope validation, and generation checks. Runtime
algorithms gain no schema interpreter or reference lookup overhead. Cold docs
rendering and executable rebuild behavior must be measured rather than assumed.

### 13.2 Alternatives

- Packaging current Markdown improves access but leaves public construction and
  behavioral relationships unenforced.
- A separate documentation manifest duplicates exposure and parsing facts unless
  it becomes the actual owning declaration.
- Rust documentation extraction retains Rust as a reference-authoring source and
  adds a compiler/extraction dependency to reference delivery. This design uses
  Nix-owned declarations and static runtime bindings instead.
- A universal language for Nix evaluation, graph algorithms, and runtime behavior
  would expand the kernel beyond its purpose and undermine native proof boundaries.
- Generating `model.json` directly from the static framework graph would confuse
  available authoring choices with one resolved project's executable facts.

### 13.3 Acceptance decision

Adopt the private contract meta-framework as the construction discipline for
Nixfied's supported public surface. Generate documentation, executable structural
bindings, and the model producer from its owned declarations. Keep native Nix
algorithms and Rust admission/execution accountable to that contract.

Before broad migration, implementation review must settle the exact small domain
vocabulary, the generated/native Rust binding interface, and the deterministic
source-generation layout through the representative cases. Those are bounded
implementation decisions; they do not reopen model seams, introduce a runtime
meta-framework, or change the authored capability authority.

Success means an adopter can discover a supported capability through `.#docs`,
understand its accepted inputs, defaults, relationships, effects, and failures,
and use the same supplying framework input to compile an independently admitted
`model.json` without reconstructing the contract from implementation source.
