# Redundant API removal and compiler boundary plan

Status: proposed implementation plan, following an architecture review against commit `7f9d383df8419a7fa5770092a35de9ebbe7fd539`.

This plan implements the cleanup identified in [the API behavior audit](API_BEHAVIOR_AUDIT.md). It does not add one-off predicates for each historically ignored option. The architectural change is to express alternatives through native Nix module types and complete configured-value evaluation before compiler projection. No production changes are made by this document.

## Architectural decision

Keep Nixpkgs as the owner of option declaration, merging, defaults and type errors. Use its existing `types.attrTag` for mutually exclusive authoring alternatives. At executable compilation, force the complete configured `nixfied` value through its ordinary JSON-compatible representation before relational validation and lowering. Keep graph/reference checks in the existing native compiler and host checks in Rust.

Do not create a second schema language, allowed/forbidden-property registry, “consumed field” ledger, validation interpreter, compatibility reader or required artifact. Remove invalid combinations from the option shapes; native unknown-option and missing-value errors handle them. A relational rule is still necessary when validity depends on another value, such as endpoint membership or a graph cycle.

The architecture reviewer inspected the actual pinned Nixpkgs implementation and exercised tagged alternatives, native merging, documentation discovery, package opacity and invalid inputs in a scratch evaluation. A separate scope review traced removal participants and cleanup evidence behavior. Recommendations below distinguish these observed mechanics from implementation work still required.

## Invariant, ownership and proof

**Invariant:** whenever compilation produces an executable manifest derivation, every configured Nixfied value has passed native evaluation/type checking, each selected alternative contains only its own fields, and compiler-owned relational guarantees hold before manifest emission. Projection cannot silently erase inapplicable authored payloads. Accepted program behavior and evidence retain their existing owners. Metadata-only reference queries remain independent of configured project values.

**Made unrepresentable at the authoring boundary:** leaf/composite payload mixtures, TCP probes with exec payloads, endpoint-less topology with primary selection, and secret resolver payload mixtures. Removed properties no longer have declarations. They are not accepted and then ignored, including when their supplied value equals an old default.

**Owners and rejection boundaries:** native option types reject shape/value errors; the compiler's configured-value boundary demands those checks; `validate.nix` owns cross-field and graph relations; derivation resolves and emits proven alternatives; wire constructors check emitted record structure; Rust independently validates untrusted manifest bytes and admits host facts before spawning children. A valid Nix value is not a substitute for Rust admission.

**Proof:** whole-compilation negative vectors, a synthetic added variant/field exercise, module-merge and defaults positives, poisoned-package/reference laziness checks, independent wire/admission tests, and behavioral state/log/summary/deadline/cleanup tests. Structural presence is not proof that a setting affects execution; behavioral tests remain necessary.

## What the current compiler lacks

| Current mechanism | What it proves | Gap found |
| --- | --- | --- |
| `nix/compiler/resolve.nix` calls `lib.evalModules` | Native option declarations, merging and checks when values are demanded | It returns a lazy configuration. Merely obtaining the result is not a complete configured-value validation barrier. |
| `nix/modules/primitives.nix` combines `kind` with nullable/defaulted fields | Each individual property can have a valid type | Flat product records admit values belonging to incompatible alternatives. Defaults obscure absence, and lowering must remember which fields to ignore. |
| `nix/compiler/validate.nix` forces a list of native semantic checks | The relationships those checks actually inspect | It is not a complete traversal of configured values. Current leaf/composite coherence checks enumerate selected fields and omit others. The special forcing of verb descriptions illustrates the incomplete boundary. |
| `nix/compiler/derive.nix` projects authoring data | Graph facts, executable resolution and manifest construction | Projection can erase an invalid or irrelevant payload before anything examines it: composite fields, TCP invocation and singular/absent primary selection. |
| `nix/meta/structure.nix` constructors | Known, required, well-typed emitted fields | They see the projected record, not all authored input. They cannot recover a value already dropped by derivation. |
| `nix/meta/options.nix` / static reference checks | Complete documented option metadata | Intentionally inspect declarations without forcing configured values/defaults. Making this layer strict would break its job. |
| Rust structural validation and lowering | Independent current-wire and host/runtime guarantees | Cannot diagnose invalid authoring that Nix erased before emission. Must remain as protection against directly supplied manifests. |

Evidence: `nix/compiler/validate.nix` (`surfaceDescriptionsForced`, `leavesCoherent`, `compositesCoherent`), `nix/compiler/derive.nix` (`probeOf`, `serviceSpec`, `taskSpec`), `nix/meta/structure.nix` (`construct`), and the audit's seven whole-manifest equality comparisons.

Two gaps require two complementary corrections. Forcing alone does not reject a well-typed but inapplicable property. Better alternatives alone do not ensure unused configured values are ever demanded.

## General compilation correction

### 1. Establish one configured-value evaluation barrier

Introduce the barrier at the existing configured compilation/validation entry, before relational checks or lossy projections. Keep `resolve.nix` usable for lazy declaration discovery. Conceptually:

```nix
checkedConfig =
  builtins.seq (builtins.toJSON evaluated.config.nixfied)
    (validate evaluated.config);
```

The serialized value is discarded. Return and lower the original checked config, retaining Nix string context and package values. Do not persist this serialization, parse it back, compute a second manifest, or expose a new public API. Existing manifest emission remains the semantic seam.

This is suitable for the current concrete option domain: scalars, paths, collections, submodule records and package values with native store-path coercion. It is not a promise to accept arbitrary future functions as configuration. New opaque option domains require an explicit review of their evaluation boundary. Wrong types fail at their native options, rather than being silently dropped because a later projection did not read them.

Do not `deepSeq` the whole module result, provider set or derivation attributes. That can force irrelevant package `passthru`/implementation details and destroy intended laziness. The architecture spike used a derivation-shaped package with throwing `passthru` to verify native JSON coercion avoids it. Implementation must add a real package/string-context fixture as well; the spike is not a complete performance or package-compatibility proof.

Do not equate “no explicit default” with “required.” Some native types, notably `listOf`, supply an implicit empty value. Choose types/defaults according to the invariant: required scalar/submodule payloads, nonempty lists where required, and explicit defaults where absence has meaning. Tests must cover implicit empty values as well as missing scalar properties.

Apply the barrier to executable manifest compilation, including all declared components even when a selected task would not use them. Inspect public `compileManifest` and runtime app program construction for bypasses. `projectApps` currently uses raw resolved config for app discovery and separately compiles the manifest; its comment that validation already proved the raw config is too broad. Every runtime executable wrapper must depend on the checked manifest. App name/description discovery and packaged help/docs must not force unrelated project execution values merely to construct the app catalog. No global strict evaluation of `authoring.evaluated` or `projectApps` is permitted.

After the barrier is in place, remove isolated forcing workarounds such as `surfaceDescriptionsForced` where they become redundant. Keep semantic checks for exported-task existence and reserved names.

### 2. Express exclusive shapes with native tagged alternatives

Use the pinned Nixpkgs `types.attrTag` with ordinary option/submodule payloads. It selects exactly one named alternative and supports native merging and `getSubOptions`; it is not a project-specific schema interpreter. Use shared ordinary option fragments for facts common to alternatives.

Adopt these final authoring shapes. Commit 2 changes the alternatives together; commit 3 moves deadline ownership together with its wire representation:

| Family | New authoring shape | Removed representational problem |
| --- | --- | --- |
| Tasks | `tasks.<id>.leaf = { invocation; timeoutMs; exitPolicy; requires; operationId; defaultOutput; serviceLifetime; };` or `.composite = { steps; serviceLifetime; };` | No leaf-only option exists in the composite branch; no top-level `kind` plus unrelated nullable payload. Common lifetime declarations are reused through one fragment. Composite wire output remains summary. |
| Probes | `lifecycle.<ready-or-health>.probe.tcp = { timeoutMs; retryIntervalMs; maxAttempts; };` or `.exec = { invocation; timeoutMs; retryIntervalMs; maxAttempts; };` | TCP has no invocation member. Shared probe timing comes from one ordinary option fragment. |
| Service endpoint topology | `services.<id>.topology.none = { };` or `.listening = { endpoints; primaryEndpoint; };` | No singular endpoint sugar and no primary selection on the no-endpoint alternative. Listening requires primary selection. |
| Secret resolvers | `secrets.<id>.source."env-var" = { envVar; };` or `.file = { path; };` | No coexistence of `kind`, unused `path` and unused `envVar`. Applies the same representation rule to an already-validated family rather than making the solution audit-specific. |

These sketches show field ownership, not that every displayed field must be supplied. Preserve meaningful existing defaults inside selected branches. Require an explicit task branch. A default probe may still select `{ tcp = { }; }`; that default must yield correctly to an explicitly selected exec branch under native module priority rules. Endpoint-less services still require exec probes; this is a real service/probe relationship, not something the local probe type can infer.

For topology, the architect considered a flat canonical endpoint map plus nullable primary selection. Select the native `none | listening` alternative instead: it costs more authoring migration but eliminates the correlated absence state using the same mechanism as other families. Retain the genuine relation that a listening primary must be a key in its endpoint map. An empty listening map cannot satisfy that relation. Do not assume wrapping `attrsOf` in a predicate automatically removes its implicit empty value.

The wire does not need to mirror these Nix authoring shapes. Continue emitting the established flat tagged task/probe/secret records and endpoint map/optional primary, except for the separately planned field removals. No new semantic task/service kind or general wire union generator is needed. Keep Rust's independent kind/payload validation.

Do not support both old and new authored forms. Delete old declarations and migrate adapters, examples, installer templates, gate modules, helpers and tests together. No aliases, null/default-based compatibility detection or field-name rejection inventory.

### 3. Keep relational validation explicit and cohesive

Native types cannot establish that a string references another declaration, that a graph is acyclic, or that endpoint demand fits a slot. Keep these checks in the existing compiler owner. Checks that need resolved invocations or derived graph facts can remain at the corresponding derivation boundary; this plan does not assert exhaustive parity with every Rust static check or require moving all checks into one file. In particular:

- Prove `listening.primaryEndpoint` belongs to the declared endpoint map. The audit found no complete Nix membership check; requiring a non-null name is insufficient.
- Preserve endpoint-less/exec-probe and listener-attestation coherence.
- Preserve service/task references, sibling dependencies, cycles, operation uniqueness/bindings, template scope, secret path/reference rules, target and source constraints, and port capacity/range checks.
- Delete shape-coherence checks that native alternatives now establish, including manual secret payload selection and the list of forbidden composite fields. Do not replace them with a longer list.

Update existing validators and derivation helpers to read the selected branch directly. Normalize only where it removes repeated interpretation; do not maintain a raw config plus a second mutable/independently validated model. Derivation must lower the selected branch, not manufacture compatibility defaults for obsolete input.

A generic compiler cannot prove that arbitrary well-typed data has an observable behavioral consumer. Do not claim this change would automatically detect another inert `stateRefs`-like field introduced later. Explicit lowering, code review and independent mutation/behavior tests remain the proof for consumption. Metadata and validation-only inputs can legitimately leave a successful execution unchanged.

## Redundant API removal scope

| API | Planned action | Guarantee preserved |
| --- | --- | --- |
| Service `stateRefs`, service `logRefs` | Remove options, wire fields, rendering, adapter assignments and fixtures | Runtime state placement/policy and service output capture remain owned by their current runtime components. |
| Task `artifactRefs`, `logRefs`, `summaryRefs` | Remove for all tasks, including obsolete composite acceptance | Task logs/summaries continue automatically. Artifact production remains child/project-owned; no collection system is implied. |
| Closure `kind` / `ClosureKind` | Remove declaration, enum and field from Nix/Rust contract | Tools, `run[0]`, executable path and host admission continue to determine execution roles. |
| Common invocation `timeoutMs` | Remove from `Invocation`; put the bounded duration on the leaf task alongside its invocation | Leaf task deadline remains enforced; exec probe deadline remains the existing outer probe timeout; service start does not gain an implicit TTL. |
| Old task/probe/secret discriminator-plus-payload option forms | Replace with native selected branches | Same primitive semantics, with incompatible authored combinations unavailable. |
| Singular `endpoint` shortcut and top-level service `endpoints`/`primaryEndpoint` pair | Replace with selected topology using the canonical endpoint map | Same runtime endpoint layout, primary selection and ownership checks. |

For the duration move, add the leaf-owned `timeoutMs` to the existing task wire record rather than introducing a second invocation record. Rust's task coherence must require a positive duration for a leaf and forbid it on a composite. Change native execution lowering and literal raw-wire proofs together. The shared invocation fragment then contains only facts valid at task, service-start and exec-probe sites.

Removing unused start/probe timeouts also removes their contribution to service compatibility hashing because they no longer exist in the hashed lifecycle invocation. Keep identity computation tied to the surviving contract; do not add exceptions that strip a growing list of ignored fields before hashing.

Keep project/app names and descriptions, generator/source provenance, consumed lifecycle terminal labels, operation IDs, closure operation-binding gates, executable requirements, task lifetime/output, state policy and the real listener attestation. “Already covered” is not an instruction to delete their genuine metadata or admission effects.

The unused fingerprint selector, dirty-warning behavior, and nonlistener effect values remain explicit follow-up decisions from the audit's partial/uncovered sections. This cleanup does not implement fingerprinting, artifacts, storage scopes, sandboxing, conditional orchestration or a daemon. It also does not silently claim those known gaps have been fixed. Adapter host propagation and command/reporting issues are separate fixes; keep them out of the compiler boundary cutover.

## Cleanup evidence correction

Keep the existing `CleanSpec` and shared terminal-label shape. Correct the actual missing consumer rather than creating a special clean-only terminal type or deleting all lifecycle evidence.

`run_slot_clean` currently emits service clean success before reconciliation, ownership/policy validation and deletion. Refactor around the existing single slot cleanup operation:

1. Establish per-service clean-start evidence for the attempt, with an explicit policy for a partially failed start-event write. Do not delete state if establishing required attempt evidence fails.
2. Run the existing aggregate slot cleanup exactly once; the state-cleanup layer remains the only owner of cleanup intent, deletion and recovery records.
3. After successful cleanup, emit service success labels. On refused/failed cleanup, emit existing service failure labels and retain the typed cleanup error as primary.
4. Preserve phase distinctions on evidence-write failure. A failure writing a success event after real deletion must not falsely report that deletion failed or rewrite history. A failure writing failure evidence must not mask the original ownership/policy/deletion error.

Precondition refusal must not fabricate `cleanup.intent` or `cleanup.deleted` records; the owning layer decides when an admitted cleanup attempt begins. Reuse current error-priority/cause facilities rather than inventing a second cleanup outcome authority. Test partial event writes and interrupted recovery as well as ordinary success/failure.

Do not infer the filesystem outcome from `Ok`/`Err` alone. The existing cleanup owner removes the tree before writing its `cleanup.deleted` record (`runtime/crates/nixfied-runtime/src/state/cleanup.rs:110`); that write can fail after actual deletion. Lifecycle failure means the operation did not settle successfully, not that files necessarily remain. Preserve the phase-specific error and existing interrupted-cleanup recovery ownership. Test this underlying registry-write failure as well as a later per-service terminal-event write failure.

This is a runtime correctness change independent of the compiler's general validation fix. Document the event ordering and rotate the behavioral ABI in its own coherent commit.

## Ordered delivery plan

Every commit must leave one current, coherent implementation. Suggested subjects are lower case. These are logical commits, not permission to leave Nix and Rust on different contracts between commits.

### Commit 1 — `close configured compilation evaluation boundary`

- Add the one compilation-only forcing boundary and route executable manifest production through it.
- Remove redundant ad hoc value-forcing checks while keeping semantic validations.
- Add generic strictness tests for unused bad values and for legitimate package/reference laziness.
- Do not alter manifest fields or accepted valid output. Prove ordinary valid manifests remain identical.
- Prove docs/help metadata can be read without forcing configured task values, contextual defaults or runtime packages; preserve program laziness in public app exports.

### Commit 2 — `represent authoring alternatives with native tagged options`

- Migrate task, probe, topology and secret declarations to `types.attrTag`, preserving shared fragments and native override behavior.
- Migrate compiler consumers, `seq` usage sites, adapters, examples, installer templates, gates, option/reference machinery and tests atomically.
- Preserve leaf/service reference fields, closure kind and their current default wire emissions until commit 3. Composite references can disappear with the incompatible branch because they were already absent from emitted composites. The final shape table describes the completed plan, not premature deletion in this commit.
- Remove superseded shape-coherence predicates. Add the genuine endpoint membership proof to existing relational validation.
- Keep the old invocation duration placement on both authoring and wire for this intermediate coherent commit: the newly selected leaf branch still uses `leaf.invocation.timeoutMs`. Service/probe invocation duration is also still the old contract until commit 3. Commit 3 moves the leaf option to `leaf.timeoutMs` while removing it from the shared invocation. Do not add a transitional dual path or advertise removal before that cutover.
- Verify emitted manifests for equivalent supported programs remain compatible, except where old inputs were invalid and now reject. Authoring-only shape changes need not rotate `runtimeAbi` unless the emitted/runtime contract changes.

### Commit 3 — `remove redundant manifest controls and move leaf deadlines`

- Remove the five refs, closure kind, and common invocation timeout; move leaf deadline to the task wire record.
- Update capability inventory, authored wire declarations, constants/digest snapshot, generated Rust, structural/native lowering, fixtures, independent vectors, views and docs in this same commit.
- Recompute the descriptor-derived ABI and acknowledge its snapshot; change numeric manifest/ABI-base/toolchain versions only if their defined semantics require it.
- Remove obsolete source assignments and tests whose only purpose was proving discarded metadata round-trips. Replace their useful guarantees with behavioral state/log/summary/reuse tests.
- Delete old readers and fields. Unknown-option/native unknown-wire rejection is the retirement behavior; no migration or alias path.
- Preserve historical upgrade archives/goldens as historical artifacts and append-only registry evidence. Update only current-source consumers and intentional current-contract snapshots.

### Commit 4 — `record clean lifecycle outcomes after slot cleanup`

- Implement the evidence correction above using existing cleanup, lifecycle and error owners.
- Update behavioral capability inventory/ABI and contract documentation together.
- Add refusal/deletion/evidence-failure/recovery tests that establish outcome order and retained primary errors.

Run focused proofs for each commit and the cross-layer gate for the final state. Do not combine independent adapter or installer fixes merely to inflate the cleanup's LOC reduction.

## Proof matrix

| Boundary / invariant | Required checks |
| --- | --- |
| Complete configured evaluation | An invalid declared-but-unselected task, bad nested scalar, unknown nested property and missing required payload all fail whole compilation before manifest emission. A valid unused declaration succeeds. |
| Native alternatives | Exactly one tag; unknown tag, two tags, conflicting tags across imports and wrong-branch fields reject. An inappropriate field set to the old default must still reject. |
| Native module semantics | Same-branch merges, imports, `mkDefault`, `mkForce`, list ordering, explicit branch overriding a default, and allowed absence/default behavior work as intended. Test empty lists/maps separately from missing required scalars. |
| No validation registry | Add a synthetic alternative and required field in a test declaration; native type/strict boundary/docs behavior follows without editing a central allowed-field list or checker dispatch. |
| Package and reference laziness | A valid package with poisoned unused `passthru` compiles; real store-path dependencies and source string context survive. Metadata queries tolerate poisoned configured values/providers and expose all branch option paths. |
| Genuine relations | Primary membership, graph/reference/cycle failures, source/target constraints, endpoint-less probe requirements, listener attestation and port capacity still reject at their existing owning boundary. |
| Wire retirement | Independent literal removed-field/enum cases reject in Rust, including nested service/task/invocation records; new task timeout missing/null/zero/wrong-kind cases reject. Old ABI rejects rather than translating. |
| Runtime continuity | State directory/policy, logs, redaction, summaries, task success codes, service reuse and command outputs retain behavior without refs or closure kind. |
| Timing | Leaf timeout still cancels a bounded command; probe outer timeout controls attempts; stopping and cancellation preserve containment/evidence. No unused inner deadline can churn reuse identity because it no longer exists. |
| Cleanup evidence | Successful deletion precedes success events; protected/unowned/live-owner refusals emit no success; deletion errors preserve failure evidence; failures writing either `cleanup.deleted` or a later service terminal event do not falsify the filesystem outcome; interrupted cleanup remains recoverable. |

Use existing owners: `nix/checks/option-metadata.nix`, structure/coverage/reference/maintenance checks, Nix/Rust derivation vectors, raw manifest wire tests, runtime service/output/state tests and downstream gates. Add a cohesive compiler-boundary check where needed; do not distribute an audit-specific denylist across fixtures and implementation.

Regenerate using `nix run .#regenerate` and the option-reference generation command in [DEVELOPMENT.md](DEVELOPMENT.md). Do not edit generated Rust or `docs/OPTIONS.md` manually. Run the narrowest focused checks first, then `nix run .#ci -- --dirty` for the cross-layer result. Report release-build and macOS/integration coverage explicitly; the debug fixture-backed gate is not a release/platform proof.

## Coupled files and documentation

- **Native declarations/compiler:** `nix/modules/{primitives,invocation}.nix`, `nix/compiler/{default,resolve,validate,derive,views}.nix`, affected graph/composition helpers, app compilation entry points.
- **Contract:** `nix/meta/manifest.nix`, `runtime/crates/nixfied-manifest/capability.txt`, derived ABI snapshot/constants, generated manifest definitions, native validation and runtime lowering/types/identity.
- **Consumers:** all adapters, examples, installer-generated modules, gate task graphs, test fixtures/builders and current downstream source fixtures.
- **Evidence:** runtime service clean orchestration, existing cleanup/error/event owners and focused integration tests.
- **Docs/reference:** CONTRACT, ARCHITECTURE, DERIVATION_SPEC where authoring/deadline/default representations change, GUIDE, ADAPTERS, DEVELOPMENT, generated OPTIONS, topic selectors, exact-query fixtures and publication usage examples. Update the audit to distinguish historical findings from completed changes after implementation.

## Review receipt and limits

Architecture review used the pinned Nixpkgs source `/nix/store/q1xsswrh8rfy39561x8hc3anfrbxzwvl-source/lib/types.nix`; `attrTag` is defined there at line 1070. Native source inspection is the basis for the recommendation, not an assumed third-party API.

The scratch evaluator at `/tmp/nixfied-architect-proof.nix` was run with `nix eval --json --file /tmp/nixfied-architect-proof.nix`. Its eight negative cases returned false (empty/unknown/multiple/conflicting tags, irrelevant default-valued field, missing required command, unknown nested field, unused invalid metadata); the merged timeout was `40`, package forcing succeeded without touching poisoned passthru, and metadata discovery returned both branches despite a throwing configured task. The negative tag/nested-field fixtures use otherwise-valid command payloads to avoid confounding failures. These results support the feasibility of the chosen mechanism, not completion of the production acceptance matrix above. Audit behavior receipts remain in [API_BEHAVIOR_AUDIT.md](API_BEHAVIOR_AUDIT.md).

No production implementation, regeneration, full CI or release/platform validation was performed while writing this plan. The unrelated `RFC_REFACTOR_DEDUP_LAYERS.md` remains untouched. The authoring syntax change is deliberately breaking and must ship with all current callers migrated; there is no proposed compatibility phase.
