# RFC: MFM POV Problem Analysis And Refactor Plan

Status: draft

Scope

- Capture the validated problems we observed from MFM's real use of nixfied.
- Record the solution directions that best fit MFM's engineering principles and architecture.
- Deep dive each problem area to identify adjacent issues we also need to solve.
- Turn the document into an execution-oriented refactor plan, not only a critique.

Validated Against

- MFM checkout: `/Users/willyrgf/dev/rust/src/github.com/willyrgf/2/mfm`
- MFM vendored nixfied: `/Users/willyrgf/dev/rust/src/github.com/willyrgf/2/mfm/nixfied`
- Current upstream: `/Users/willyrgf/dev/random/nixfied`

Principles To Preserve

- Explicitness over magic.
- Machine-readable introspection over debugger-only answers.
- Reusable architecture surfaces over project-local shell duplication.
- Thin public wrappers over model-backed primitives.
- Stable text and JSON contracts.
- Small additive changes over broad rewrites.

Cross-Cutting Observations

## 1. "App" Is Still Too Implicit

- Today most public apps are still derived from `task.ui.app`.
- `model.views.apps` is useful metadata, but it is not yet a first-class compiled app graph.
- This blocks two things at once:
  - generic introspection of app resolution
  - reusable public app wrappers like strict machine-output apps
- A large part of this RFC becomes cleaner if `apps` becomes a first-class compiled entity and `views.apps` becomes the user-facing projection of that entity.

## 2. Introspection And Execution Should Not Share The Same Closure

- Today the same architecture that drives execution also ends up being the only place where many answers exist.
- That is why simple questions like "why does `.#check` build `mfm_cli`?" require debugging the launcher path instead of querying a stable interface.
- A cheap introspection graph should answer compile-time questions without materializing the heavy execution runtime.

## 3. Service Reuse Needs One Stable Layer Above Existing Primitives

- The framework already has good low-level service and lifecycle helpers.
- The missing layer is the reusable model-backed abstraction that downstreams can compose.
- That layer should be the same one used for:
  - single-service session reuse
  - multi-service bootstrap
  - status and readiness surfaces
  - export and handoff contracts

## 4. Root Behavior Must Be Composable And Inspectable

- Workspace-scoped isolation is a good default.
- Shared roots are also a real downstream need.
- The problem is not lack of path override capability.
- The problem is that root behavior is currently expressed as project-local composition without a reusable named policy surface and without a normalized model export that explains the effective result.

Detailed Problem Review And Proposed Refactor

## 1. Introspection, App Resolution, And Closure Explanation

Current Failure

- A simple command like `nix run .#check` can realize unrelated builds like `mfm_cli`.
- Neither a user nor an agent can query a supported machine-readable surface and get the reason.
- Current introspection surfaces are too coarse:
  - `.#model` dumps the full model
  - `.#tasks` and `.#services` print tables
  - `.#task::<id>` dumps a task spec
- They do not answer:
  - what token resolves to what entity
  - what launcher path is taken
  - what runtime path is taken
  - why a derivation is in closure
  - who pulls that derivation in

Additional Problems Uncovered

- `app` is not yet a canonical compiled entity. It is still mostly a view.
- Selected-app resolution currently goes through heavy runtime materialization.
- The full serialized model still carries unrelated `runtimeInputs` paths.
- Stringifying packages with `builtins.toString` preserves string context, which makes a generic model export drag derivations into closure even when the selected app does not semantically need them.
- Local overrides, selected services, effective root policy, and app resolution are all difficult to inspect from one place.

Preferred Solution

- Add a generic `.#introspect` as the stable central introspection surface.
- Back it with a cheap compiled `introspectionGraph`, not the heavy execution runtime.
- Make `apps` a first-class compiled entity so app resolution is queryable instead of inferred from `views.apps`.
- Split "what exists in the graph?" from "what execution runtime do we materialize for this app?"

Preferred Model And API Shape

- Add `model.apps`.
- Add `model.introspection` or an adjacent exported `introspectionGraph`.
- Keep `model.views.apps` as a user-facing projection.
- Add `.#introspect` examples like:
  - `nix run .#introspect -- check`
  - `nix run .#introspect -- app:check --json`
  - `nix run .#introspect -- task:task.check`
  - `nix run .#introspect -- workflow:workflow.ci.full`
  - `nix run .#introspect -- service:postgres`
  - `nix run .#introspect -- app:check --why package:mfm-cli`
  - `nix run .#introspect -- reverse package:mfm-cli`
- The JSON contract should answer:
  - resolution target
  - entity kind
  - owner file
  - launcher path
  - runtime path
  - task or workflow mapping
  - workflow plan membership
  - selected service closure
  - package and derivation reasons
  - reverse dependency edges
  - local override activation
  - effective root policy

What To Avoid

- Do not make users inspect the heavy runtime model just to answer compile-time questions.
- Do not treat more docs as the solution.
- Do not leave app resolution as an implicit convention spread across launchers.

Execution Plan

1. Add a first-class compiled app model.
2. Remove task-owned app exposure from task authoring and move app authoring into explicit app definitions.
3. Add a compiler pass for a cheap introspection graph built from:
   - apps
   - tasks
   - workflows
   - service catalog
   - state metadata
   - selection index
4. Export the introspection graph as a package and query app.
5. Reimplement existing introspection commands in terms of the introspection graph where practical.
6. Add `why` and `reverse` query modes for closure explanation.
7. Add a closure-specific runtime manifest for selected apps.
8. Narrow selected-app execution so it no longer closes over unrelated package references by default.

Validation And Tests

- Contract tests for `.#introspect --json`.
- Regression test for the `.#check` / `mfm_cli` case.
- Contract test that app resolution can be explained without materializing the heavy runtime.
- Snapshot tests for human-readable output and stable key ordering in JSON output.

Concrete Refactor Shape

- Add `nixfied/modules/apps.nix` and import it from `nixfied/modules/core.nix`.
- Remove app authoring from `nixfied/modules/tasks.nix` and make app definition explicit in `nixfied/modules/apps.nix`.
- Add `nixfied.compiler.compile-apps.nix`.
- Add `nixfied.compiler.compile-introspection-graph.nix`.
- Extend `nixfied/compiler/default.nix` so compile order becomes roughly:
  - tasks
  - workflows
  - service catalog and service surface catalog
  - apps
  - selection index
  - introspection graph
  - views
  - finalized model
- Extend `nixfied/compiler/finalize-model.nix` to export top-level `apps`.
- Update `nixfied/schemas/model-export.json` for the new export shape.
- Extend `nixfied/framework/core/mkCoreSurfaces.nix` with:
  - `.#introspect`
  - optional additional packages for the introspection graph
- Replace or remove coarse introspection surfaces like `.#tasks`, `.#model`, and `.#task::<id>` where `.#introspect` becomes the clearer canonical surface.
- Only after the compile-side graph is stable, change launchers and runtime materialization.

Query Contract For `.#introspect`

- Input forms:
  - bare token: `check`
  - explicit selector: `app:check`, `task:task.check`, `workflow:workflow.ci.full`, `service:postgres`
  - reverse lookup target: `package:mfm-cli`, `derivation:/nix/store/...`, `task:task.mfm_cli`
- Core flags:
  - `--json`
  - `--why <target>`
  - `--reverse <target>`
  - `--kind <app|task|workflow|service|package|derivation>`
  - `--resolution-only`
  - `--execution`
  - `--closure`
- Ambiguous bare-token resolution should be explicit:
  - exact app name wins only if unique
  - otherwise fail with a machine-readable ambiguity error
  - do not guess silently
- `--why` should return all known model-level reason chains, not only one canonical path.
- Reason chains should be ordered with the most direct and shortest explanations first, but the response should remain exhaustive within the compiled graph.

JSON Output Shape For First Version

- Top-level keys:
  - `query`
  - `resolved`
  - `resolution`
  - `execution`
  - `closure`
  - `reverse`
  - `diagnostics`
- `resolution` should include:
  - entity id
  - entity kind
  - owner file
  - source kind
- `execution` should include:
  - launcher class
  - launcher target
  - run surface
  - mapped task or workflow ids
  - selected services
  - effective root policy reference
  - local override activation
- `closure` should initially be model-level, not store-level:
  - packages referenced by runtime inputs
  - hook runtime packages
  - workflow and task dependency edges
  - service requirements
  - generated execution dependencies
- `closure.reasonChains` should contain all known explanation chains for the queried target.
- `reverse` should initially answer "which model entities reference this target?"

Reason Layers We Need To Keep Separate

- Layer 1: graph resolution
  - what token resolves to which app, task, workflow, or service
- Layer 2: execution resolution
  - what launcher and runtime surface are used
- Layer 3: model dependency explanation
  - which tasks, hooks, workflows, services, or packages pull something in
- Layer 4: realized store closure
  - which derivations are actually built by Nix
- The first implementation should fully solve layers 1 through 3.
- Layer 4 can start as a smaller additive explanation surface later if needed.

Why This Matters

- The `.#check` and `mfm_cli` problem is mostly a layer 3 failure today.
- Users do not first need full Nix store graph theory.
- They need the framework to say:
  - `check` resolves to app `check`
  - app `check` resolves to task `task.check`
  - selected-app execution currently serializes a full model
  - the full model includes task `task.mfm_cli`
  - that task includes package `mfm-cli` in its runtime inputs
- That answer should be available without reverse-engineering implementation details.

Schema Strategy

- This refactor likely justifies a model schema version bump.
- Recommended direction:
  - add top-level `apps`
  - keep `views.apps` only as the user-facing projection of explicit apps
  - set `model.schema.version` from `2` to `3`
  - remove task-owned app exposure from the task schema
  - make `.#introspect` the canonical introspection surface

Resolved Decisions

- `apps` lands in one hard cut together with removal of task-owned app exposure.
- `.#introspect --why` returns all known reason chains in the compiled graph.
- Coarse legacy introspection surfaces are removed immediately once `.#introspect` lands.
- Layer 3 explanation stays at package-name level; store paths remain the job of lower-level Nix tooling.
- `.#introspect` ships with both human and JSON modes, with human mode as the default.

Likely Second-Step Runtime Refactor

- After introspection lands, add app-specific execution manifests.
- A selected app should materialize:
  - only the relevant task or workflow subgraph
  - only the relevant runtime package references
  - only the relevant service closure
- Likely shape:
  - compile `appExecutionPlan` or `appRuntimeManifest`
  - feed that into a narrower materialization path instead of the whole compiled model
- This should be done after the compile-side introspection graph is stable, so the same graph can explain the new narrower behavior.

## 2. Root Policies And Shared Runtime Behavior

Current Failure

- MFM needs shared runtime roots for some reuse and persistence flows.
- Upstream defaults now prefer workspace-scoped isolation, which is correct as a default.
- Downstream shared-root behavior exists today only as ad hoc path composition and overrides.
- The compiled model exports roots, but not the policy that produced them.

Additional Problems Uncovered

- Root semantics are tightly coupled with service reuse semantics.
- The root-policy domain is currently split across:
  - `runtime.directories.base`
  - `state.workspaceId`
  - `state.registryRoot`
  - `state.artifactsRoot`
- That split makes one coherent root policy harder to define and inspect.
- Root behavior is resolved in project composition logic instead of being a named reusable policy surface.
- `workspaceId` derivation is also part of the same policy domain today, but it is not modeled that way.
- There is no stable model export that says whether the effective policy is:
  - workspace-scoped
  - project-shared
  - slot-shared
  - custom
- Because of that, introspection cannot explain why state paths look the way they do.

Preferred Solution

- Do not add a broad user-facing mode flag.
- Add reusable root-policy modules or surfaces that projects can compose and override.
- Normalize the effective result into a model-backed root policy object that introspection can explain.

Preferred Model And API Shape

- Add a first-class compiled `statePolicy`.
- Add reusable framework policy modules such as:
  - workspace-scoped
  - project-shared
  - slot-shared
- Allow downstreams to define project-owned policy modules as well.
- Compile the effective result into a normalized export such as:
  - `model.state.policy.id`
  - `model.state.policy.kind`
  - `model.state.policy.workspaceId`
  - `model.state.policy.runtimeBase`
  - `model.state.policy.registryRoot`
  - `model.state.policy.artifactsRoot`
  - `model.state.policy.ownerScope`
  - `model.state.policy.discoveryScope`
  - `model.state.policy.source`
- Make `runtime.directories.base` a projection of the resolved state policy rather than a separate unrelated root decision.
- Allow later surfaces like `serviceSet` to reference a root policy or override it explicitly.

What To Avoid

- Do not hide shared-root behavior behind silent conventions.
- Do not introduce a second unrelated root-policy mechanism in service/session code.
- Do not force downstreams to choose between reusability and explicitness.

Execution Plan

1. Add a compiler pass for normalized state-policy resolution.
2. Move `workspaceId`, runtime base, registry root, and artifacts root under one coherent policy domain.
3. Define a normalized root-policy schema.
4. Ship framework-owned reusable root-policy modules.
5. Export effective root policy into the model and introspection graph.
6. Allow higher-level surfaces such as `serviceSet` to reference or specialize the policy.
7. Backfill docs with explicit examples and remove the old ad hoc root-resolution path.

Validation And Tests

- Tests for workspace-scoped default behavior.
- Tests for project-shared and slot-shared policy modules.
- Introspection tests that the effective root policy is exported and stable.
- Tests that root-policy resolution fully owns `workspaceId`, runtime base, registry root, and artifacts root.

Concrete Refactor Shape

- Add `nixfied/modules/state.nix` and import it from `nixfied/modules/core.nix`.
- Add `nixfied/compiler/compile-state-policy.nix`.
- Move state-related module options out of `nixfied/modules/core.nix` into the dedicated state module and make `statePolicy` authoring explicit there.
- Add framework presets under a path like:
  - `nixfied/framework/presets/state-policies/workspace-scoped.nix`
  - `nixfied/framework/presets/state-policies/project-shared.nix`
  - `nixfied/framework/presets/state-policies/slot-shared.nix`
- Remove root-resolution logic from `nixfied/project/module.nix`.
- Change `nixfied/project/runtime.nix` to consume compiled state-policy outputs instead of pre-resolved roots passed from project composition.
- Extend `nixfied/compiler/default.nix` so state-policy resolution happens immediately after module resolution and before final model export.
- Extend `nixfied/compiler/finalize-model.nix` to export the normalized state policy.
- Extend `nixfied/compiler/compile-views.nix` and the introspection graph so help and docs can explain the effective policy cleanly.

State-Policy Contract We Actually Need

- One policy object should own:
  - workspace id derivation
  - runtime directory base
  - registry root
  - artifacts root
  - owner scope semantics
  - discovery scope semantics
  - policy source metadata
- Projects should author this by composing policy modules, not by sprinkling path overrides across unrelated files.
- The compiled export should contain both:
  - resolved concrete paths
  - enough metadata to explain why those paths were chosen

Why This Matters

- Shared-root behavior is not only a path question.
- It directly changes reuse semantics, discovery semantics, and future service-session behavior.
- If we do not unify this domain now, `serviceSet` will either reimplement root logic or depend on opaque project-local conventions.

Resolved Decisions

- `statePolicy` gets a dedicated `nixfied/modules/state.nix` module.
- `artifactsRoot` is fully owned by `statePolicy` from day one.
- Root-policy authoring should be as rich as necessary to express reusable policies and overrides cleanly; do not artificially constrain it to resolved-path metadata only.

## 3. Service Sessions And Service Sets

Current Failure

- The framework already has low-level service policy helpers and service lifecycle primitives.
- MFM still re-implements reusable session behavior in project shell.
- There is no first-class compiled entity for a reusable service session or service set.

Additional Problems Uncovered

- The missing abstraction is not only `start` and `stop`.
- The missing abstraction also includes:
  - reuse policy
  - owner scope
  - discovery scope
  - runtime-root selection
  - setup hooks
  - export and handoff contract
  - cleanup semantics
  - stable `health` and `ready`
- Service APIs exist today per service, but not for groups of services.
- Whole-project `health` and `ready` operations already behave like one hard-coded global service set.
- If we do not model grouped service behavior explicitly, we will keep growing one-off grouped service paths in operations and downstream project shell.
- The multi-service bootstrap problem is downstream evidence that this abstraction is missing.

Preferred Solution

- Add first-class `serviceSet` surfaces as the reusable abstraction.
- Treat a single-service session as a one-member service set rather than inventing a second abstraction first.
- Reuse the existing low-level policy and lifecycle helpers under the hood.

Preferred Model And API Shape

- Add `model.serviceSets`.
- Compile service sets before apps so app surfaces can target service-set operations directly.
- A service set should define:
  - members
  - optional members
  - startup ordering
  - readiness policy
  - reuse policy
  - owner scope
  - discovery scope
  - root policy reference
  - setup hooks
  - export and handoff contract
  - failure log capture policy
- Generate stable lifecycle surfaces:
  - `start`
  - `status`
  - `stop`
  - `health`
  - `ready`
  - `export`
- Treat current project-wide readiness and health behavior as a default grouped-service surface, not as a separate concept.
- Expose service-set entities through the app model and introspection graph.

What To Avoid

- Do not create a second policy system separate from existing service helpers.
- Do not make multi-service reuse a special CI-only system.
- Do not stop at per-service lifecycle APIs when downstreams need grouped semantics.

Execution Plan

1. Define `serviceSet` module schema and model export.
2. Compile service sets into the model and introspection graph.
3. Generate runtime surfaces for service-set lifecycle operations.
4. Reuse existing service-policy and managed-service lifecycle helpers underneath.
5. Add JSON-friendly status and export contracts.
6. Migrate one framework-owned or test-owned use case onto `serviceSet`.
7. Use MFM's snapshot and CI bootstrap flows as downstream proving cases.

Validation And Tests

- Service-set lifecycle contract tests.
- Readiness and health contract tests.
- Export and handoff contract tests.
- Reuse-policy validation tests.
- Service-exclusion tests when a selected graph omits required services.

Concrete Refactor Shape

- Add `nixfied/modules/service-sets.nix` and import it from `nixfied/modules/core.nix`.
- Add `nixfied/compiler/compile-service-sets.nix`.
- Extend `nixfied/compiler/default.nix` so service sets are compiled after service catalog and state policy, and before apps.
- Export top-level `model.serviceSets`.
- Add a runtime layer like `nixfied/framework/core/mkServiceSetRuntimeSurfaces.nix`.
- Keep `nixfied/framework/core/mkServiceRuntimeSurfaces.nix` focused on per-service surfaces.
- Reuse the existing low-level helpers rather than replacing them:
  - `service-policy.nix`
  - managed-service lifecycle helpers
  - service API contract patterns where they still fit
- Decide whether grouped surfaces need their own contract helper file, or whether an extended generic service-surface helper is enough.
- Re-express whole-project grouped readiness and health checks through a service-set-backed surface once the new layer exists.

Service-Set Contract We Actually Need

- A service set should own:
  - membership
  - optional membership
  - startup order
  - readiness semantics
  - health semantics
  - reuse policy
  - owner scope
  - discovery scope
  - root-policy reference
  - setup hooks
  - export and handoff contract
  - failure-log capture policy
- Export and handoff should be explicit, not implied.
- Export should be modeled per service for composability, with an optional combined summary layered on top.
- Per-service records are the canonical truth.
- Any grouped summary should be a deterministic derived view over those per-service records:
  - ordered service records
  - aggregated status
  - concatenated failure list
  - optional derived counters or summary fields
- The first cut should support machine-readable export output directly, not only shell environment exports.

Relationship To Existing Service Surfaces

- Per-service public surfaces stay valid.
- `serviceSet` adds grouped behavior above them.
- `serviceSet` should not force every grouped operation to be implemented as shell calling public service apps recursively.
- It should compile its own grouped runtime plan and reuse the lower-level lifecycle helpers where appropriate.
- Current whole-project `health` and `ready` operations can later become a default named service set, or at least be implemented through the same grouped runtime path.

Why This Matters

- MFM's duplicated session and bootstrap code is not a one-off smell.
- It is the downstream symptom of a missing grouped-service abstraction.
- If we solve only the single-service API and not the grouped layer, downstreams will keep re-implementing the hardest parts:
  - reuse
  - readiness
  - export and handoff
  - failure collection

Resolved Decisions

- `serviceSet` should be allowed to override root policy immediately when the user needs it.
- `serviceSet export` should support both machine-readable JSON output and env-export mode.
- Optional members affect `health` only; `ready` remains strict.
- Whole-project grouped behavior should become a built-in default service set with `health` as the default mode and an explicit option to make `ready` the default.
- Grouped bootstrap export is per service, with optional combined summary data.
- Failure-log collection belongs in the core grouped runtime by default; wrappers may add presentation-specific handling on top.
- Combined summaries are derived mechanically from per-service records rather than being an independent contract source.

## 4. Public Machine-Output Apps

Current Failure

- MFM has a real public command pattern that reserves stdout for final JSON and sends logs elsewhere.
- Today that pattern is still manually assembled in project shell.
- Workflow semantics also remain intentionally limited:
  - one shared passthrough argv
  - no per-step args
  - no per-step env
  - no native step output wiring
- That means strict public output contracts are hard to express without hand-built wrappers.

Additional Problems Uncovered

- This problem is partly an app-model problem, not only a workflow problem.
- Without first-class apps, wrappers end up being encoded indirectly as tasks.
- The runtime already has machine-JSON behavior internally, but it is still exposed as a hidden execution detail rather than as an explicit public app model.
- A reusable wrapper must be able to target more than one thing:
  - a task
  - a workflow
  - a service-set operation
- Validation and replay of final machine output need to be explicit and inspectable.

Preferred Solution

- Add a reusable machine-output wrapper surface on top of an explicit app model.
- Keep it narrow and contract-oriented.
- Do not turn workflows into a general dataflow engine.

Preferred Model And API Shape

- Add first-class app entities with kinds such as:
  - `taskRef`
  - `workflowRef`
  - `serviceSetOp`
  - `machineOutput`
- A `machineOutput` app should declaratively describe:
  - setup phase
  - main target
  - teardown phase
  - captured final output source
  - validation step
  - stdout replay contract
  - log routing mode
- The target should remain extendable by downstream modules.

What To Avoid

- Do not solve this by making workflows much more implicit.
- Do not force downstreams to keep rebuilding the stdout reservation pattern by hand.
- Do not hide validation and replay logic inside bespoke shell without model visibility.

Execution Plan

1. Make apps first-class compiled entities.
2. Add `machineOutput` as a new app kind.
3. Teach the selected-app launcher to resolve explicit app entities first.
4. Add one framework example and one downstream proving case.
5. Document how to build strict JSON commands from reusable surfaces instead of shell glue.

Validation And Tests

- Machine-output contract tests for valid JSON replay.
- Tests that logs do not corrupt final stdout.
- Failure-path tests for validation errors.

Concrete Refactor Shape

- Add `machineOutput` as an explicit kind in `nixfied/modules/apps.nix`.
- Compile that kind in `nixfied/compiler/compile-apps.nix`.
- Add a runtime implementation layer for app kinds, either:
  - a dedicated machine-output runtime helper
  - or a generic app-kind runtime dispatcher with `machineOutput` as one branch
- Reuse existing shell-contract JSON validation and emission helpers where they fit.
- Stop treating hidden executor-level `MACHINE_JSON` behavior as the public abstraction.
- Make selected-app launchers resolve the explicit machine-output app directly.

Machine-Output Contract We Actually Need

- One machine-output app should own:
  - setup phase
  - main target
  - teardown phase
  - output source
  - validation rule
  - stdout replay rule
  - log routing rule
- Output source should support at least:
  - direct JSON payload
  - file path
  - generated artifact
- Failure behavior should be explicit:
  - invalid output
  - missing output
  - setup failure
  - teardown failure

Why This Matters

- The runtime already knows how to preserve machine JSON in some cases.
- The real gap is that users cannot author that behavior as a clear reusable public contract.
- If we leave it implicit, downstreams will keep building wrapper shell around hidden executor details.

Resolved Decisions

- The first cut of `machineOutput` is JSON-only.
- Validation should support both schema-based and command-based modes, with schema-based validation as the canonical path for deep introspection and command-based validation as an escape hatch.
- Teardown failure replaces the final output in the first cut.

## 5. Multi-Service Bootstrap

Current Failure

- MFM's CI parity bootstrap manually handles service discovery, startup ordering, readiness, hooks, optional services, and failure log collection.
- The framework primitives are good enough to build this manually.
- They are not yet enough to make the pattern small, reusable, and obvious.

Additional Problems Uncovered

- This is not a separate orchestration domain from sessions.
- It is the grouped form of the same service lifecycle and reuse problem.
- We also need a clean way for workflows and wrapper apps to consume the result of a service-set bootstrap without rewriting the same shell contract repeatedly.
- Current workflow `preRun.tasks` and `postRun.tasks` already reveal the need for grouped-service lifecycle integration, but only through generic task hooks.

Preferred Solution

- Build multi-service bootstrap on top of `serviceSet`.
- Add small integration surfaces where they are justified.
- Keep the single orchestration abstraction consistent.

Preferred Model And API Shape

- A `serviceSet` should support:
  - multiple services
  - optional services
  - startup ordering
  - readiness policies
  - setup hooks
  - failure log collection
  - export contracts
- Wrapper apps and workflows can then consume those surfaces.
- Add workflow-native adapters immediately as thin adapters over `serviceSet`, not as a second orchestration model.

What To Avoid

- Do not create a separate CI bootstrap DSL.
- Do not duplicate service policy logic in workflow code.
- Do not solve grouped startup with more project shell if the underlying abstraction is the same as service sessions.

Execution Plan

1. Land `serviceSet` first.
2. Express multi-service bootstrap as a service-set proving case.
3. Add workflow-native service-set adapters immediately on top of the base service-set layer.
4. Migrate large bootstrap tasks gradually instead of by one-shot rewrite.

Validation And Tests

- Multi-service ordering tests.
- Optional-member tests.
- Failure log collection tests.
- Workflow or wrapper integration tests once the thin adapter exists.

Concrete Refactor Shape

- First implementation:
  - express grouped bootstrap entirely through `serviceSet`
  - let tasks, workflows, or explicit apps call the generated service-set surfaces
  - add thin workflow-native adapters such as `preRun.serviceSets` or `postRun.serviceSets`
  - compile those adapters into ordinary service-set lifecycle calls
- Do not encode another orchestration graph for CI bootstrap.
- Re-express existing global readiness and health workflow patterns through the same grouped runtime once the service-set layer exists.

Why This Matters

- MFM's bootstrap logic is large because grouped service lifecycle is real, not accidental.
- If we skip the service-set-first shape and go straight to workflow-specific adapters, we will recreate the same problem in a second place.

## 6. `nixfied/local/default.nix` Activation

Current Failure

- The seam is real and useful.
- The current wrapper template sets `localOverrides = [ ]`.
- A downstream can define real local apps and still have them silently inactive.

Additional Problems Uncovered

- Activation state is currently not obvious in help, introspection, or model output.
- The install wrapper and the selected launcher both need to agree on local override wiring.
- The issue is not capability. The issue is visibility and ergonomics.

Preferred Solution

- Keep local overrides explicit and opt-in.
- Make their activation state inspectable.
- Add linting or doctor-style warnings for the "file exists but is not loaded" case.

Preferred Model And API Shape

- Export local override metadata into the introspection graph:
  - active or inactive
  - source paths
  - effective module list
- Improve generated wrapper comments to show exactly how to wire local overrides.
- Add a lint or doctor surface for common activation mistakes.

What To Avoid

- Do not auto-load local overrides silently.
- Do not leave the seam invisible in model-backed introspection.

Execution Plan

1. Improve wrapper template comments and examples.
2. Export local override activation into the introspection graph.
3. Add a lint or doctor surface for inactive local overrides.
4. Add help or docs references once the introspection path exists.

Validation And Tests

- Wrapper template tests.
- Introspection tests for active and inactive local overrides.
- Lint or doctor tests for the non-empty-but-unwired case.

Concrete Refactor Shape

- Extend `nixfied/framework/install/wrapper-flake.nix` comments and generated defaults.
- Export local override activation into the introspection graph and model-backed diagnostics.
- Add a small lint or doctor surface focused on local override activation.
- Make `.#introspect` able to answer:
  - whether local overrides are enabled
  - which module paths are active
  - whether a local override file exists but is unwired

Why This Matters

- The seam itself is fine.
- The problem is that today the framework makes it too easy to believe a local surface is active when it is not.
- Once `.#introspect` exists, this should become a simple factual query instead of a source-reading exercise.

Open Questions To Resolve Before Implementation

- Do we want a standalone `doctor` surface, or should `.#introspect --diagnostics` own this?
- Should unwired local overrides be a warning only, or a hard failure in some modes?

## 7. The Monolithic Project Layer

Current Position

- MFM's large `nixfied/project/module.nix` is a real maintenance issue.
- It is also a symptom.
- A large part of that file exists because key framework surfaces are still missing or too weak.

Decision

- Do not make "split the monolith" the first-order solution.
- Fix the missing surfaces first.
- Expect the monolith to shrink naturally as downstream glue is replaced by reusable model-backed surfaces.

Full Refactor Dependency Graph

- First-class `apps` supports:
  - `.#introspect`
  - machine-output wrappers
  - clearer launcher resolution
- Introspection graph supports:
  - closure explanation
  - local override visibility
  - root policy explanation
  - better agent and user debugging
- Root policy supports:
  - service-set semantics
  - shared runtime behavior
  - future policy overrides without ad hoc path rewrites
- `serviceSet` supports:
  - reusable service sessions
  - multi-service bootstrap
  - cleaner machine-output wrappers that depend on services

Recommended Phased Delivery

## Phase 0: Foundations

- Add first-class compiled `apps`.
- Remove task-owned app exposure.
- Move app definition into the explicit app layer.
- Land this as one hard cut, not a compatibility transition.

## Phase 1: Introspection

- Add compiled `introspectionGraph`.
- Add `.#introspect`.
- Export local override activation and current app resolution through it.
- Remove coarse introspection surfaces in the same cut.

## Phase 2: Root Policy

- Extract and normalize root-policy resolution.
- Ship reusable root-policy modules.
- Move artifacts ownership fully into `statePolicy`.
- Export effective policy into the model and introspection graph.

## Phase 3: Service Sets

- Add `serviceSet` schema, model export, and runtime surfaces.
- Reuse current low-level service helpers.
- Land lifecycle surfaces: `start`, `status`, `stop`, `health`, `ready`, `export`.

## Phase 4: Machine-Output Apps

- Add `machineOutput` app kind.
- Resolve explicit apps in selected-app launchers.
- Migrate one strict JSON command onto the new wrapper surface.

## Phase 5: Bootstrap And Cleanup

- Express multi-service bootstrap through `serviceSet`.
- Add optional workflow adapter only if still needed.
- Add local override linting and wrapper improvements if not already landed.
- Reduce downstream monolithic glue as a consequence of the new surfaces.

Recommended Commit Sequence

1. `rfc: finalize refactor execution plan`
   Keep the design and rollout contract stable before implementation work starts.
2. `apps: add explicit app model and remove task-owned app exposure`
   This is the hard-cut commit for explicit apps.
3. `introspection: add introspection graph and .#introspect`
   Land human and JSON modes and remove coarse old introspection surfaces in the same cut.
4. `state: add state module and compile state policy`
   Move `workspaceId`, runtime base, registry root, and artifacts root under `statePolicy`.
5. `runtime: add app execution manifests and narrow selected-app closure`
   Attack the `.#check -> mfm_cli` over-closure structurally.
6. `service-set: add compiled serviceSet model and grouped runtime surfaces`
   Include per-service export records, grouped lifecycle surfaces, and core failure-log collection.
7. `workflow: add native serviceSet adapters`
   Add workflow-native adapters such as `preRun.serviceSets` and `postRun.serviceSets`.
8. `machine-output: add explicit machineOutput app kind`
   Land JSON-only first cut with schema-first introspectable validation.
9. `bootstrap: migrate multi-service bootstrap onto serviceSet`
   Move proving cases onto the new grouped runtime.
10. `local-overrides: add diagnostics and wrapper visibility`
    Finish local override introspection and linting.
11. `cleanup: remove dead paths and shrink project glue`
    Remove superseded code only after the new surfaces are proven.

Commit Discipline

- Every commit should leave the repo building and testable.
- The only intentionally disruptive commit is the explicit app hard cut, and even that should be internally complete in one step.
- Avoid mixing refactor layers in the same commit when rollback value would be lost.

Suggested Migration Sequence For MFM

1. Replace ad hoc `model-introspection.nix` usage with `.#introspect`.
2. Re-express shared-root behavior through a reusable root-policy module.
3. Extract current services-start logic into one or more `serviceSet` definitions.
4. Rebuild the snapshot-style strict JSON command as a `machineOutput` app.
5. Shrink the project monolith only after the new surfaces exist and are proven.

Not In Scope For This RFC

- A Rust or EVM preset.
- A standalone migration initiative.
- Silent auto-loading of `nixfied/local/default.nix`.
- Another low-level service policy system separate from the one already present.
- Turning workflows into a full general-purpose dataflow engine.

Expected Outcome

- Users can query nixfied directly instead of debugging it.
- Agents can reason from stable machine-readable surfaces instead of reverse-engineering implementation details.
- Downstreams get reusable architecture surfaces instead of assembling large project-local shell tasks.
- Shared-root behavior becomes reusable without becoming implicit.
- Service reuse and multi-service bootstrap converge on one stable abstraction.
- Public machine-output commands become explicit model-backed wrappers instead of bespoke shell.
- Reusability increases without hiding behavior or reducing explicitness.
