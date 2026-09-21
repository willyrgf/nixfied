# Engineer handoff: shared contracts and adopter reference

Status: implemented corrective decisions and delivery requirements. The six
corrections below supersede the original launch instructions and any broader
publication audit or default-safety proposal; they are retained as the review
record, not a second active implementation assignment. The owner cutovers, maintenance
traces and verification entry points are recorded in
[the delivery audit](docs/DEVELOPMENT.md#adopter-api-delivery-audit); its earlier
receipt does not establish acceptance of these corrections. Read [the RFC](RFC_EXPOSE_ADOPTER_FACING_API.md)
for the stable design and A1–A12 gate, and the authority routed by
[AGENTS.md](AGENTS.md) for current behavior. [FIXES_DOCS_REFS.md](FIXES_DOCS_REFS.md)
retains the adopter problem. The preparation receipts below describe the
reviewed starting tree, not evidence for the final implementation.

## Corrective implementation assignment

Work from the current checkout, recording HEAD and the full dirty status before
editing. Preserve the existing implementation and unrelated work; do not reset
to the historical baseline or consult the abandoned archive. The original
five-unit launch below is historical, not an instruction to repeat the migration.
Do not commit unless requested.

The invariant is that supported declarations produce valid native bindings,
already-emitted nested values obey their producer policies, and every framework
option and publication is accounted for without forcing configured values or
native package bindings. The existing structural checker owns value rejection;
the raw option audit owns metadata rejection before documentation rendering;
publication tests observe actual native exports. Native evaluation, decoding,
parsing, effects and behavioral validation retain their owners. State the
specific rejection boundary and independent proof before each implementation edit.

### 1. One structural matcher, two purposes

In `nix/meta/structure.nix`, replace the boolean matcher mode with the explicit
internal purposes `NixWire` and `DecoderLiteral`. Retain one recursive matcher.
`NixWire` validates already-emitted nested values against Nix presence and
omission policies. `DecoderLiteral` validates declared defaults against decoder
rules, including the referenced record's unknown-field policy and decoder
availability. The outer constructor still receives pre-emission inputs: required
null or empty inputs that it must omit remain accepted. Do not validate those
inputs as though omission had already occurred.

Extend `nix/checks/structure.nix` with independent accepted/rejected nested
presence cases and outer null/empty construction cases. Do not add a second
validator, intermediate representation, or reconstruction-and-comparison path.

### 2. Narrow decoder-default restrictions

Within `DecoderLiteral`, reject an encountered `NativeDomain` value. Empty
collections and optional absence/null remain supported because they construct
no native value. Reject a nonempty unique-list default when its element type
contains a `RecordRef`, using the existing reference traversal. Record defaults
must reference decoder-capable records. Ordinary structural defaults remain
supported within these restrictions; a native-free check alone is insufficient.
At default admission, require `isJson literal` alongside the matcher, including
ignored record members. Reject the ignored-function regression before Rust
generation and retain acceptance of ignored JSON values.

The independent counterexample is a unique list of `Child` records defaulting to
`[ {} { value = null; } ]`, with `Child.value` optional. The two raw Nix records
are distinct, but decode identically and Rust `UniqueVec` rejects them. Require
declaration rejection for this case and other nonempty record-containing unique
lists, plus acceptance of the supported empty/absent cases and ordinary
structural defaults. Do not add decoded-value normalization or another equality
algorithm. Keep the restrictions in RFC section 4.2 equally narrow.

### 3. Raw option audit as assertions only

In `nix/meta/options.nix`, traverse native option records and
`type.getSubOptions`, checking visibility, non-internal status and valid metadata
before rendering. Retain the explicit exclusions for native `_module` options.
Pass native `option.loc` into both `getSubOptions` and recursive auditing so
keyed `<name>` mounts are preserved. The mounted-prefix regression must prove
that a prefix-sensitive native type cannot hide undocumented descendants.
Use the existing Nixpkgs documentation renderer after this assertion pass;
remove redundant validation of its rendered metadata. Do not construct another
option representation or duplicate native documentation normalization.

Extend `nix/checks/option-metadata.nix` only with hidden-parent counterexamples
and their valid/lazy counterparts. Prove that hiding a parent cannot hide a
framework option from the audit and that required values, contextual defaults
and poisoned package bindings remain unforced.

### 4. Observe actual publications directly

The existing downstream test in `nix/gate-nix.nix` already evaluates an actual
empty-verbs project and checks exactly `clean`, `docs`, `down`, `help`,
`model-check`, `ps`, and `run` (line 838 at this review). Preserve it; add no new
project-app audit. In the existing `nix/checks/publications.nix`, evaluate the
native resolver and observe:

```nix
actualArgs = resolved._module.specialArgs;
actualAdapters = actualArgs.adapters;
```

Compare each observed set with the existing descriptors using the existing
audit: two positive checks. For each set, append one undocumented attribute and
remove one expected attribute and require rejection: four small negatives.
These observations must work with poisoned `pkgs` without forcing it. Do not
patch source text, add production observation hooks, or introduce another
registry. This replaces publication-specific source-mutation instructions;
the existing product source-isolation matrix remains a separate required proof.

### 5. Help padding and the existing maintenance fixture

In `nix/meta/command-help.nix`, clamp row padding to `max 1`. Preserve all current
help bytes and continuation indentation. Lengthen only the metavar of the
existing `--budget-ms` argument in `nix/checks/maintenance.nix` so its label
exceeds the normal column width. Update the independent help expectation in
`maintenance-native.rs` and the packaged-reference assertion. This same fixture
must exercise declaration checking, help generation and reference packaging;
do not change the token, add a parser branch, or create another derivation.

### 6. Shared Rust string escaping, actual failure-path proof

Extract the escape-aware helper from `nix/meta/syntax-project.nix` and use it in
both that generator and `nix/meta/rust.nix`. Extend the existing
`nix/checks/structure-projection.nix` fixture with an unusual wire field name and
a normal explicit Rust field name. In `structure-projection.rs`, independently
write the expected serialization bytes and compile/test the actual generated
binding. Preserve the syntax literal test in `syntax-projection.nix`.

Structural text defaults alone are insufficient evidence: their double JSON
encoding can mask the broken escaping implementation. Do not add another
escaping framework or a broad test matrix.

### Preservation and acceptance

Preserve capability bytes/runtime ABI, production model/output/help bytes,
decoder and native-domain behavior outside the restricted declaration inputs,
parser tokens/precedence, reference coverage, lazy providers, source isolation,
and the separately approved secret-diagnostic correction below. No compatibility
path, new dependency, semantic seam, runtime branch or generated parser is needed.

Run the focused structural, option-metadata and publication evaluation checks
first, then the existing compiled structural/syntax projection and maintenance
fixtures, generated-source freshness, exact baseline help and packaged-reference
checks. Regenerate through the existing pinned operation only where needed.
Widen to the relevant fixture-backed Rust checks and affected package checks
under `docs/DEVELOPMENT.md`. After all implementation and documentation edits,
run `nix run .#ci -- --dirty` on the exact final tree, plus the required affected
release checks. Do not edit the tree during final verification or present a
previous tree's green CI as acceptance.

Update the delivery receipt with HEAD, dirty diff/source identity, the six
corrections and removed duplication, actual focused/final commands and results,
generated-file freshness, revised maintenance/cost accounting, and unverified
platform/release/integration coverage. Reconcile the delivery audit's completion
claims with that receipt. The architect's reproductions establish the need for
the work; they do not establish that the correction has been implemented.

### Corrective handoff preparation receipt (historical)

This revision records the six settled decisions after inspecting the current
matcher, option collector, publication tests, resolver/provider bindings, help
formatter, compiled projection fixture and downstream empty-verbs assertion.
Only this handoff and the RFC are changed for this preparation. Document checks
passed: `git diff --check`, local file links, balanced fenced blocks and retained
A1–A12 identities. A SHA-256 comparison against the pre-edit file-content
manifest confirmed that all other existing repository files were preserved.
These are preparation checks; corrective implementation checks and final
exact-tree CI remain pending. Earlier delivery receipts remain historical
evidence for their recorded trees.

Implementation follow-up: all six corrections now use the existing owners and
fixtures described above. Focused structure/syntax/options/publication checks,
compiled structural and syntax projections, the maintenance fixture and pinned
regeneration passed. Production generated Rust bytes were unchanged. The
delivery audit records the resulting boundaries; the final execution receipt
records exact-tree CI and release results separately from this preparation
history.

## Starting tree and recovery receipt

Baseline: `c3d9111c861d40b0885a49f3d62950f9871702e1`, branch `ex-adp-api`.
Recovery on 2026-09-21 restored every production source, test, fixture, generated
artifact, dependency, and normative document to that revision. Only this handoff,
the revised RFC, and the revised problem statement differ. Those three documents
were subsequently committed as b0e379b; the production baseline is unchanged.

The architect preserved the abandoned worktree, original index and Git metadata,
binary diff, baseline RFC, and recovery manifest outside the repository. All
245 archived worktree entries were extracted and verified for content and modes
before restoration. Ignored Cargo artifacts were moved out intact. Unrelated
local settings were preserved unchanged. No checkout-local CI/build process was
running at recovery; no earlier CI result is treated as acceptance evidence.
An isolated restoration also reproduced HEAD, the exact original 159-path Git
status, binary diff, and index diff, and passed git fsck and archive checksums.

The archive has no branch, worktree, symlink, fixture, or build-input connection
to this checkout. It is architect-only recovery material. Do not locate, read,
copy, port, compare against, or ask another agent to consult it. Start a fresh
engineering session with this checkout and handoff; exclude the abandoned
implementation conversation. Existing committed framework code is the baseline,
not a directive to rewrite Nixfied itself. Historical RFC revisions are superseded.

## Settled choices and first work

The four mechanisms are native option metadata, shared serialized structure,
command syntax facts consumed by native parsers, and publication descriptors.
Algorithms, effects, native transformations, parser sequencing, context, text,
byte streams, and behavioral enforcement retain their native owners. There is
no prototype phase, universal parser, rule interpreter, or additional execution
grammar. The RFC fixes passes, laziness, source isolation, field policies, public
docs interfaces, and unsupported forms.

RFC sections 4.2.1 and 6.7 fix private Rust bindings: generate owned data
definitions with native impls, borrowing only for actual temporary projections.
Sections 4.2.2 and 6.8 fix nested diagnostic ownership and native help formatting.
These are decisions to implement, not alternatives to resolve during migration.
Reference packaging discards string context only at the presentation boundary
specified in section 8; native values retain their dependency context.

Begin RFC unit 1 as one coherent authoring/publication/docs cutover. Implement
checked metadata without replacing Nixpkgs modules; migrate actual exports and
reserved app names; deliver revision-bound docs and the original stateRefs and
placeholder answers; remove superseded lists. Run its focused and downstream
proofs before proceeding to unit 2. Do not first create empty implementations
of all later mechanisms. Early app descriptors reference native topics until
their command declarations land; no dangling future references are admitted.

Units 2–4 replace model structure, result/error/status structure, and command
facts at their existing consumers. Unit 5 closes reference coverage and audits
the complete delivered tree. Apply RFC A1–A12 throughout; local green checks
never establish whole-project completion. Record actual deleted owners and
proofs against the rows below at each cutover. Any proposed new mechanism needs
architectural review before implementation, rather than an RFC progress entry.
Ordinary supported declarations use the already specified mechanisms.

## Engineer-agent launch

Use the supplied reviewed handoff commit and a clean working tree. Record the
starting commit and status before editing; do not substitute an earlier RFC
revision. Preparing the handoff does not start implementation: launch a fresh
engineer session separately against that revision.
Read AGENTS.md and its routed authorities in a fresh session, without the
abandoned implementation conversation or archive. Do not commit unless requested.

Before the first implementation edit, give a brief unit-1 interpretation naming
the invariant, owners, interfaces, evaluation dependencies, rejection boundaries,
definitions being replaced and focused proofs. Instantiate the settled design;
do not write another RFC or scaffold later generators. Then implement unit 1
autonomously within its specified boundaries.

After each replacement unit, audit actual removed owners, remaining duplication,
new mechanisms/dependencies, maintained-code growth and proof results against
the coverage rows and RFC A1–A12. Report unverified coverage explicitly. Complete
the unit's coupled cutover and required checks before starting the next unit.
Proceed on ordinary implementation choices; return a concrete architectural
departure for review before implementing it. Do not enlarge the acceptance gate
or absorb adjacent defects merely to keep local work moving.

## Baseline option coverage: 128 entries

These finite families include container options. `<name>` is an open keyed
family, not a wildcard accepting undocumented child fields. All rows use native
authoring (unit 1); native compiler relational validation and lowering remain.
The exact current paths are also readable in [OPTIONS.md](docs/OPTIONS.md).

Shared mounted forms, counting the option itself:

- Invocation (8): `tools`, `run`, `env`, `codebaseId`, `cwd`, `stdin`, `timeoutMs`.
- Terminal (3): `success`, `failure`.
- Probe (13): `kind`, Invocation at `invocation`, `timeoutMs`, `retryIntervalMs`,
  `maxAttempts`.

All paths below are under `nixfied`.

| Family and complete members | Count | Existing declaration owner | Required preservation evidence |
| --- | ---: | --- | --- |
| `project.{projectId,name}` | 2 | `nix/modules/project.nix` | Native accepted/rejected IDs and required name |
| `target.system` | 1 | `nix/modules/project.nix` | Contextual defaultText, two systems, explicit override |
| `surface.verbs` | 1 | `nix/modules/project.nix` | Existing dangling/type/list/empty/collision negatives; new docs collision |
| `slotPolicy.{min,default,max}` | 3 | `nix/modules/project.nix` | Native bounds and relational slot rejection |
| `codebases.main.{logicalRoot,sourceMode,sourceIdentity,dirtyPolicy,admissionFingerprintPolicy}` | 5 | `nix/modules/source.nix` | Native path/string apply, source/dirty modes and required-value laziness |
| `state.{markerIdentity,stateEpoch,cleanupPolicy,persistence}` | 4 | `nix/modules/state.nix` | Marker, epoch, cleanup and persistence behavior unchanged |
| `placement.ports.{base,windowSize,slotStride}` | 3 | `nix/modules/primitives.nix` | Port-demand and slot-window bounds |
| `closures`; `<name>.{package,executable,kind,requiresExecutable,operationBindings,effects}` | 7 | `nix/modules/primitives.nix` | Package authoring, native executable selection and narrowing/attestation rejection |
| `secrets`; `<name>.source`; `.source.{kind,envVar,path}` | 5 | `nix/modules/primitives.nix` | Source coherence, substitution scope and path confinement |
| `tasks`; `<name>.{kind,defaultOutput,operationId,serviceLifetime,invocation,steps,requires,exitPolicy,artifactRefs,logRefs,summaryRefs}`; Invocation expansion; `.steps.<name>.{task,dependsOn}`; `.exitPolicy.successCodes` | 22 | `nix/modules/primitives.nix` | Leaf/composite coherence, graph/reference checks, output selection, descriptive refs |
| `services`; `<name>.{lifecycle,endpoint,endpoints,primaryEndpoint,connectsTo,stateRefs,logRefs,containment}`; `.endpoint.{endpointId,host}`; `.endpoints.<name>.host`; lifecycle expansion below | 75 | `nix/modules/primitives.nix` | Endpoint shorthand/native apply, endpoint-less services, preparation, placeholders, reuse and descriptive refs |

Lifecycle mounts: `prepare` and `.task`; `start` and `.operationId`, Invocation
at `.invocation`, Terminal at `.terminal`; each of `ready`/`health` and
`.operationId`, Probe at `.probe`, Terminal at `.terminal`; `stop` and
`.operationId`, `.signal`, `.timeoutMs`, Terminal at `.terminal`; `clean` and
`.operationId`, Terminal at `.terminal`.

Reuse existing compiler negatives in `nix/gate-nix-negatives.nix`, model tests,
runtime admission/state/service/endpoint/output/lifecycle tests, example models,
and independent derivation vectors. Add RFC section 6 metadata, nested reuse,
default/apply/override, mutation, and poisoned-dependency cases. Existing option
snapshot freshness proves generator agreement, not explanation accuracy.

## Publication coverage

Every system-scoped set below exists for `aarch64-darwin`, `aarch64-linux`, and
`x86_64-linux`. Unit 1 replaces name/description duplication with actual
publication descriptors while retaining native implementations. The baseline
has five output namespaces: lib, packages, apps, checks, devShells.

| Complete identities | Form / scope | Existing owner and cutover |
| --- | --- | --- |
| `compileModel`, `projectApps`, `seq` | Function / library | `flake.nix::mkNixfiedLib`; native compiler, project-app builder, `nix/lib/compose.nix` |
| `synthetic`, `postgres`, `reth` | Module / adapter | Names in `nix/adapters/default.nix`; retain individual native modules |
| `pkgs`, `system`, `adapters`, `nixfiedLib` | Argument / module argument | `nix/compiler/resolve.nix` specialArgs; same lazy providers in declaration-only evaluation |
| `help`, `install`, `upgrade`, `gate`, `check`, `test`, `ci` | App / root | `flake.nix`; native help, install/upgrade, gate and dev programs |
| `help`, `run`, `model-check`, `ps`, `down`, `clean` | App / project | `nix/project-apps.nix`; replace reserved-name literals in compiler validation |
| New `docs` in root and project; new `docs` package | App / root and project; Package / root | One static reference builder and supplying-source identity per RFC section 8 |
| `default`, `toolchain-model`, `gate-runtime-model`, `nixfied-cli`, `nixfied-runtime`, `install`, `upgrade`, `gate`, `check`, `test`, `ci`, `minimal-model`, `postgres-model`, `composite-model`, `polyglot-stack-model`, `downstream-model`, `reth-model` | Package / root | All 17 native package bindings in `flake.nix`; retain builders |
| `minimal-model`, `derive-facts-vectors`, `nixfied-runtime`, `rust-workspace` | Package / check | Four native check derivations in `flake.nix`; lazy audit-check bindings |
| `default` | Package / devShell | Native pinned shell in `flake.nix`; retain fixture environment |
| Each exact key/description in `config.nixfied.surface.verbs` | Native config-derived app | Keep ordinary dynamic expansion in `nix/project-apps.nix`; no per-task descriptor registry |

`default` and `minimal-model` intentionally expose the same model under distinct
names. Nixpkgs-provided `lib`, `config`, `options`, and `_module` remain native
module facilities; explain them without creating extra Nixfied providers.
Debug runtime and test-child are private transitive inputs, not new exports.

Use baseline `nix/gate-nix.nix` root/project help, final merged app metadata,
context mismatch, no-state and downstream-adoption cases. Add actual-export
bypass negatives, scope identity, lazy provider/bootstrap, exact reference query,
source provenance, revision switching, and model/runtime independence cases.
An export audit checks final names against projections without becoming their
dependency or realizing the derivations it describes.

## Model and vocabulary coverage

Unit 2 uses the shared structure grammar for all **29** `primitive` identities
in the baseline [capability inventory](runtime/crates/nixfied-model/capability.txt):
all are Owned bindings with Serialize/Deserialize and RejectUnknown decoding.

| Complete record identities | Existing replacement sites |
| --- | --- |
| Model, Generator, Project, Target | `nix/compiler/derive.nix`; model `src/types.rs` |
| Codebase, SourcePolicy, StatePolicy, SecretDescriptor, SecretSource | Same producer and typed consumer |
| SlotPolicy, Placement, SlotPlacement, CandidatePortWindow | Same; retain native placement calculation |
| ClosureSpec, Invocation | Same; preserve Rust name InvocationSpec and native package/executable lowering |
| ServiceSpec, Lifecycle, PrepareSpec, StartSpec, ReadySpec, HealthSpec, StopSpec, CleanSpec | Same; native lifecycle and relational checks retained |
| Endpoint, ProbeSpec, TerminalSemantics, TaskSpec, StepSpec, ExitPolicy | Same; native endpoint, task/probe coherence and independent graph derivation retained |

Field membership comes from the authored inventory; baseline Rust serde and Nix
construction determine explicit per-field policies under RFC section 4.2. No
question-mark notation, omitted producer input, or decoder default is allowed to
silently choose a serialization policy. The RFC's null/absence/empty examples
are mandatory independent cases, not alternatives to whole-record coverage.

Unit 2 projects these 13 model enum inventories: ClosureKind, ClosureEffect,
StdinPolicy, ProbeKind, TaskKind, TaskDefaultOutput, ServiceLifetime,
SecretSourceKind, ContainmentRequirement, SourceMode, DirtyPolicy, CleanupPolicy,
PersistencePolicy; `signal` supplies StopSignal. Native identifier wrappers,
LoopbackHost, NonZero integers, UniqueVec and ordered maps retain their exact
construction/validation roles. Identifier wrappers separate namespaces; native
model validation owns their lexical checks.

Unit 2 also replaces the matching native types.enum membership lists below.
Paths are under nix/modules; option paths are relative to nixfied. The shared
invocation fragment mounts under tasks and service start/ready/health invocations.
Use inventory members directly, retaining native option defaults separately.
Unit 1 may retain these baseline lists; unit 2 removes them in the same cutover
as the corresponding Rust enum definitions.

| Inventory owner | Native file | Option / shared fragment |
| --- | --- | --- |
| StdinPolicy | `primitives.nix` | `invocation.stdin` |
| ProbeKind | `primitives.nix` | `services.<name>.lifecycle.{ready,health}.probe.kind` |
| signal (StopSignal) | `primitives.nix` | `services.<name>.lifecycle.stop.signal` |
| ContainmentRequirement | `primitives.nix` | `services.<name>.containment` |
| ClosureKind | `primitives.nix` | `closures.<name>.kind` |
| ClosureEffect | `primitives.nix` | `closures.<name>.effects element type` |
| SecretSourceKind | `primitives.nix` | `secrets.<name>.source.kind` |
| TaskKind | `primitives.nix` | `tasks.<name>.kind` |
| TaskDefaultOutput | `primitives.nix` | `tasks.<name>.defaultOutput` |
| ServiceLifetime | `primitives.nix` | `tasks.<name>.serviceLifetime` |
| SourceMode | `source.nix` | `codebases.main.sourceMode` |
| DirtyPolicy | `source.nix` | `codebases.main.dirtyPolicy` |
| CleanupPolicy | `state.nix` | `state.cleanupPolicy` |
| PersistencePolicy | `state.nix` | `state.persistence` |

Use inventory order for these native type projections. This deliberately changes
SourceMode's reference/type-description order to snapshot, flake-input,
live-workspace; its accepted values and live-workspace default stay unchanged.
Regenerate OPTIONS.md accordingly. Do not author a second member list just to
preserve documentation ordering. target.system is a separate authoring domain;
TaskDefaultOutput and RunOutputMode, and PersistencePolicy and ServiceLifetime,
remain distinct inventories despite shared spellings.

Add an independent fixture mutation of one shared inventory domain: a removed
member rejects and an added member accepts through the native Nix option and
Rust decoder, and the generated reference reflects that same change. Assert
literal expected membership independently; do not generate the oracle from the
mutated inventory. This fixture must not change the production capability bytes.

Unit 3 projects PortConflictReason, error-code, exit-class, and the five status
inventories RunStatus, ProcessStatus, RunLeaseStatus, PortStatus, CleanupStatus.
Emit status macro invocations in `registry/status.rs`; retain DbStatus, parsing
failures, classification sets, SQL and transitions. Unit 4 projects RunOutputMode
from run-output-mode; native classification methods remain native.

Use model `tests/model_contract.rs`, runtime `tests/capability_coverage.rs` and
`tests/admission.rs`, Nix `checks/derive-facts-vectors.nix`, and independent Rust
lowering/planning golden tests. Add separately authored raw JSON, producer-byte,
Rust serialize/decode, invalid-domain and false-derived-fact expectations.
Constructor/consumer agreement alone is not proof of preservation. Inventory
bytes and runtime ABI remain baseline unless a separate explicit amendment is
approved; inventory omissions do not justify silently extending it.

## Result and error coverage

Unit 3 covers **19 structured identities**: 15 inventory-backed records and
four local records. There are 13 Owned definitions, five Borrowed projections
and one MemberNamesOnly envelope; this is not a count of generated Rust structs.
Paths below are within `runtime/crates/nixfied-runtime/`. Replace structural
duplication while retaining native impls, data acquisition and lifecycle.

| Complete identities | Binding | Existing serializer/construction boundary | Baseline evidence to extend |
| --- | --- | --- | --- |
| Local CheckOutput, DownReport, CleanupOutcome | Owned | `src/main.rs`, `src/control.rs`, `src/state/cleanup.rs` | admission, registry, state tests; runtime gate |
| run-json, run-node, run-service | Owned | `src/main.rs::RunOutput/NodeResult/ServiceRunOutput` | output/lifecycle tests; runtime gate |
| run-summary-json | Borrowed | `src/main.rs::write_run_summary` | summary bytes, ordering, redaction and write failures |
| run-task, selected-endpoint | Owned | `src/service/task.rs::TaskRun`, `src/service/process.rs::SelectedEndpoint` | output/service tests; TaskRun decoder cases |
| ps-json, ps-process | Owned | `src/control.rs::PsReport/ProcessObservation` | registry/endpoint tests |
| runtime-error, runtime-error-cause | Owned | `src/error.rs::RuntimeError/RuntimeCause` | native error cause tests; display/source, omission/null, admission/output |
| runtime-error-projection | Borrowed | `src/output.rs::ReplayReport::into_error`, `src/main.rs::output_projection_io_error` | native output faults, lossy paths and output integration |
| runtime-error-port-conflict | MemberNamesOnly | Literal key at native `with_detail` call in `src/service/process.rs` | preserve serialization-failure fallback |
| port-conflict, port-conflict-endpoint | Borrowed | `src/service/process.rs::PortConflictDetails/PortConflictEndpoint` | endpoint tests, including lock/listener conflicts |
| nixfied-owner | Owned | `src/service/process.rs::NixfiedOwner` | nested presence/omission and owner proof |
| Local RegistryIdentityDiagnostic | Borrowed | `src/registry/schema.rs::registry_identity_json` | expected/found identity, negative observed slot, mismatch causes and human output |

TaskRun alone among these outputs keeps Deserialize, with IgnoreUnknown. Preserve
its explicit-null exitCode and native paths; no duplicate TaskRunView is needed.
RuntimeError retains Box<Vec<RuntimeCause>> through the bounded private storage
binding. Replace its thiserror derive with native Display/Error impls preserving
display and source() == None, and remove the now-unused direct dependency.
All other output records have NoDecoder and no Nix producer. Remove handwritten
data definitions or projected field lists in the same unit that adds their
generated replacement. Keep ProjectionIssue storage and lossy conversion native.

The uninventoried scalar leaves OutputStream and ProjectionOperation remain
native bindings, as do PathBuf and usize. Keep `to_value -> redact -> format ->
write` where present, explicit nulls versus omission, ordering, newlines,
conversion failures and intentional lossy path formatting. New independent
byte/omission/native-path/redaction expectations are required; the existing
suite does not exhaustively prove all output records.

Error per-member explanations/recovery links cover the inventory exactly.
Native constructors still choose ExitClass; `main.rs::exit_code` retains numeric
process exit mapping (success 0, existing error statuses 12–38). Its mapping is
native behavior, not generated per-error policy. Verify actual CLI status and
failure precedence as well as enum spelling. Open error details remain open;
declared structured diagnostics cannot hide inside an opaque schema escape.
RuntimeError.details is Required(OpenJson) plus Present: plain serde_json::Value
includes null. Preserve with_details(Value::Null) emitting a present null member;
cause projection still turns non-object details into an empty object. Nullable
wrapping of OpenJson rejects during declaration checking. For NoDecoder records,
rustEncode determines emitted member presence, so a Required list may OmitEmpty.

Fixed nested detail ownership is exhaustive for current production insertions:
taskRun uses run-task; projections entries use runtime-error-projection;
portConflict uses its declared endpoint/owner structure; expectedRegistryIdentity
and foundRegistryIdentity share RegistryIdentityDiagnostic. The latter's required
projectId, environment, slot, runtimeAbi and toolchainId fields are all Present;
slot is signed i64, the others Text. Preserve native RegistryIdentity, SQLite
reads, ordered mismatchedFields, mismatch classification, tolerant human display,
recovery guidance and cause filtering. Tests must assert all five fields, including
a negative observed slot, not merely that the values are objects.

Other scalar/path/list detail keys stay native, documented once in the error
topic by producer; this list is an audit, not an executable registry:

| Native producer | Other detail keys |
| --- | --- |
| `src/slot.rs` | slot, slotMin, slotMax |
| `src/main.rs` | command, compositeSteps, declaredTasks, environment, failedNodeId, failedService, logsDir, registryDir, registryPath, runDir, runId, runSummaryPath, slot, stateBase, stateRoot, stderrPath, stdoutPath, summaryPath, task, unknownTask |
| `src/error.rs` | unsupportedFeature |
| `src/service/process.rs` | address, endpointId, port |
| `src/registry/schema.rs` | mismatchedFields |
| `src/registry/sqlite.rs` | registryPath, registryDir |

Existing cause allowlist names endpoint and nixfiedOwner have no direct
production insertion; preserve the allowlist without inventing producers.
Safe causes must retain identity objects while excluding raw infrastructure
text. Preserve with_detail serialization fallback and cause_details returning
empty details for non-object input. Persistent TaskCommandRecord, process-start
identity and lifecycle/cleanup/upgrade event payloads retain native storage and
history ownership; no public-output schema machinery is added for them.

## Commands and native topics

| Complete command identities | Unit 4 consumer/replacement site | Independent evidence |
| --- | --- | --- |
| check, run, ps, down, clean | Runtime `main.rs` native parsing loops, option structs, token/default/help literals; `nix/project-apps.nix` argument tokens | `tests/command_help.rs`, output parser cases, both gates; add all RFC section 7 corner cases |
| install | CLI `main.rs::run/InstallOptions::parse/take_value` and native usage printers | Existing CLI inline tests and Nix adoption gate; add section 7 encoding/operand/precedence cases |
| upgrade | `nix/install/upgrade.nix` initial syntax/default/help definitions | Native Nix adoption/transaction tests and `nix/fixtures/upgrade-golden`; add section 7 operand/help/repetition cases |

The new docs dispatcher, contextual help and gate wrappers remain native apps
with complete interface topics; they do not enlarge command syntax machinery.
Upgrade/installer fixture oracles already committed in the baseline remain valid
evidence. No archived new fixture is an acceptable oracle.

Native renderHelp consumes checked syntax facts, with helpers iterating every
visible argument. Preserve RFC section 6.8's runtime rows and compact installer /
upgrade usage; no help-layout vocabulary or hand-maintained row list is added.
The shared help pair comes from surface-help. Default reference entries do not
automatically add default prose to baseline help. Native parser error construction
can consume inventory-derived spelling phrases without acquiring generated policy.

Unit 5 completes these native topics, with links available from earlier units.
Each row retains its implementation owner and requires behavioral proof, not
a new executable declaration family.

| Topic / complete responsibility | Native owners retained | Existing independent evidence |
| --- | --- | --- |
| Authoring algebra, tasks/services, inline invocations, static DAGs, adapters and limits | Modules, compiler, compose, adapters; CONTRACT/GUIDE/ADAPTERS | Compiler negatives, example/adopter gates, model/service tests |
| Derived operation IDs/bindings, service closure, ordering and terminals | Nix derive-facts/compiler; independent Rust lowering/planning; DERIVATION_SPEC | Nix and Rust golden vectors |
| Seam, raw model bytes/hash, ABI, target, closure and source admission | Compiler emission/spec; runtime loader and admission | model/admission/capability tests, immutable-source gate |
| State/slots, epochs, markers, placement, purge and cleanup | Runtime slot/state/control | state/upgrade/lifecycle tests and gate |
| Endpoints, primary selection, placeholders, endpoint-less restrictions, service identity/reuse | Compiler validation/derivation; runtime execution/service | Compiler negatives, endpoint/service tests, cross-root gate |
| Context and hermetic children | State placement, admission source/secrets, execution/service process | admission/state/service tests; new isolated precedence/encoding cases |
| Secret descriptors, file confinement, substitution and redaction | Admission secrets, redaction and native output paths | Compiler negatives, admission/output tests; baseline issue below tracked separately |
| Registry/event order, leases/liveness, cancellation and containment | Registry, service process, cancellation, reconciliation/control | registry/lifecycle/service/endpoint tests |
| Output modes/defaults, summaries, task-output replay, typed errors and failure precedence | Native main/output/error/task finalization | Output fault and integration tests; task-output gate |
| Install/upgrade reports, transactions/preflight and recovery | Native CLI and Nix installer/upgrade | CLI preservation/refusal tests, upgrade goldens and Nix gate |
| Discovery/help, fixed model.json and disposable views/docs.md | Native help and compiler emission/views | Final app metadata/help tests and model/adoption assertions |
| Developer tools, build/source isolation, verification and platform limits | DEVELOPMENT, native dev/gate/package/toolchain builders | Existing build checks; new source-variation matrix and freshness from first generated Rust consumer |

Context coverage includes state precedence NIXFIED_STATE_DIR/XDG_STATE_HOME/HOME,
secret-directory precedence NIXFIED_SECRETS_DIR/XDG_CONFIG_HOME/HOME, actual
secret material lookup, live versus immutable source roots, and optional host
ephemeral-port observations. Text inventory run-summary-text and
run-error-summary-text, and byte inventory run-task-output, bind to native
topics and behavioral evidence. Endpoint-acquisition/reuse, lease-authority,
escape-settlement and escaped-port-reconciliation inventory lines also remain
native guarantees, not structural records or generated transition machines.

No executable source-variation matrix was identified in the baseline; the prose
in DEVELOPMENT.md is not proof that one exists. Unit 2 adds the focused check
specified in RFC section 5.1; unit 4 extends it to CLI-generated files. Vary each
product's sources/generated files independently and check that only its filtered
source and product derivation identities change. Also vary declaration/reference
inputs with unchanged generated Rust bytes and require stable Rust product
identities. Keep shared toolchain/manifest/lock inputs fixed in those cases.
Wire the check into repository verification and update DEVELOPMENT.md to name
the implemented invocation. These are new implementation obligations, not passed
preparation checks.

## Baseline issue kept separate

Implementation baseline amendment (2026-09-21, after verified unit 4): preserving
the rejected value in a non-UTF-8 environment-secret diagnostic is explicitly
excluded from preservation. Secret admission must report unavailable/invalid
encoding without formatting the rejected value, before child execution or state
materialization. The regression in `tests/output.rs` exercises all four output
modes with synthetic bytes and a child sentinel; it failed against the reviewed
native implementation before the correction.

ABI disposition: this restores the existing REDACT-1 guarantee. Admission still
rejects the same input with `SECRET_UNAVAILABLE`, its existing exit class/status,
and the same error fields. Only the unsafe free-form message changes. No model,
capability inventory, numeric version, alias or compatibility reader changes.
The missing-variable diagnostic remains native and unchanged. This amendment is
separate from the structural and command migrations.

The baseline `admission/secrets.rs::read_env_secret` formats the error returned
by `std::env::var`. For a non-UTF-8 value, VarError::NotUnicode's display includes
the rejected value. This conflicts with the secret-handling invariant; it must
not be frozen as expected output by a preservation fixture. Owner: runtime
secret admission. Reproduction: use a synthetic non-UTF-8 environment secret,
then assert the failure contains no secret bytes or printable fragments.

Treat correction as a separate native fix with an explicit baseline amendment
and contract/ABI disposition before including it. Do not restore the abandoned
fix or conceal it inside this structural migration. This issue does not require
another metadata mechanism or change the settled architecture.

## Verification receipt and remaining proof

Preparation checks are recorded below; they establish the restored baseline
and document integrity only. All new implementation proofs above remain open.

- Restored production tree and index match the recorded baseline; no abandoned
  added paths or Cargo build tree remain in the checkout.
- `nix flake check --all-systems --no-build` passed for all three supported
  systems; this is evaluation, not cross-platform execution.
- `nix build .#checks.aarch64-linux.derive-facts-vectors
  .#checks.aarch64-linux.rust-workspace .#checks.aarch64-linux.minimal-model
  --no-link` passed, including generated option consistency, rustfmt and Clippy.
- A standalone probe under the pinned development toolchain confirmed that
  NotUnicode's display includes synthetic secret material. No real secret was
  used, and this was not an end-to-end runtime test.
- Document whitespace, local links, fenced examples, RFC sections/A1–A12,
  option-entry count and complete named capability coverage passed. A separate
  architect reviewed the coverage counts, scope choices, and readiness claims.

The subsequent design refinement used focused Rust, diagnostic and command /
reference architects plus independent counter-review. It resolved emission
bindings, diagnostic classification, native help formatting and presentation
context handling; these are design findings, not compiled implementation proof.
Local links, fences, section/checklist identities, the 19-record assignment and
git diff whitespace passed. All 142 tracked non-design files matched their
pre-review content hashes. Only the RFC and this handoff changed; no implementation
or CI run was part of this refinement.

Do not infer runtime, release, macOS execution, hosted CI or full integration
success from these checks. Implementation must run the focused proofs for each
unit and the final affected-product/cross-layer gate on its exact final tree,
as required by RFC A12 and DEVELOPMENT.md. No implementation acceptance item is
marked passed merely because preparation is complete.
