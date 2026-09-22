# Documentation metadata and relationship audit

Status: architectural audit with the selective migration implemented in the
working tree; this report is not an amendment to the product contract.
Originally prepared against `9529b90` on `ex-adp-api`
and the existing uncommitted topic-section changes, 2026-09-22. Five architect
reviews covered authoring, runtime, maintenance, relationship assembly and
independent counter-review. The inventory below records the audited baseline.

Delivery: topics now compose focused authored sections with metadata-derived
definitions. Checked relationships supply forward links and derived backlinks
for exact references. Duplicated reference inventories were replaced with
command pointers in the README, guide, adapters, architecture, contract,
development guide and downstream example. Native behavioral explanations,
normative expectations and examples remain authored. Deferred owner extensions
D1–D9 remain deferred. Metadata-only changes may remain absent from the existing
source-scoped upgrade documentation diff, as accepted for this migration.

Readability follow-up: four independent architect reviews compared the baseline
with rendered topics. Their fixes restore adapter composition, runtime identity
and placement, downstream workflow explanation, dirty-mode verification guidance,
and representative derivation examples. Topic selectors now preserve reading
order; related topics precede supporting definitions; error tables replace
repetitive cards without directing readers back to the current topic. Exact
queries retain detailed fields. Reader-scenario assertions complement structural
reference checks; neither replaces editorial review of the explanation.

Context follow-up: topics now compose ordered, checked fragments across authored
files. Output guidance precedes its contract, and derivation includes the complete
golden-vector section so examples retain their parent context. Error destinations
are explicitly contextual (`contextTopic`), not recovery instructions; their
multiline entries and backlink lists suit terminal reading. Native option prose
no longer duplicates topic navigation, and OPTIONS was regenerated. The
`PROC_ESCAPE` annotation now includes failed containment/identity verification,
matching native emitters. State/recovery guidance asks which check failed and
preserves the distinction between cleanup policy and live ownership.

## Decision

Replace duplicated inventories and reference passages with views of their real
owners. Compose topic explanations from authored narrative plus those views and
checked relationships. Do not replace entire authored documents with metadata
strings. Moving a paragraph into a Nix string without sharing its facts or
relationships does not remove a maintenance burden.

Accepted delivery decision: detailed reference material may move from Markdown
into metadata-backed docs output. Keep concise explanations and exact commands
in the owning Markdown, for example:

```sh
nix run .#docs -- topic state
nix run .#docs -- option 'nixfied.services.<name>.stateRefs'
nix run .#docs -- api command clean
```

The user explicitly accepted this approach after the architect review. A new
generated Markdown snapshot or rendered-reference upgrade diff is not a
prerequisite for the migration.

The invariant is one owner per fact. Structural definitions own structural
facts; native implementations own behavior; authored guides/specifications own
explanations, rationale and independent normative expectations. Navigation is
presentation data. Missing or wrong-kind navigation targets must reject before
reference packaging. Native behavior still requires independent verification.

There are four different actions in this report:

- **Generate:** an existing owner supplies the exact reference facts. Reuse it
  in a topic or generated block and remove the equivalent manual inventory.
- **Compose:** retain explanation and examples; add relevant generated entries
  and derived navigation. Some associations must first be authored once.
- **Defer:** a native/checker-owned definition or a design decision is missing.
  Do not substitute a second documentation-only registry for it.
- **Keep:** retain the authored passage as explanation, policy, specification or
  evidence. Related links can be added without replacing its content.

These labels can coexist within a section. A type/default list can be generated
while the paragraph explaining its operational consequences stays authored.
Small mentions inside examples are not automatically duplicate authorities and
should not be replaced by sprawling generated reference output.

## Verified scope and current implementation

The audit covers all 11 tracked Markdown files existing at its start:
`README.md`, `AGENTS.md`, `docs/ADAPTERS.md`, `docs/ARCHITECTURE.md`,
`docs/CONTRACT.md`, `docs/DERIVATION_SPEC.md`, `docs/DEVELOPMENT.md`,
`docs/GUIDE.md`, `docs/OPTIONS.md`, `docs/known-gaps.md`, and
`examples/downstream/README.md`. This report is a new audit artifact, not another
product documentation authority. Generated packaged references and model views
are covered separately below. Historical RFCs and archived fixtures are evidence,
not active Markdown replacement targets.

A read-only evaluation of `packages.aarch64-linux.docs.serialized` confirmed:

| Current inventory | Count |
| --- | ---: |
| Native option entries | 128 |
| Topics | 17 |
| API entries | 131 |
| Apps / module arguments / commands | 16 / 4 / 7 |
| Errors / functions / adapter modules | 27 / 3 / 3 |
| Packages / records | 23 / 48 |
| Structural vocabulary entries | 23 |

These are observed counts, not new product constraints. API entries serialize as
`{ kind, id, text }`; options serialize as `{ name, loc, text }`. Structured
references are formatted into prose and discarded. Topics read authored sections
and manually selected related-topic names. They do not collect the API entries
that reference them. See [reference.nix](../nix/docs/reference.nix),
[topics.nix](../nix/docs/topics.nix), and the existing
[reference check](../nix/checks/reference.nix).

Other verified gaps:

- All options receive the same authoring/state/placeholders guidance.
- All records receive model/derivation guidance, including output records.
- Adapter and module-argument publications currently declare no references.
- Existing runtime commands point to `commands`, their related output record,
  and the error envelope; these are generic reference edges.
- Option metadata currently rejects additional attributes such as references.
- Vocabulary entries are rendered in the packaged reference but have no
  standalone `docs api` query kind.
- Topic coverage is an editorial selection, not a full document index. For
  example, `development` selects only “Canonical local checks”; `state` includes
  nested context and secrets sections. More metadata must not recreate a broad
  whole-reference dump under a topic name.

## Relationship ownership and proposed use

| Relationship | Actual owner today | What may be rendered | What it does not establish |
| --- | --- | --- | --- |
| App to command or topic | `nix/project-publications.nix`, `flake.nix`; checked by `nix/meta/publications.nix` | App routing/reference and inverse “related apps” | A prerequisite execution sequence |
| Publication to another entry/option/topic/command | `references` plus supplied target inventory in `publications.nix` | Existing “see” links and derived backlinks | `requires`, `produces`, runtime effect or failure reachability |
| Command to topic/record/error | `nix/meta/commands.nix`, checked by `syntax.nix` | Related records/topics and inverse command references | Unconditional output production; current edges do not encode output-mode conditions |
| Record field to nested record | `RecordRef` in `model.nix`/`outputs.nix`, checked by `structure.nix` | Field type and inverse “used by”, including lists/maps | Authoring-to-wire lowering or runtime consumption |
| Field/argument to enum domain | `Enum.coordinate` in structure/syntax declarations, members from `capability.txt` | Accepted vocabulary and users of that vocabulary | Per-member lifecycle meaning or behavior |
| Error to recovery topic | `outputs.nix` error annotations, validated by `structure.nix` | Error-specific recovery guidance and inverse topic error index | Retryability, guaranteed remediation or failure precedence |
| Topic to prose sections / other topics | `nix/docs/topics.nix` | Section selection and navigation | Native semantics |
| Topic to selected options/records/other entries | Missing for most desired associations | Proposed checked presentation membership and its inverse | A new semantic owner for the selected entities |
| Option to lowered field / constraint / identity effect | Not represented generally | Defer stronger claims to owner-backed work | Names or structural similarity cannot establish these relations |

Do not expand the last two rows into a universal relation language. The immediate
need is a private presentation index retaining existing edges and a small number
of explicitly authored topic selections. Reuse existing edges; derive inverses
from them. Do not parse generated “See …” sentences to reconstruct relationships.

For missing membership, `topics.nix` can own checked entity selectors once.
An alternative is declaration-local topic annotations, but choose one owner per
association, not both. Declaration-local option annotations would require a new
channel through the native option wrapper/normalization; they do not work today.
Canonical option `loc` paths and whole namespace segments must be used. New
selection code must preserve shared invocation mounts and lazy defaults/bindings.

A bounded topic should show concise relevant entries and exact-query links,
not recursively print every reachable record, global error, or entire namespace.
Navigation may be cyclic; it is not an execution graph. A record or command
linked through a topic remains a reference, not a newly inferred prerequisite.

Examples that must remain distinct:

- `clean` is related to `down`; it does not universally require a prior `down`.
  The actual condition is the cleanup owner's live-process/lease/policy gates.
- `stateRefs` is serialized but discarded by execution lowering. An option/field
  correspondence does not imply an effect on service identity or state placement.
- An adapter's `contributes` description is prose, not typed service/task IDs or
  the final configuration after native module overrides.

## Replacement candidates with existing owners

This is the actionable replacement list. It identifies exact facts rather than
claiming every containing section can be generated. Replace removed reference
passages with concise orientation and exact docs queries, using the accepted
delivery approach above.

| ID | Markdown passages | Replace with | Relation and source owner | Preserve |
| --- | --- | --- | --- | --- |
| G1 | GUIDE “Fresh project”, “Run and control”, “Upgrade and recover”; command lists in CONTRACT “Native command parsing” | Visible command/app argument matrix, help/domain/default reference | App to command from publications; command arguments from `nix/meta/commands.nix` | Selection/repetition/help precedence, replay behavior, workflows; hidden flags stay explicitly identified |
| G2 | README “Quick start” / “Developing Nixfied”; GUIDE integration/discovery; CONTRACT “Task and service algebra”; DEVELOPMENT tool inventories | Fixed public app/package/check names, descriptions, usage; reserved project-app list | Actual publication declarations in `flake.nix`, `nix/project-publications.nix` | Tutorials, dynamic adopter verbs and native operational guarantees; root/check is not the runtime check command |
| G3 | GUIDE “Author nixfied.nix” / “Source and invocation context” | Four framework-supplied module argument entries | `nix/modules/providers.nix`; topic to argument association | Ordinary Nixpkgs arguments and native module evaluation explanation |
| G4 | GUIDE “Use and extend adapters”; ADAPTERS introduction and per-adapter reference material | Actual adapter catalog, usage and authored contribution descriptions | `nix/adapters/default.nix`; module to adapters/services topic | Conventions and exact evaluated/default configuration, which are not typed publication facts |
| G5 | GUIDE authoring/state/descriptive references/secrets/context option mini-references | Relevant option type/default/example/description cards | Native `nix/modules/` declarations through `nix/meta/options.nix` | Explanatory distinctions and behavioral proofs; avoid duplicating the same prose in cards and surrounding text |
| G6 | Output-mode name lists in GUIDE/CONTRACT/ARCHITECTURE; lifetime/source/secret-kind alternative lists | Vocabulary members and corresponding option/argument domains | `capability.txt`, checked vocabularies, syntax/native option owners | Meaning of lifetime modes, output resolution precedence and redaction rules |
| G7 | Field/record inventories in CONTRACT model/error sections; ARCHITECTURE invocation shorthand | Record field descriptions, nested shapes, domains and presence policy for actual fields | `nix/meta/model.nix`, `outputs.nix`, normalized `structure.nix` | Admission, identity, phase and algorithm guarantees |
| G8 | Error-code/recovery listings and fixed diagnostic shape descriptions in CONTRACT “Runtime error diagnostics” | Error index, recovery backlinks, fixed nested record reference | `outputs.nix` annotations and structural records | Open native detail-key production, numeric exit mapping, cause/redaction policy |
| G9 | GUIDE integration function mini-reference | Function input/result/usage descriptions | `flake.nix` library publication definitions | Runnable integration examples and source/locking procedures |
| G10 | Generic guidance currently emitted on every option and record page | Topic-specific links and backlinks | Existing references plus single-owned checked membership | No fabricated option-to-runtime consumption links |
| G11 | Manually repeated topic/API inventories and “related” lookup instructions in discovery surfaces | Generated topic/API inventory and consistent query links | Current topics/publications/commands/records/error inventory | Docs/help query grammar remains owned by native dispatcher unless deliberately shared |
| G12 | Downstream README “What it declares”: exact tasks, services, steps and slot facts | Existing compiled example model view or a link to it | `nix/compiler/views.nix` over compiled `model.json` | Example purpose and concrete execution walkthrough; this is project data, not framework metadata |

G1–G11 require presentation work and checked associations, not a new runtime
contract. G12 uses the existing model-derived product. `capability.txt` remains
the authored ABI vocabulary; it must not become generated documentation or be
replaced with a second vocabulary list.

## Full Markdown coverage

Line numbers below identify the audited working tree and are navigation hints,
not selectors or durable identifiers. Every heading is accounted for; document
introductions and generated families are explicitly included. G IDs refer to the
candidate table; D IDs refer to deferred work below.

### README.md

| Section (line) | Decision |
| --- | --- |
| Introduction (1) | Keep product purpose; compose task/service/authoring links. |
| Quick start (18) | Keep walkthrough; generate only reference inventories via G2. |
| How the boundary works (74) | Keep ownership explanation. |
| Ownership in practice (84) | Keep guarantees/exclusions; compose state/model/control links. |
| Documentation (114) | Keep curated reading map; G11 can supply topic navigation without a second document registry. |
| Examples (130) | Keep descriptions of what examples demonstrate; directory names cannot supply their meaning. |
| Developing Nixfied (148) | Keep workflow; G2 supplies actual tool reference descriptions. |

### docs/GUIDE.md

| Section (line) | Decision |
| --- | --- |
| Introduction (1), Install or integrate (8) | Keep orientation. |
| Fresh project (10) | Keep walkthrough and file responsibilities; G1/G2 for install syntax/reference. |
| Existing flake (34) | Keep composition/locking example; G2/G9 for public products/functions. |
| Discover the project surface (111) | G2/G11 plus authored source-bound help/docs/model distinction and broken-project recovery. |
| Author nixfied.nix (180) | G3/G5/G9 plus full example and task/composition/cache explanations. |
| Run and control (304) | G1/G6; retain direct-leaf selection, stream/replay behavior, examples and status guidance. |
| Services, slots, and state (347) | G5/G6 for option facts and member names; retain lifetime meanings, cleanup gates and placement guidance. |
| Descriptive references and state roots (400) | G5; retain labels-versus-storage and raw-model-hash-versus-reuse-identity explanations. |
| Source and invocation context (419) | G3/G5 for native argument/option facts; D2 for host selection table; keep empty/unset, OS-byte, environment and advisory behavior. |
| Secrets (459) | G5/G6 and related descriptors/errors; retain safe example, admission/redaction boundaries and child responsibility. |
| Use and extend adapters (480) | G4 plus authored module-merging example. |
| Upgrade and recover (498) | G1 for flags, recovery/error links; keep old/new pin sequence, preflight transaction and recovery decisions. |
| Keep project documentation project-specific (570) | Keep editorial ownership policy. |

### docs/ADAPTERS.md

| Section (line) | Decision |
| --- | --- |
| Introduction (1) | G4 plus authored native-module example and generic-runtime explanation. |
| What an adapter provides (23) | Compose generic structure/options and G4 descriptions; retain conceptual task/service/closure roles. D6 before typed contribution claims. |
| Conventions (35) | Keep idempotence, protocol probes, state layout, sockets, platform and shutdown guidance; link lifecycle options. |
| Parameterization (68) | Keep native module merging; compose selected lifecycle/placement options. |
| Endpoints and placeholders (75) | D1 for syntax/scope matrix; compose endpoint/invocation options now; keep examples. |
| Multiple listeners: model every endpoint (98) | Keep ownership explanation and no-wrapper-derived-ports rule; compose endpoints/primaryEndpoint definitions. |
| Endpoint-less services: durable is not listening (116) | Keep conditional probe/effect/addressability rules; compose relevant options; D1/D3 before stronger generated constraints. |

### docs/CONTRACT.md

| Section (line) | Decision |
| --- | --- |
| Introduction (1) | Keep normative authority and ABI meaning; compose owner links. |
| Model and version boundary (25) | G7 for structural facts; retain exact ABI, store-origin, realization and pre-spawn guarantees. |
| Ownership boundary (55) | Keep responsibility/effect boundaries. |
| Task and service algebra (74) | G2/G7 for names/shapes; retain two-kind/static algebra, derivation ownership, hermeticity and discovery guarantees. |
| Source, identity, and endpoints (131) | G5–G7 for option/domain/shape facts; keep source/identity/ownership conditions; D1/D3 for stronger constraints. |
| Registry, state, and processes (165) | Compose state/containment/lifetime/control definitions; retain registry, liveness, cleanup and redaction guarantees. |
| Output and failure contract (193) | G1/G6/G7; retain upgrade transaction, output selection, replay order, stream effects and failure precedence. |
| Runtime error diagnostics (247) | G8 for error/fixed nested shapes; D4 for open detail-key/exit-status reference; keep native cause/redaction behavior. |
| Definitional boundaries (282) | Keep product exclusions. |
| Changing the contract (290) | Keep atomic change procedure and independent proof obligations. |
| Native command parsing (309) | G1 for syntax inventory; keep native acquisition, encoding, repetition, help/error precedence and effects. D3 for any proposed behavioral declarations. |

### docs/ARCHITECTURE.md

| Section (line) | Decision |
| --- | --- |
| Introduction (1), The problem (14) | Keep purpose/motivation. |
| The load-bearing decision: two tools, one seam (27) | Keep Nix/Rust rationale; compose relevant publications/records/controls. |
| Why one model.json seam (62) | Keep tradeoffs and history; link Model/admission reference instead of duplicating inventories. |
| Shared contracts and the static reference (92) | Keep architectural ownership explanation; update it if presentation architecture changes. |
| The task–service algebra (136) | G7 for inline field summaries; keep anonymous invocation/static composition/cache rationale. |
| Correctness in four layers (193) | Keep phase ownership and reasoning. |
| Identity & placement (212) | G6 for source alternatives; keep placement/identity formulas and host behavior (D2/D3). |
| Registry, liveness, leases (235) | G6 for lifetime names; keep OS reconciliation, reuse/replacement and lease meanings. |
| Ports, state, containment (261) | Compose controls/records/error links; keep lock, ownership, cleanup and race-limit explanation. |
| Secrets (318) | Compose G5–G7; retain values-only-at-runtime and redaction scope. |
| Output Control (327) | G1/G6/G7; keep default precedence, direct-leaf restriction, ReplayTicket/finalization and refusal rationale. |
| Verification boundary (366) | G2 for published tools; keep independent-evidence and platform/VM tradeoffs. |
| Definitional boundaries (395) | Keep rationale for exclusions. |

### docs/DEVELOPMENT.md

| Section (line) | Decision |
| --- | --- |
| Introduction (1), Repository map (7) | Keep contributor routing; do not create a file registry merely to generate it. |
| Canonical local checks (40) | G2 for actual tool/package inventory; keep fixture/toolchain requirements, raw Cargo limits, regeneration procedure and source isolation. D5 for ordered CI stages. |
| Authoring and publication ownership (145) | Keep boundary explanation; compose links to actual declarations/artifacts; update topic-rendering description when changed. |
| Gate composition (198) | Keep layer choices, fixture history/refresh, interrupted-run evidence and dirty-pin behavior; D5 for stage inventory. |
| Local versus hosted CI (259) | Keep interpretation of actual scripts/YAML and platform coverage; D5 before deriving execution claims. |
| Change-specific verification (278) | Keep contributor proof policy; link generated tool entries. |
| Command syntax ownership (296) | G1 for literal command inventory; keep shared-fact/native-parser distinction and proof rationale. |
| Adopter API maintenance (321) | Keep owner/proof table and independent-evidence limitations. D7 for presence/producer-policy tables; per-field normalized policies are already renderable by G7. |

### docs/DERIVATION_SPEC.md

| Section (line) | Decision |
| --- | --- |
| Introduction/status/scope (1) | Keep independent normative specification. Compose structural links without replacing algorithms. |
| 1. Identifiers and canonical order (24) | Keep regex, ordering and equality rules; they constrain both implementations. |
| 1.1 run[0] resolution and executable closure (37) | Keep first-match algorithm and PATH-versus-declared-executable distinction. |
| 2. Flattening and step paths (62) | Keep algorithmic scope and evidence identity. |
| 2.1 Step-path grammar (68) | Keep independent grammar. |
| 2.2 Flattening algorithm (80) | Keep independent pseudocode and cycle/order rules. |
| 2.3 Evidence identity (126) | Keep semantics; compose relevant evidence records. |
| 3. servicesRequired(task) (134) | Keep independent closure/fixpoint specification; link input/result fields. |
| 4. operationBindings(closure) (180) | Keep executable-closure binding and narrowing semantics; tools membership is not binding. |
| 5. Default operation ids and terminal tokens (224) | Keep normative defaults; D8 explains optional supplemental views. |
| 5.1 Operation ids (230) | Keep rules and examples as independent specification. |
| 5.2 Terminal tokens (248) | Keep normative table; do not silently replace its independent role with implementation-generated output. |
| 6. Golden vectors (266) | Keep all literal expected values. |
| V1 nesting/order (277); V2 repeated task (300); V3 direct leaf (316) | Keep each independent vector. |
| V4 servicesRequired closure (322); V5 executable bindings (341); V6 defaults/overrides (361) | Keep each independent vector. |
| V7 shared closure (376); V8 diamond dedup (386); V9 prepare composite (399); V10 connectsTo fixpoint (414) | Keep each independent vector. |

### docs/known-gaps.md and AGENTS.md

| File / section (line) | Decision |
| --- | --- |
| known-gaps introduction (1), Incomplete semantic parity (6) | Keep unresolved evidence limits; no metadata replacement. |
| Gap (11), Existing protections and their limits (24) | Keep distinction between structural consistency and actual runtime effects. |
| Concrete evidence (41) | Keep stateRefs finding and independent test reference. |
| Ownership and follow-up evidence (56) | Keep native proof requirements. |
| Separately scoped review candidates (83) | Keep unapproved design questions distinct from supported API. |
| PostgreSQL lifecycle test reliability (104) | Keep observed incident and uncertainty. |
| AGENTS introduction (1), Core design (15), Boundaries (58) | Keep collaborator instructions and architectural constraints. |
| Authority (79), Change routing (92), Contract and delivery (107) | Keep contributor policy; do not introduce a second owner registry to generate it. |

### examples/downstream/README.md and docs/OPTIONS.md

| File / section (line) | Decision |
| --- | --- |
| Downstream introduction (1), Layout (12) | Keep purpose and file responsibilities. |
| What it declares (22) | G12 for exact model facts; retain why the example exists. |
| Build and run (43) | Keep executable walkthrough; link relevant command references. |
| macOS + Linux (67) | Keep platform explanation; claims require actual integration evidence. |
| OPTIONS introduction (1) and every one of its 128 option headings | Already generated from native declarations via `nix/docs/options.nix`. Keep as a generated snapshot, never manually edit. Relationships belong in the owning declarations/presentation assembly, not in this Markdown output. |

## All 17 topic assemblies

This is a selection plan, not a claim these associations already exist. Every
identifier below was checked against the evaluated reference. Prefix selectors
refer to native option namespaces, not wildcard instance data. Related records
can be linked without expanding every field. Existing inbound references and
recovery associations should be reused, not duplicated in the topic selections.

| Topic | Definitions to select or link | Authored explanation retained |
| --- | --- | --- |
| `commands` | Commands `check`, `run`, `ps`, `down`, `clean`, `install`, `upgrade`; apps referring to each | Native parser rules and effect boundaries |
| `runtime` | Apps `project/run`, `project/model-check`, `project/ps`, `project/down`, `project/clean`; their commands; package `root/nixfied-runtime`; records `primitive/Model`, `local/CheckOutput`, `local/DownReport`, `local/CleanupOutcome`, `output-schema/ps-json` | Responsibility, admission/execution, registry/leases, liveness, containment, cleanup |
| `authoring` | `install`, `root/install`; functions `library/compileModel`, `library/projectApps`, `library/seq`; four module arguments; selected project/task/service option groups | Complete examples, native merging, limits |
| `tasks` | `library/seq`, `run`, `project/run`; `primitive/TaskSpec`, `primitive/StepSpec`, `primitive/Invocation`, `primitive/ExitPolicy`; selected `nixfied.tasks` options; recovery backlinks for `TASK_FAILED`, `TASK_SELECTION_INVALID` | Leaf/composite execution and dependency semantics |
| `services` | `primitive/ServiceSpec`, `primitive/Lifecycle`, lifecycle step records, `primitive/ProbeSpec`, `primitive/Endpoint`; selected `nixfied.services` options; adapter modules; existing service recovery backlinks | Readiness/health/ownership, endpoint-less rules and lifecycle sequencing |
| `state` | `nixfied.state`, `nixfied.slotPolicy`, `nixfied.placement`; `primitive/StatePolicy`, `primitive/SlotPolicy`, `primitive/Placement`; `ps`, `down`, `clean`; state/lease/registry recovery backlinks | State roots, cleanup gates and lifetime/persistence distinction |
| `placeholders` | `primitive/Endpoint`, `primitive/Invocation`, `primitive/SecretDescriptor`; endpoint/dependency/invocation option references; `PORT_CONFLICT`, `PORT_UNVERIFIABLE` recovery backlinks | Substitution syntax/scope and examples until D1 is resolved |
| `secrets` | `nixfied.secrets`; `primitive/SecretDescriptor`, `primitive/SecretSource`; mounted invocation env options; `SECRET_UNAVAILABLE`, `SECRET_LEAK_BLOCKED` | Resolution/redaction/non-disclosure and child responsibility |
| `adapters` | `adapter/synthetic`, `adapter/postgres`, `adapter/reth`; `module-argument/adapters`; selected lifecycle/placement options | Module composition and adapter conventions |
| `context` | `module-argument/pkgs`, `module-argument/system`, `module-argument/adapters`, `module-argument/nixfiedLib`; `nixfied.codebases.main`, relevant target/invocation options; `primitive/Codebase`, `primitive/SourcePolicy`, `primitive/Invocation`; `SOURCE_MISMATCH` | Host paths, environment precedence, OS-byte handling |
| `outputs` | `run`; `run-output-mode` domain; `output-schema/run-json`, `output-schema/run-summary-json`, `output-schema/run-task`, `output-schema/run-node`, `output-schema/run-service`, `output-schema/ps-json`, `output-schema/runtime-error-projection`; output error backlinks | Stream effects, mode precedence, redaction/replay/finalization |
| `errors` | All 27 error annotations; `output-schema/runtime-error`, `output-schema/runtime-error-cause`, fixed nested diagnostic records | Open details, cause construction, exit/failure precedence |
| `recovery` | `upgrade`, `root/upgrade`; relevant state controls; existing `RUNTIME_ABI_MISMATCH`, `PLATFORM_UNSUPPORTED`, `PROC_ESCAPE`, `CANCELED` recovery backlinks | Ordered, conditional operational procedures |
| `discovery` | `root/help`, `root/docs`, `project/help`, `project/docs`; `library/projectApps`; `nixfied.surface.verbs`; package `root/docs`; topic/API inventory | Source-bound help versus static docs, broken-project recovery |
| `model` | `library/compileModel`, `check`, `project/model-check`; `primitive/Model` and linked structural records; model/closure recovery backlinks | Exact ABI/provenance/closure/admission guarantees |
| `derivation` | `library/seq`; `primitive/TaskSpec`, `primitive/StepSpec`, `primitive/ClosureSpec`, `primitive/TerminalSemantics`; relevant field/option links | Normative algorithms and independent vectors |
| `development` | `root/check`, `root/test`, `root/gate`, `root/ci`, `root/regenerate`; packages `check/rust-workspace`, `check/derive-facts-vectors`, `check/nixfied-runtime`, `check/minimal-model`, selected root/devShell packages | Verification selection, maintenance workflow, platform/release limits |

The actual slot namespace is `nixfied.slotPolicy`, not `nixfied.slots`. The audit
checked this against collected options rather than accepting an inferred name.

## Deferred replacements requiring an owner-backed design

| ID | Candidate | Actual owner / missing fact | Required boundary and proof before replacing prose |
| --- | --- | --- | --- |
| D1 | ADAPTERS placeholder scope matrix, endpoint-less constraints | `capability.txt` lists substitution tokens; `nix/compiler/validate.nix`, `derive.nix`, runtime lowering enforce scopes and coherence. Token inventory alone is not a scope definition. | Keep independent Nix/Rust rejection for direct scope, first dependency, primary endpoint, secret-env-only and endpoint-less conditions. A shared syntax definition must be consumed by relevant native code, not just docs. |
| D2 | GUIDE host state/secrets precedence table | `state/placement.rs::state_base_from_env/default_state_base`; `admission/secrets.rs::secrets_base/default_secrets_base`; parser handles explicit state base | Runtime-owned representation must preserve precedence, empty/unset, OS bytes, platform paths and missing-file behavior; context/secret tests remain independent. No host placement in model.json. |
| D3 | Cross-field requirements, command prerequisites, per-command failure sets and lowering/effect relations | Compiler validators and runtime admission/execution/control/main owners, not generic metadata references | Explicitly identify actual predicate/phase and consumer. Preserve no-child admission, cleanup gates, task selection, output precedence and OS evidence. Do not create a behavior interpreter to improve docs. |
| D4 | Open diagnostic producer-key table and numeric exit mapping | Native `error.rs::with_detail/cause_details`, producer call sites, `main.rs::exit_code` | Keep details open. New owner-local annotations/accessors would need actual production consumers and independent output/redaction/encoding tests; a closed docs-only schema is wrong. |
| D5 | CI/gate stage order and platform coverage | `nix/dev.nix`, `nix/gate.nix`, gate model, `.github/workflows/checks.yml` | Only share a small ordered definition if actual orchestration consumes it. Do not infer CI coverage from published package names or make metadata existence evidence of execution. |
| D6 | Exact adapter contributions/default configurations | Native adapter modules plus evaluator/overrides; publication `contributes` is prose | Any evaluated catalog must state system/configuration context, preserve native overrides and avoid forcing products in static docs. Do not create framework-owned IDs for adopter instances. |
| D7 | DEVELOPMENT full presence/producer-policy inventory | `nix/meta/structure.nix` policy branches, `declarations.nix` constructors | A checker-consumed definition could generate the full table. Actual field decode/encode facts already normalize; do not add a parallel descriptive policy registry. Preserve malformed-policy and compiled projection tests. |
| D8 | DERIVATION_SPEC operation/terminal default tables | `nix/lib/derive-facts.nix` functions/defaults plus independent Rust derivation | Supplemental native-owner views are possible without new meta definitions. Retain current normative tables and literal goldens as independent expectations unless their authority is explicitly redesigned. |
| D9 | Docs/help query grammar in GUIDE and usage text | Native `nix/docs/reference.nix` dispatcher and contextual help implementation | Not part of the seven shared command declarations. Share only native presentation facts if justified; preserve native exact query/rejection behavior rather than building a universal parser. |

Current behavioral owner locations above are under
`runtime/crates/nixfied-runtime/src/` where Rust paths are abbreviated.
Deferring these replacements does not prevent adding ordinary checked navigation
to their existing prose.

## Coupled generated products and upgrade reporting

| Surface | Existing owner | Migration requirement |
| --- | --- | --- |
| `docs topic`, `docs option`, `docs api` | `nix/docs/reference.nix` | Preserve relationship data before rendering; derive relevant forward/reverse navigation; keep exact lookup/rejection behavior. |
| Packaged `share/nixfied/reference/API.md` | Same builder | It currently appends all owning authored documents after generated entries. Topic-only changes will not remove those duplicated inventories. Decide its composed layout in the same change. |
| Checked `docs/OPTIONS.md` | `nix/docs/options.nix`, native module declarations | Keep generated snapshot and freshness check; do not author relationships directly in the output. |
| Project `views/docs.md` | `nix/compiler/views.nix`, `emit-model.nix` | Already generated from compiled project facts. Preserve disposable/project-specific status and model.json as the only required semantic seam. |
| Upgrade documentation diff | `nix/install/upgrade.nix::emit_docs_diff`, CONTRACT UPGRADE-1 | Currently compares README.md and regular files beneath docs/ from the two locked sources. It does not compare nix/meta definitions or built API.md. |

The upgrade diff retains its existing README.md/docs source scope. It can show
changes to the authored guidance and command pointers; subsequent metadata-only
reference changes need not appear in that diff. Detailed reference remains
available through docs at the supplying framework revision. This is the accepted
delivery choice, not a migration blocker. Do not add a new snapshot or extend
upgrade scope solely to preserve the removed reference inventories. Existing
OPTIONS.md generation/freshness remains in place. Preserve source binding,
unavailable-report handling and preflight/transaction behavior.

## Verification and implementation order

1. Retain existing structured references in a private presentation assembly and
   derive backlinks. Reject unknown/wrong-kind targets and duplicate identities
   before producing the reference. Do not add a public JSON seam or alter ABI.
2. Add narrowly selected missing topic membership once. Validate native option
   paths, whole-segment namespace selectors, empty selections and selected IDs.
   Avoid transitive expansion that broadens topic meaning or implies execution.
3. Replace blanket option/record guidance; compose the 17 topics from concise
   prose and relevant owner-derived entries. Preserve exact authored-section
   validation and choose nested-section inclusion deliberately.
4. Remove G1–G11 duplicated inventories, replacing them with concise explanations
   and exact docs commands; adjust packaged API.md in the same cutover. G12 remains
   a compiled-model view decision. Keep useful examples and procedures. No new
   snapshot or upgrade-report expansion is required.
5. Consider D1–D9 individually only when the owning layer benefits from the
   shared representation; do not bundle them into a documentation feature.

Required proofs for those future changes:

- Independently expected inclusions and exclusions for every topic, including
  small ones such as secrets and broad ones such as runtime/development.
- A mutation of a real definition must update its reference entry, topic view
  and inverse links together; add/delete/dangling/wrong-kind cases must be covered.
- Record/enum extraction must cover nested collection references. Generic
  references must stay generic in display and not become invented guarantees.
- Mounted native option identity and default/type behavior must remain intact;
  shared invocation fields must not require a per-mount parallel registry.
- Preserve poisoned package/system/default/binding tests, string-context removal
  only at presentation, static docs without model/runtime realization, absent
  host PATH operation and no runtime/state/secret lookup.
- Preserve downstream pin/source switching and native product source isolation.
- Preserve independent `coverage.nix` and final-export audits. Do not turn their
  test-only routing maps into production navigation inputs.
- Verify that replacement Markdown commands resolve to the intended topic,
  option or API entry at the supplying revision and expose the moved details.
  Preserve the existing upgrade scope and transaction/preflight behavior.
- Keep native parser, model admission, derivation, output/redaction, cleanup,
  liveness and OS tests. Renderer self-consistency is not evidence of behavior.

Start with focused metadata/reference checks, then use the downstream/source
isolation gate and broader checks required by DEVELOPMENT for the actual scope.
No runtime test suite is necessary merely to review this audit artifact.

## Audit evidence and review disposition

The primary audit inspected Git status/history, enumerated all tracked Markdown
headings outside fenced examples, evaluated the static reference on aarch64-linux,
and checked the proposed entity IDs/option namespaces against that inventory.
The architects independently traced native and metadata owners; a counter-review
checked authority duplication, missing relation data, generated surfaces,
upgrade reporting, and independence of behavioral proofs.

The consensus is G1–G12 as selective replacement/projection opportunities, with
D1–D9 explicitly conditional. Reviews differed on generating the terminal-default
specification table; this report conservatively retains its normative/independent
role and permits only supplemental owner-derived views without a separate
redesign. They also identified two viable places for missing topic membership;
this report requires one owner and favors minimal presentation selectors rather
than simultaneously extending every declaration family. The later user decision
above resolves the review's proposed upgrade-report dependency: concise Markdown
command pointers are sufficient, without new snapshots or broader upgrade diffs.

This audit verifies ownership and migration scope. It does not claim a new
relationship renderer has been implemented, that existing prose has been
removed, or that native behavioral parity is complete. No runtime/platform or
release tests were run for the report. Existing uncommitted implementation work
was preserved.
