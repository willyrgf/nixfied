# Engineer handoff: shared contracts and adopter reference

Status: architecture settled; implementation starts from baseline. Read this
document, [the RFC](RFC_EXPOSE_ADOPTER_FACING_API.md), and the authority routed
by [AGENTS.md](AGENTS.md). The RFC owns the design and stable acceptance gate;
[FIXES_DOCS_REFS.md](FIXES_DOCS_REFS.md) owns the adopter problem. This is a
baseline coverage and delivery audit, never a source registry or generator input.

## Starting tree and recovery receipt

Baseline: `c3d9111c861d40b0885a49f3d62950f9871702e1`, branch `ex-adp-api`.
Recovery on 2026-09-21 restored every production source, test, fixture, generated
artifact, dependency, and normative document to that revision. Only this handoff,
the revised RFC, and the revised problem statement differ. Nothing is committed.

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

Unit 3 covers **18 structured identities**: 15 inventory-backed records and
three local records. Paths below are within `runtime/crates/nixfied-runtime/`.
Use serialization views where native objects own state; replace field-list
duplication, not native data acquisition or lifecycle.

| Complete identities | Existing serializer/construction boundary | Baseline evidence to extend |
| --- | --- | --- |
| Local CheckOutput, DownReport, CleanupOutcome | `src/main.rs`, `src/control.rs`, `src/state/cleanup.rs` | admission, registry, state tests; runtime gate |
| run-json, run-summary-json, run-node, run-service | `src/main.rs::RunOutput/write_run_summary/NodeResult/ServiceRunOutput` | output/lifecycle tests; runtime gate |
| run-task, selected-endpoint | `src/service/task.rs::TaskRun`, `src/service/process.rs::SelectedEndpoint` | output/service tests |
| ps-json, ps-process | `src/control.rs::PsReport/ProcessObservation` | registry/endpoint tests |
| runtime-error, runtime-error-cause | `src/error.rs::RuntimeError/RuntimeCause` | native error cause tests; admission/output tests |
| runtime-error-projection | `src/output.rs::ReplayReport::into_error`, `src/main.rs::output_projection_io_error` | native output fault tests; output integration tests |
| runtime-error-port-conflict, port-conflict, port-conflict-endpoint, nixfied-owner | `src/service/process.rs::port_conflict_error` and native diagnostic structs | endpoint tests, including lock/listener conflicts |

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
| Developer tools, build/source isolation, verification and platform limits | DEVELOPMENT, native dev/gate/package/toolchain builders | Existing source/build checks; freshness from first generated consumer |

Context coverage includes state precedence NIXFIED_STATE_DIR/XDG_STATE_HOME/HOME,
secret-directory precedence NIXFIED_SECRETS_DIR/XDG_CONFIG_HOME/HOME, actual
secret material lookup, live versus immutable source roots, and optional host
ephemeral-port observations. Text inventory run-summary-text and
run-error-summary-text, and byte inventory run-task-output, bind to native
topics and behavioral evidence. Endpoint-acquisition/reuse, lease-authority,
escape-settlement and escaped-port-reconciliation inventory lines also remain
native guarantees, not structural records or generated transition machines.

## Baseline issue kept separate

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

Do not infer runtime, release, macOS execution, hosted CI or full integration
success from these checks. Implementation must run the focused proofs for each
unit and the final affected-product/cross-layer gate on its exact final tree,
as required by RFC A12 and DEVELOPMENT.md. No implementation acceptance item is
marked passed merely because preparation is complete.
