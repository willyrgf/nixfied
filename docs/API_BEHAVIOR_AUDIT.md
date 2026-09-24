# API behavior audit

Research against commit `7f9d383df8419a7fa5770092a35de9ebbe7fd539`, 2026-09-24.
The pre-existing working-tree change to `PROBLEM_DUPLICATION_LAYERS.md` was preserved.
This report records current behavior and proposed follow-up; it changes no product contract or implementation.

The subsequent [compiler architecture and cleanup plan](API_COMPILER_CLEANUP_PLAN.md) turns the covered findings into an ordered implementation proposal. It replaces case-by-case shape rejection with native authoring alternatives and one complete configured-value evaluation boundary. It also selects a concrete cleanup correction: retain the shared terminal-label shape and make clean failure/success evidence follow the real outcome. The findings and original proof receipts below remain the audit baseline, not a claim that the proposal is implemented.

The audit found more than descriptive references. Some supplied configuration disappears before manifest emission; some policy names have no implementing behavior; some unused execution settings still change service reuse identity.

## Method and classification

Four parallel reviews covered core options/compiler, manifest/runtime, adapters/public library, and commands/install/upgrade. Coverage starts from the 128 option headings in `docs/OPTIONS.md` (including containers), the complete primitive and enum inventory in `capability.txt`, and public function/module/app/command declarations. Each candidate was traced through production consumers rather than classified from documentation or an underscore binding alone.

“No direct effect” needs several distinct classifications:

| Classification | Meaning |
| --- | --- |
| Discarded input | Accepted author input disappears; even the emitted manifest can be identical. |
| Descriptive/provenance only | Changes recorded bytes or presentation, but does not select execution policy. |
| Missing promised behavior | A policy/control name suggests behavior that no consumer implements. |
| Identity-only execution setting | Does not control execution as named, but still changes service reuse compatibility. |
| Conditional or validation-only | Has a real consumer, although it may not change an already-valid run in a particular context. |

Raw manifest hash changes are evidence changes, not proof that an option controls execution. Conversely, a reuse-identity change is operationally significant even when the underlying execution setting is unused. Display labels, output fields and admission assertions are not inherently defects merely because they do not launch or configure a child process.

## Already covered by existing features

Coverage is assessed against a concrete user need, not every capability a field name could suggest. A row marked covered means no new runtime feature is needed for that need; it does not mean the misleading declaration or missing validation is acceptable. Partial coverage means some of the same workflow exists but a material guarantee is missing. Not covered means there is no implementation of the specific requested guarantee; adjacent infrastructure is not counted as a substitute.

The recommendations below are design judgments based on the audit, not implemented changes or evidence of measured adopter demand. The original source traces and executed checks are preserved in the evidence appendix.

| Audited API / need | Existing owner that solves it | Recommended disposition |
| --- | --- | --- |
| Service/task `logRefs`: obtain runtime process logs | Runtime capture, redaction, run/log placement and returned evidence paths | Remove reference labels unless a concrete annotation consumer warrants them. Custom destinations/retention are a separate extension, discussed under partial coverage. |
| Leaf/composite `summaryRefs`: obtain execution summaries | Automatic task/run evidence and summaries; selected-task `defaultOutput` and command `--output` control presentation | Remove labels; reject unsupported composite fields. Named custom reports remain ordinary task outputs, not runtime summaries. |
| Composite `exitPolicy`: decide whether child exit codes count as success | Leaf `exitPolicy.successCodes` and propagation of leaf outcomes through the composite DAG | Reject a composite exit policy. An aggregate has no child exit code of its own. Conditional failure-tolerant orchestration is discussed separately below. |
| Invocation supplied to a TCP probe: execute a readiness/health command | Existing `probe.kind = "exec"` plus its invocation | Reject TCP plus invocation at Nix validation; use the existing alternative. |
| `primaryEndpoint` with singular endpoint: select the service's addressable endpoint | Singular `endpoint.endpointId` already identifies it; the multi-endpoint form supports explicit primary selection | Reject conflicting forms. For endpoint-less services, reject non-null primary selection because no endpoint can satisfy it. |
| Ready/health exec-probe invocation `timeoutMs`: limit an attempt | Outer `probe.timeoutMs`; retry interval and maximum attempts control retry policy | Remove the unused inner deadline and its contribution to identity in the same contract change. |
| Closure `kind`: distinguish the launched program from supporting tools | Ordered invocation `tools`, `run[0]` resolution, PATH construction and closure host admission | Remove the unused role discriminator. Preserve the actual executable and tool requirements. |
| `clean.terminal.failure`: report cleanup failure | Typed cleanup errors, command outcome and cleanup registry evidence | Remove the unused label if no event consumer needs it. Correct per-service success event ordering; this is an evidence defect, not a missing cleanup engine. Not every precondition refusal produces the same registry event. |
| Lifecycle terminal strings: determine success/failure | Native start/probe/stop/cleanup results and typed statuses | Outcome rules already exist. Custom strings are event labels only; keep them only for an identified event consumer, accounting for their identity effect. |
| Generator fields: decide runtime compatibility | Exact `runtimeAbi` and `toolchainId` admission | Keep generator data only for provenance. It should not become a second compatibility authority. |
| `sourceIdentity` in live mode: locate the source tree | Invocation root plus `logicalRoot`, canonical resolution and confinement checks | Do not treat the arbitrary live identity string as root selection or verification. Content identity remains a separate gap below. |
| `snapshot` versus `flake-input`: resolve immutable source | Shared immutable store-root admission using `sourceIdentity` | One runtime mechanism already serves both origins. Keep the distinction only if provenance is useful. |
| Task lifetime/output overrides, closure bindings/executable checks, fixed codebase/environment domains | Existing selected-root rules and admission validators, detailed in appendix section 7 | Keep real controls and describe their scope. No-op changes to an already-satisfied constraint are not evidence of a missing feature. |
| Project names, app descriptions, reference metadata and output fields | Existing presentation, discovery and evidence consumers | Keep useful metadata as metadata. These are not defective execution controls. |

## Partially covered needs

These entries identify an existing useful mechanism and the precise remaining gap. The gap is not automatically a backlog commitment. The importance arguments are prospective use cases, not claims that the audit found adopters depending on them.

### P1. State management and selectable storage lifetimes (`stateRefs`)

**Already present.** `${stateDir}` addresses the runtime-owned slot root. `nixfied.state` controls marker ownership, compatibility epoch, persistence and cleanup eligibility. Slots isolate concurrent project copies; child programs can choose service subdirectories. `down` and `clean` own process shutdown and safe state deletion. Ordinary slot-scoped service storage is fully covered.

**Missing.** Selecting session/run/slot roots by reference, with distinct deletion guarantees for each root. Runtime run directories exist for evidence, but they are not a public general-purpose storage-scope selector. Task `serviceLifetime` controls processes, not data lifetime; `state.persistence = "run-scoped"` does not promise automatic deletion when a run finishes.

**Why it could matter.** A test may need a fresh database on every invocation while a development database must survive runs. Shared slot data can unintentionally couple tests. Explicit data lifetimes would make freshness and retention requirements reviewable instead of relying on each child's directory conventions.

**Case against adding it now.** Slot isolation, service subdirectories and explicit cleanup already cover many projects. Automatic deletion must account for shared services, concurrent borrowers, crash recovery and retained debugging evidence. Naming a root `session` does not settle who can delete it or when. A new storage registry would add an authority the existing contract does not require.

**Recommendation.** Remove `stateRefs` as an operational-looking label. Keep current state primitives. Consider a new typed storage-lifetime design only after a concrete use case cannot safely compose those primitives. If pursued, Rust must own materialization and deletion; incompatible lifetime/ownership combinations must reject before creation or deletion, with concurrent-borrower and interrupted-cleanup proofs.

### P2. Reproducible source and live-content identity (`admissionFingerprintPolicy`)

**Already present.** Immutable source modes resolve declared Nix-store roots, and live mode confines execution to the selected source root. Manifest hashing records the exact declaration used for a run. These answer where source comes from and which declaration was admitted.

**Missing.** A fingerprint of the live source contents actually observed by a run, an expected-content comparison, and any guarantee that contents remain unchanged after admission. The raw manifest hash cannot provide those guarantees. An admission-time digest alone would still permit later edits; immutable source selection addresses a different, stronger execution model.

**Why it could matter.** Two runs of the same manifest against different local edits can produce different results. Source identity evidence could help reproduce a failure or explain why a standing service and a new task observe different source revisions. Recording a fingerprint and using one to gate service reuse are separate product decisions; one must not silently imply the other.

**Case against adding it now.** Release/CI workflows can use immutable source roots. Live fingerprints require an explicit file scope: untracked and ignored files, generated files, submodules, symlinks and potentially large trees. Inspection failures need defined outcomes, and computing a digest cannot freeze a mutable workspace. A policy string without these decisions provides false assurance.

**Recommendation.** Prefer immutable sources when reproducibility is required and remove the unused fingerprint selector. Reintroduce a typed facility only for an identified live-evidence or admission requirement. Nix can declare intent; Rust must own any runtime observation, rejection and evidence. Proof must distinguish changed contents, inaccessible inputs and post-admission mutation instead of only checking hash serialization.

### P3. Producing artifacts versus managing artifacts (`artifactRefs`)

**Already present.** Tasks can write reports, bundles or other outputs using ordinary invocation arguments and environment. Runtime placement creates an artifacts directory beneath the run root (`runtime/crates/nixfied-runtime/src/state/placement.rs:87`, `:125`). That directory's existence does not expose an artifact collection API or a public artifact-path template.

**Missing.** Declared artifact discovery, collection, output inventory, required-output validation, export and independent retention. Composite artifact labels do not aggregate children; leaf labels do not collect their files.

**Why it could matter.** CI callers need to find test reports or build bundles without guessing child-specific paths. A task exiting successfully while omitting its promised report can otherwise appear complete. A bounded output contract could make such workflows easier to integrate and diagnose.

**Case against adding it now.** Child tools already know how to produce their outputs, and projects or CI systems can own publication. A generic collector must define path confinement, symlinks, file sizes, missing outputs, partial results and secret handling. Runtime stdout/stderr redaction does not make arbitrary artifact files safe to publish. A storage/export service would substantially exceed what labels express; this is distinct from the explicitly excluded cache manager.

**Recommendation.** Remove `artifactRefs` unless a defined descriptive consumer exists. Keep project-owned artifact production. Consider a narrow local output inventory only when repeated integration needs justify it, separately from upload or retention. Any future collector must own output validation and fail explicitly on missing or unsafe required outputs after execution, without misreporting that as admission failure.

### P4. Phase deadlines versus total execution deadlines (start timeout and `run --timeout-ms`)

**Already present.** Leaf invocation timeouts bound finite commands; probe policies bound attempts; stop settings and CLI timeout inputs bound relevant shutdown/cleanup operations. Task service lifetime controls whether services stop with the run or remain available.

**Missing.** A single end-to-end startup/run deadline or a maximum age for a standing service. These are three different requirements. None is supplied by the unused start invocation timeout, and `run --timeout-ms` is not a total-run limit. Adding per-phase budgets does not establish a hard wall-clock bound over all phases and cleanup.

**Why it could matter.** A CI job may have a fixed total budget even when each individual task stays within its own timeout. A user may want startup to fail within a predictable period across preparation and multiple dependencies. Automatic expiry of standing services could matter on shared hosts, but requires an owner that remains able to enforce it.

**Case against adding it now.** Existing phase budgets and outer CI cancellation may suffice. A durable service should not expire merely because a generic invocation default was inherited. A total deadline must preserve containment, cleanup and completed evidence; it cannot promise that arbitrary OS operations finish at the exact deadline. Expiry after the runtime exits cannot rely on a timer in that exited process, and a required daemon is outside the current contract.

**Recommendation.** Remove the unused start timeout; retain the existing phase controls and clarify the CLI timeout. If a demonstrated need remains, design a distinct total-run/startup budget with Rust cancellation ownership and tests covering preparation, probes, tasks and cleanup. Do not turn an unused invocation field into an implicit service TTL.

### P5. Adapter host configuration

**Already present.** Endpoint declarations, host templates (`${host}` and `${host:<endpointId>}`), planning and listener-ownership checks support explicit hosts. The generic feature is complete.

**Missing.** Synthetic/Postgres/Reth adapter wiring from those declarations into all affected child listeners and probes. Their hardcoded addresses can disagree with the admitted endpoint.

**Why it matters.** An accepted host override can fail readiness or ownership checks even though the generic runtime supports it. The service and the runtime should agree on the endpoint without requiring users to override several separate invocations.

**Case against expanding scope.** No new address abstraction, listener policy or runtime branch is necessary. Nix adapters already own child arguments. Multi-listener programs may have their own limitations that must remain explicit.

**Recommendation.** Fix adapter propagation using existing templates. Preserve endpoint admission checks; reject adapter-specific unsupported combinations at Nix evaluation where knowable. Prove the same declared host reaches listener arguments and probe targets, then verify readiness/ownership on supported platforms.

### P6. Runtime logging versus custom destinations and retention (`logRefs`)

**Already present.** Runtime capture, redaction, deterministic evidence paths and summaries already solve ordinary debugging and automation needs. Removing descriptive refs would not remove those logs.

**Missing.** Arbitrary named sinks, independent per-log retention or size limits, and routing of child-managed files. These are possible interpretations of `logRefs`, not behavior that the current contract promises.

**Why it could matter.** Long-lived services can generate substantial output, while projects may need to preserve failure evidence longer than successful-run logs. Child programs also produce files outside captured stdout/stderr.

**Case against adding it now.** Capture and external log publication have different ownership, redaction and failure semantics. Routing to additional destinations expands the risk of losing or exposing evidence. Central log aggregation is explicitly outside the current product definition. The audit establishes no requirement for a named-sink registry.

**Recommendation.** Remove reference labels and retain native evidence capture. Evaluate bounded local log retention only from concrete capacity requirements, with the runtime as owner and no deletion of live or required evidence. Treat remote aggregation as a separate product decision, not completion of an unfinished `logRefs` implementation.

## Needs not covered at all

For these specific guarantees there is no equivalent current framework mechanism. Their absence does not, by itself, make them desirable additions. The first two are suggested by audited policy vocabulary; the third is a possible broader interpretation of the ignored composite exit policy, not a promise made by that field.

### N1. Inspecting and warning about live-workspace cleanliness (`dirtyPolicy = "warn"`)

**Missing guarantee.** Detect uncommitted changes and emit a warning, or inspect the workspace and admit a clean checkout under `reject`. Currently `warn` equals `allow`, while live `reject` refuses even a clean checkout because it cannot prove cleanliness. Immutable source admission is useful but is not a live-workspace inspection feature. Nix command warnings, if encountered, do not establish that the runtime inspected the source root it later executes.

**Why it could matter.** A warning can prevent someone from mistaking results from local edits for results from a committed revision. A clean-workspace requirement can support project release discipline. Neither cleanliness nor a commit ID proves reproducibility: ignored inputs, external tools and subsequent edits still matter.

**Case against adding it.** Local development normally requires modified files, so warnings can become routine noise. Non-Git workspaces and incomplete repositories need defined handling. A Git-only rule introduces source-tool semantics into a generic runtime without covering all mutable inputs. Projects can implement release preflight as an ordinary declared task, although that is not a global pre-spawn admission guarantee.

**Recommendation.** Remove the ineffective distinct warning choice unless users need it. Prefer immutable sources for strict reproducibility and project tasks for project-specific release checks. If framework admission must inspect cleanliness, define a typed supported policy, its source-tool requirements and unknown-state rejection at Rust admission; prove clean, dirty and uninspectable cases before any child starts.

### N2. General child access permissions (`process`, `source-read`, `file-write` effects)

**Missing guarantee.** Enforce which processes may be spawned, which files a child may read/write, or which network operations it may perform. Effect strings are attestations, not OS permissions. Process ownership/containment, hermetic environment, cwd confinement and endpoint checks enforce other guarantees; none is a general sandbox. In particular, confined cwd does not prevent a program from opening an absolute path elsewhere.

**Why it could matter.** Running untrusted scripts or restricting accidental writes would require real OS isolation. This becomes important if Nixfied is expected to execute third-party tasks with reduced authority, rather than trusted project programs with the invoking user's access.

**Case against adding it.** The current contract does not offer that trust boundary. Portable enforcement across Linux/macOS needs a supported isolation mechanism, a permissions model, host-capability admission and explicit handling of unsupported hosts. A few effect labels cannot represent paths, subprocess permissions or outbound network access. Per-tool effects are already listed as a product redefinition, not backlog work.

**Recommendation.** Remove unused effect distinctions if no attestation consumer justifies them; retain the real `network-listener` coherence check. Do not upgrade the wording to imply enforcement. If untrusted execution becomes a product goal, design and prove a separate isolation boundary that rejects unsupported protection before spawn. Nix build isolation does not extend automatically to runtime children.

### N3. Failure-tolerant or conditional composite orchestration

**Missing guarantee.** Author a policy to continue selected branches after failure, recover through fallback steps, or apply a new composite-wide success rule. Leaf success codes and a static dependency DAG do not expose those choices. The current root execution loop finalizes on a node execution error (`runtime/crates/nixfied-runtime/src/main.rs:845`). Applying a composite exit-code list cannot solve this because the composite is not a single child process.

**Why it could matter.** A test suite may want to execute all independent tests before reporting an aggregate failure; a release workflow may require explicit recovery steps. Those requirements should be stated as orchestration semantics, rather than inferred from a currently ignored field.

**Case against adding it.** Failure-tolerant scheduling changes dependencies, cancellation, evidence and the meaning of completion together. The current static algebra deliberately leaves conditionals and retries to Nix-side expansion or opaque child programs. Some needs can be handled by the test runner inside a leaf, preserving its aggregate exit result without adding a runtime scheduler policy.

**Recommendation.** Reject composite exit policies and use leaf-owned result rules for current workflows. Do not add conditional orchestration solely to make this field effective. A future demonstrated need would require a separate contract review, one runtime scheduling owner and tests for failure propagation, cancellation and retained evidence.

## Decisions and fixes that do not require a new capability

The remaining command findings are input/reporting issues rather than uncovered feature proposals:

| Finding | Decision |
| --- | --- |
| Root `.#check` silently ignores arguments | Add honest argument rejection/help or deliberately supported forwarding. The checking capability already exists. |
| Trailing operand-less `--state-base` selects fallback | Existing documented parser behavior. Decide explicitly whether to tighten rejection; no new placement feature is needed. |
| Installer metadata reported despite preserving existing module | Report which configuration was actually created versus preserved. Creating a project is supported; replacing project-owned metadata is not implied by installation. |
| `upgrade --no-lock` without URL | An explicitly reported no-op; no missing upgrade mechanism. |

Removal recommendations concern redundant or misleading behavioral surfaces, not all metadata. Keep useful human names, descriptions and provenance with their existing consumers. For partially covered and uncovered needs, require a concrete guarantee and one owner before introducing a replacement API. Contract and ABI changes remain atomic as described in the appendix; no proposal here changes runtime behavior.

## Evidence appendix

The original numbered findings below preserve conditions, source references, scope and verification results. “Section N” in this appendix refers to these numbered evidence sections; P/N identifiers above refer to the coverage decisions.

### 1. Inputs silently discarded by the Nix compiler

All paths below are under `nixfied`. Seven mutation checks compared the entire serialized manifest with a baseline and found identical bytes.

| API | Condition | What actually happens | Evidence |
| --- | --- | --- | --- |
| `tasks.<name>.exitPolicy.successCodes` | `kind = "composite"` | Supplied success codes disappear. They do not override child exit policies or composite outcome. | `nix/compiler/validate.nix:114`; `nix/compiler/derive.nix:360` |
| `tasks.<name>.artifactRefs` | Composite | Dropped before manifest emission. | `nix/compiler/derive.nix:360` |
| `tasks.<name>.logRefs` | Composite | Dropped before manifest emission. | Same |
| `tasks.<name>.summaryRefs` | Composite | Dropped before manifest emission. | Same |
| `services.<name>.lifecycle.{ready,health}.probe.invocation` and its child options | `probe.kind = "tcp"` | Replaced with null. Its command, environment, tools, source/cwd, stdin and timeout cannot influence the probe. | `nix/compiler/derive.nix:218`; `nix/compiler/validate.nix:193` |
| `services.<name>.primaryEndpoint` | Singular `endpoint` supplied | Replaced by `endpoint.endpointId`, even when the supplied primary names a nonexistent endpoint. | `nix/compiler/derive.nix:291` |
| `services.<name>.primaryEndpoint` | No endpoints | Forced to null, even when non-null was supplied. | `nix/compiler/derive.nix:327` |

The TCP mutation included an undeclared tool and forbidden `env.PATH`; the manifest was still identical. Invocation collection also excludes TCP payloads, so their package-shaped tools do not synthesize closures. The dynamic case exercised readiness; health uses the same compiler function and filtering.

These are owning-boundary gaps: Nix has enough information to reject incompatible combinations but instead erases them. Rust cannot reject information no longer present. Raw Rust manifest validation rejects composite leaf machinery and nonempty refs (`runtime/crates/nixfied-manifest/src/validation.rs:466`); runtime lowering rejects an invocation on a TCP probe (`runtime/crates/nixfied-runtime/src/execution/lower.rs:379`). Those protections do not repair the Nix omission.

Recommended follow-up: reject incompatible authored values at Nix validation, and make the option shape/defaults reflect the chosen alternative. Do not implement speculative composite exit semantics or a second probe execution path merely to give the accepted input meaning.

### 2. Five descriptive reference APIs

| API | Actual effect | Behavior it does not control |
| --- | --- | --- |
| `services.<name>.stateRefs` | Serialized labels and generated view | State roots, directories, persistence, cleanup, service reuse |
| `services.<name>.logRefs` | Serialized labels and generated view | Service log placement, capture or retention |
| `tasks.<name>.artifactRefs` on leaves | Serialized labels and generated view | Artifact creation, collection or placement |
| `tasks.<name>.logRefs` on leaves | Serialized labels and generated view | Task log placement or capture |
| `tasks.<name>.summaryRefs` on leaves | Serialized labels and generated view | Summary generation or placement |

Declarations: `nix/modules/primitives.nix:242`, `:255`, `:414`, `:419`, `:424`.
Emission: `nix/compiler/derive.nix:329`, `:388`.
Presentation: `nix/compiler/views.nix:29`, `:61`.
Explicit runtime discard: `runtime/crates/nixfied-runtime/src/execution/lower.rs:117`, `:262`.

These values change raw manifest bytes/hash but do not change lowered service reuse identity. Defaults such as `[ "slot" ]` and `[ "summary" ]` are labels, not registrations of supported resources. Arbitrary strings are accepted. The existing runtime test `descriptive_refs_change_manifest_bytes_but_not_service_reuse_identity` independently asserts the distinction.

Recommended follow-up: remove these fields if there is no identified descriptive consumer worth maintaining. A retention decision should call them annotations explicitly; turning them into storage/log/artifact managers would require a separate product design and contract change.

### 3. Source policy names without corresponding policy behavior

#### `codebases.main.admissionFingerprintPolicy`

This accepts arbitrary nonempty Nix strings. It is copied into admitted source evidence; it selects no fingerprint algorithm and computes or compares no source fingerprint. The Nix mutation emitted `"not-a-policy"` successfully.

Evidence: declaration `nix/modules/source.nix:34`; copy `runtime/crates/nixfied-runtime/src/admission/source.rs:52`; structural source validation `runtime/crates/nixfied-manifest/src/validation.rs:132`. The reference description at `nix/meta/manifest.nix:197` says “native validation enforces the supported choice,” which is not implemented. Nix's nonempty constraint is not validation of a supported policy. Full runtime admission also accepted an empty raw-wire fingerprint string; this is stricter in Nix than in Rust, but neither implements fingerprint policy selection.

Recommended follow-up: remove the unsupported policy surface or specify and implement a concrete fingerprint requirement atomically. Merely changing the description would leave an operationally named string with no policy owner.

#### `codebases.main.dirtyPolicy = "warn"`

For live workspaces, `Allow | Warn => {}` is one empty runtime branch. There is no cleanliness inspection or warning. `reject` is different: Nix rejects it for live workspaces, and raw runtime admission rejects because cleanliness cannot be proven. It does not inspect Git and accept a clean checkout.

Evidence: `nix/compiler/validate.nix:16`, `:307`; `runtime/crates/nixfied-runtime/src/admission/source.rs:28`.

For immutable sources, all dirty-policy values use the same immutable store-root admission; immutable resolution supplies the actual guarantee. Changing policy bytes still changes provenance.

Recommended follow-up: either provide a defined warning with an owning source-observation boundary, or remove the distinct `warn` choice. Treat live `reject` as an unsupported combination, not an implemented dirty-check mode.

#### Source distinctions that are provenance only in particular contexts

| API / distinction | Context and actual behavior |
| --- | --- |
| `codebases.main.sourceIdentity` | In `live-workspace`, root resolution uses invocation root plus `logicalRoot`; the supplied identity string is retained in source evidence and does not choose or verify the root. In immutable modes it is operational. |
| `sourceMode = "snapshot"` versus `"flake-input"` | Both use exactly the same immutable-root resolver. The different enum value records provenance; it does not choose a different runtime source mechanism. Live versus immutable remains a real behavioral distinction. |

Both are visible in `runtime/crates/nixfied-runtime/src/admission/source.rs:28`. Nix checks immutable source identities for store-path form but does not require live identities to equal the documented `"live"` marker (`nix/compiler/validate.nix:10`). These are conditional/provenance cases, not evidence that source admission as a whole is inert.

### 4. Unused invocation deadlines that still affect service reuse

| API | Actual timing owner |
| --- | --- |
| `services.<name>.lifecycle.start.invocation.timeoutMs` | No start-invocation duration limit consumes this field. The service is long-lived; readiness and shutdown have separate controls. |
| `services.<name>.lifecycle.ready.probe.invocation.timeoutMs` with `kind = "exec"` | The enclosing `ready.probe.timeoutMs` owns each attempt's deadline. |
| `services.<name>.lifecycle.health.probe.invocation.timeoutMs` with `kind = "exec"` | The enclosing `health.probe.timeoutMs` owns each attempt's deadline. |

The shared authoring fragment describes `timeoutMs` as a maximum invocation duration (`nix/modules/invocation.nix:37`). Runtime resolution drops it (`runtime/crates/nixfied-runtime/src/execution/lower.rs:444`); task lowering separately reads it (`:320`), and probe lowering uses the outer timeout (`:366`). `StartOp` has no timeout field (`runtime/crates/nixfied-runtime/src/execution/types.rs:165`). The outer probe option does document this distinction (`nix/modules/primitives.nix:83`). For TCP probes, the entire supplied invocation is instead omitted by Nix as described in section 1, so it cannot change manifest bytes or identity.

However, service identity hashes the entire serialized lifecycle (`runtime/crates/nixfied-runtime/src/service/identity.rs:43`). These three unused timing fields therefore change runtime compatibility and can prevent reuse of an otherwise behaviorally identical service. They must not be described as entirely effect-free.

Recommended follow-up: remove timeout from invocation positions where it has no meaning, retaining task duration and probe-attempt limits with their actual owners. Keep identity inputs aligned with the resulting execution contract.

### 5. Closure classifications with no execution enforcement

| API | Actual behavior |
| --- | --- |
| `closures.<name>.kind = "executable"` versus `"helper"` | Enum/type and serialized role metadata. It does not choose dispatch, PATH inclusion, executable checking or sandboxing. |
| `closures.<name>.effects` member `"process"` | Attestation only; does not gate spawning. |
| Same, `"source-read"` | Attestation only; does not grant or restrict source reads. |
| Same, `"file-write"` | Attestation only; does not grant or restrict filesystem writes. |

Declarations: `nix/modules/primitives.nix:277`, `:298`. Closure host admission checks paths, target and executable bits, without branching on kind or these effects (`runtime/crates/nixfied-runtime/src/admission/closures.rs:10`). Invocation resolution dispatches by declared executable/tool order, not kind (`runtime/crates/nixfied-runtime/src/execution/lower.rs:450`).

`"network-listener"` is the exception: Nix and Rust require it on the selected start closure when a service declares endpoints, and reject it on an endpoint-less start closure (`nix/compiler/derive.nix:303`; `runtime/crates/nixfied-runtime/src/execution/lower.rs:160`). This is an admission attestation, not an OS network permission mechanism. Effects on unrelated task/probe/tool positions do not become general sandbox permissions.

Changing closure kind or the three descriptive effect members changes manifest bytes, but not service reuse identity. The identity input is the service lifecycle, endpoints, containment, wiring, state and target; it does not include the closure record's role/effect labels.

Recommended follow-up: remove unconsumed role/effect distinctions unless their attestation value has an explicit consumer. Do not describe these declarations as capability enforcement. The contract currently calls effects attestations, so the lack of a sandbox is not itself a violated sandbox promise.

### 6. Intentional presentation and evidence APIs

These belong in the inventory of APIs without direct child-execution effects. Most have intentional presentation/evidence consumers; the clean-failure exception follows the table:

| Surface | Consumer / effect |
| --- | --- |
| `nixfied.project.name` | Human-readable generated manifest view heading; distinct from operational `projectId`. `nix/compiler/views.nix:96`. |
| `nixfied.surface.verbs.<task>` values | Flake app descriptions/help. Keys publish executable task apps. Values stay outside the manifest. `nix/project-apps.nix:50`. |
| Manifest `generator.name`, `.version`, `.emitter` | Nonempty provenance strings serialized into registry run evidence; they are not compatibility gates. `runtime/crates/nixfied-manifest/src/validation.rs:72`; `runtime/crates/nixfied-runtime/src/admission/mod.rs:57`; `service/registry.rs:279`. Exact ABI/toolchain fields are the real compatibility gates. |
| Lifecycle `terminal.success` / `.failure` on start, ready, health, stop; `clean.terminal.success` | Labels recorded in lifecycle events. They do not redefine what counts as success/failure. They also enter lifecycle reuse identity, so they have an indirect operational effect. `runtime/crates/nixfied-runtime/src/service/process.rs:2573`, `:2597`; `service/identity.rs:43`. |
| Publication descriptions, option documentation/default text, topic associations and reference usage/effect descriptions | Documentation/help/discovery consumers; not execution permission enforcement. `nix/meta/publications.nix:152`; `nix/docs/reference.nix`; `nix/docs/topics.nix`. |
| Public result/error/provenance fields | Runtime evidence/output projections, rather than accepted configuration switches. Reviewed as output consumers, not counted as dead controls. |

**Additional unused field: `services.<name>.lifecycle.clean.terminal.failure`.** No failure-event path consumes this label. `run_slot_clean` first records each service's clean start and success, then performs aggregate marker-owned slot cleanup (`runtime/crates/nixfied-runtime/src/service/process.rs:2023`, `:2043`). A later cleanup refusal does not use the configured service failure token. The token still changes manifest bytes and lifecycle reuse identity. This is also an evidence-ordering concern: a service clean success event precedes the actual slot cleanup result. Follow-up should define the relationship between per-service events and aggregate cleanup, with a failure/refusal test, rather than merely finding a place to print the unused token.

### 7. Real APIs whose effects are conditional or validation-only

These exclusions prevent overcounting every unchanged valid run as a no-op:

| API | Real owner and limitation |
| --- | --- |
| `closures.<name>.operationBindings` | An authored narrowing gate. Nix rejects undeclared or insufficient allowed operations, but emits independently derived bindings rather than the supplied list. Widening a sufficient gate can leave manifest bytes unchanged. `nix/compiler/derive.nix:190`. |
| `closures.<name>.requiresExecutable` | Controls executable-bit checking for otherwise unused closures. Any closure listed in invocation tools is checked regardless of `false`. `runtime/crates/nixfied-runtime/src/admission/closures.rs:67`. |
| Invocation `codebaseId` | Currently only `main` is supported; it is an admission constraint, not an implemented multiple-source selector. Structural and relational validators enforce it. |
| Task `serviceLifetime` | The directly selected root sets the lifetime of the full required service union. Nested and prepare task declarations do not independently choose lifetime for that run. `runtime/crates/nixfied-runtime/src/execution/plan.rs:205`; independent test at `:718`. |
| Task `defaultOutput` | Only the directly selected task supplies the fallback; explicit command output wins. Nested task defaults do not independently project output. `runtime/crates/nixfied-runtime/src/main.rs:1297`. |
| `state.persistence = "run-scoped"` | Real cleanup eligibility/compatibility policy, not an instruction to delete all state automatically at run completion. `runtime/crates/nixfied-runtime/src/state/cleanup.rs:261`; `state/marker.rs:136`. |
| State marker identity, epoch and cleanup policy | Actual ownership, compatibility and deletion gates. The fact that unchanged compatible state needs no action does not make these metadata-only. |
| Derived `servicesRequired`, `operationBindings`, invocation `executable`, duplicated target/placement identity facts | Independently re-derived or checked at admission. An executor need not read the original carried field for the field to have a rejection effect. |
| Fixed manifest `environments = ["dev"]` | A closed namespace contract; unsupported alternatives reject. Not an exposed environment-membership selector. `runtime/crates/nixfied-manifest/src/validation.rs:153`. |

### 8. Commands and adapters: related surprises

No entirely inert **declared** runtime/install/upgrade flag was located. The following contextual cases remain relevant:

| Surface | Finding | Evidence |
| --- | --- | --- |
| Framework root `nix run .#check -- <args>` | Drops all arguments, including `--help`; runs the framework checks. This is an unsupported-input rejection gap, not a declared no-op flag. Generated adopter task apps do forward arguments. | `nix/dev.nix:41`; `nix/project-apps.nix` |
| Runtime trailing `--state-base` without operand | Becomes absence and falls back to environment/default; a trailing repetition can erase an earlier explicit base. This is explicitly documented parser behavior, not an inert valid path. | `runtime/crates/nixfied-runtime/src/main.rs:1433`, `:1493`, `:1553`, `:1573`; `docs/CONTRACT.md` native parsing section |
| `install --project-id` / `--name` with existing `nixfied.nix` and no `flake.nix` | Preserves existing module, so requested values do not reconfigure it, although success output reports them. Preservation is intentional; reporting can mislead. | `runtime/crates/nixfied-cli/src/main.rs:135`, `:159`, `:363` |
| `run --timeout-ms` | Bounds cleanup/termination operations, not total run duration, leaf execution or readiness. Leaf invocation and probe policies own those deadlines. | `nix/meta/commands.nix:131`; runtime `main.rs:655`, `:783`, `:801`; `service/task.rs:199`; `service/process.rs:849` |
| `upgrade --no-lock` without URL | Explicit no-op with a message; with URL it performs the documented mechanical update and skips lock-based verification. | `nix/install/upgrade.nix:334` |
| Adapter endpoint host overrides | Synthetic/Postgres children and probes hardcode `127.0.0.1`; Reth defaults its wrapper to that host and does not pass its supported `--host`. Changing the declared host affects runtime planning/ownership but does not reconfigure these children. Can cause failure; not a globally inert endpoint option. | `nix/adapters/synthetic.nix:90`, `:111`; `postgres.nix:92`, `:171`, `:221`; `reth.nix:35`, `:108`, `:139` |

All adapters repeat the descriptive refs from section 2; they add no distinct operational meaning to them. No ignored parameter was located in the exported `compileManifest`, `seq` or `projectApps` functions. `seq` derives dependencies and rejects duplicate task IDs. Provider arguments and publication bindings reach their native implementations.

### 9. Coverage and follow-up order

| Reviewed family | Disposition |
| --- | --- |
| Project, target, source, state, slot/port options | Findings and contextual distinctions in sections 3, 6, 7; other fields have admission, namespace, placement or cleanup consumers. |
| Closure package/executable/role/effects/bindings | Role and effect labels in section 5; remaining fields participate in builds, dispatch or admission. |
| Service endpoint forms, dependencies, containment, lifecycle | Compiler omissions and unused deadlines in sections 1 and 4; metadata in sections 2 and 6; remaining controls consumed. |
| Task kinds, invocations, exit policies, dependency DAGs, lifetime/output | Composite omissions in section 1; refs in section 2; selected-root semantics in section 7; remaining controls consumed. |
| Secrets and invocation env/argv/tools/cwd/stdin | Runtime resolution, template, confinement and execution consumers found. No additional inert knob located. TCP invocation omission covers all nested invocation options in that incompatible context. |
| Manifest records/enums and derived facts | Full primitive inventory traced; provenance/attestation/fixed-domain cases separated from independently checked admission facts. |
| Public library/modules/providers and adapters | No additional unused exported function argument; repeated refs and hardcoded adapter hosts noted. |
| Generated project apps, runtime controls, install/upgrade, help/docs, developer gates | Declared flags have consumers; contextual behavior and ignored root-check arguments in section 8. |

Suggested sequence for a subsequent implementation task:

1. Reject silently erased incompatible Nix input: composite fields, TCP invocation payloads, and inappropriate primary endpoint declarations.
2. Resolve unsupported policy promises: fingerprint policy and the distinct dirty-warning mode.
3. Remove unused deadlines from the relevant invocation shapes and their identity inputs in one contract cutover.
4. Decide whether to delete descriptive refs and unconsumed closure classifications. Preserve intentional human names, descriptions and provenance where useful.
5. Clarify conditional controls and fix adapter host propagation and installer reporting separately.

For each cutover, the invariant should be: an accepted behavioral declaration either reaches its named owner or is rejected before effects. Tests should mutate one input and assert the promised execution, rejection, evidence or identity effect. Wire/semantic removals require capability inventory, ABI snapshot, Nix/Rust producers/consumers, fixtures and docs to change together; use the contract and development guide's required cross-layer gate. This report does not authorize replacing historical append-only records.

### 10. Verification receipt

The execution checks below belong to the original audit. The later coverage classification is a documentation-only revision: source/contract review, heading and whitespace checks were performed; runtime tests were not rerun and no new capability was implemented.

- Nix mutation evaluation: seven whole-manifest equality comparisons returned true; arbitrary fingerprint policy string was emitted. Executed with `nix eval --impure --json --file /tmp/nixfied-options-audit.nix`.
- `nix run .#test -- descriptive_refs_change_manifest_bytes_but_not_service_reuse_identity`: passed, one focused test.
- `nix run .#test -- exec_probe_uses_its_attempt_deadline_instead_of_authored_invocation_timeout`: passed, one service behavior test.
- External Rust mutation/admission harness: 17 cases passed (eight lowered-program/identity comparisons and nine full host admissions). Invocation start timeout, closure kind, nonlistener effects, fingerprint policy, live source identity, allow/warn, project name and generator mutations preserved the tested executable program projection; only the start timeout changed compatibility identity. Admissions accepted arbitrary and empty raw fingerprint strings, arbitrary live identity, helper executables and effect substitutions, and resolved snapshot/flake-input to the same root. Admissions used temporary fake-store executable files and spawned no children.
- Harness command: `nix develop --command cargo run --offline --manifest-path /tmp/nixfied-audit-3trz0t4b/Cargo.toml --target-dir /tmp/nixfied-audit-3trz0t4b/target`. Source: `/tmp/nixfied-audit-3trz0t4b/src/main.rs`. Scratch proof files are session-local, not maintained regression tests.

Source tracing covers more cases than the focused dynamic proofs. No full `.#ci`, platform matrix, release build or adapter end-to-end gate was run for this research-only report. In particular, the adapter host observation is source-confirmed rather than a newly executed integration reproduction. Absence of a consumer is supported by inventory review and traces; this is not a formal proof over arbitrary adopter Nix modules or child programs that independently inspect manifest bytes.
