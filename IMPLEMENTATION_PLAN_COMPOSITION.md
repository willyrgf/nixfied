# IMPLEMENTATION PLAN: the task–service algebra

Status: implementation plan for `DESIGN_COMPOSITION.md`. The document series is
`PROBLEM_COMPOSITION.md` (why) → `DESIGN_COMPOSITION.md` (what, and the logic
behind it) → this file (how, in what order, with what proofs). The design is a
full rewrite of the model's semantic surface; this plan decomposes it into
phases, and each phase into commits — one buildable slice per commit, each with
its own targeted proof — ending with the documentation work that makes the new
design and its reasons the recorded contract.

## Ground rules

- **No backward compatibility, anywhere, at any point.** This lands on the
  `dev` branch; every breaking change the new architecture needs is allowed —
  wire format, module surface, run selection, generated views, error payloads,
  evidence schema. Old structure is deleted, not wrapped; no transitional shims,
  not even between commits of one phase. Adopters still experience exactly one
  ABI rotation because they pin revs and ABI-1 derives the digest from the
  capability descriptor — intermediate rotations on `dev` are free and invisible.
- **The work divides per commit.** Each phase below enumerates its commits. A
  commit is one coherent slice: it builds, and it passes the targeted proof
  named next to it. Because `lower.rs` destructures every model struct with no
  `..`, a wire-contract commit carries its lowering counterpart by
  construction — that is the slicing constraint, not a compatibility concern.
- **`.#ci` is the whole-workflow validator, not the per-commit loop.** It is
  expensive; run it only when the whole workflow needs validation — at each
  phase boundary and before release. The per-commit loop is the targeted check
  named with each commit (the cargo floor for runtime slices, evaluating or
  building the affected models for Nix slices, the specific gate stage a slice
  touches).
- **Commits**: single concise line, no body, per repo convention.

## Phase map

| Phase | Delivers | Kills |
| --- | --- | --- |
| 0 | spike + the normative derivation spec | nothing (throwaway code) |
| 1 | `invocation` + leaf tasks (tools, `run`, per-leaf env/timeout) | `execs`, the closure/exec split in authoring |
| 2 | composite tasks (nested DAG, stable step paths) | `workflows` |
| 3 | derived facts + membership deletion + `--task` selection | `environments.*.{services,tasks}`, hand-declared `operationBindings`/`operationId`s, `servicesRequired` |
| 4 | endpoint-optional services + prepare-as-task | the endpoint requirement; prepare-as-bare-exec |
| 5 | the generated surface (`surface.verbs`, `admit`, reserved namespace) | framework-named project verbs in `project-apps.nix` |
| 6 | adapters + examples in final form; the two new gate examples | the old example/adapter idiom |
| 7 | documentation + the mfm acceptance run | the stale docs |

Phases 1–4 each rotate the capability digest; phase 5 changes only generated
views and flake surface; phases 6–7 change no contract.

## Phase 0 — spike and the derivation spec

The cheapest decisive experiment, run before any contract change is committed.

1. Prototype the new Nix module surface and emitter far enough to compile two
   models to `model.json`: the paper mfm rewrite (all ten tasks, three
   workflows-as-composites, `surface.verbs`) and a minimal endpoint-less worker
   (no listener, invocation-probed readiness, `connectsTo` postgres, one leaf
   `requires`-ing it).
2. Measure: model size vs. today's mfm model (the INVOKE-1 inflation factor),
   and confirm nothing in the ten tasks needs a concept the algebra lacks.
3. Write **the normative derivation spec** — the one deliverable that survives
   the spike: exact algorithms, as pseudocode plus golden test vectors, for
   - `servicesRequired(task)` = union of transitive leaf `requires`, closed
     over `connectsTo`, with ordering and dedup rules pinned;
   - `operationBindings(closure)` from the leaf/lifecycle graph;
   - default `operationId`s (`task.<name>.run`, `service.<name>.<op>`) and
     default terminal tokens;
   - stable **step paths** for composite evidence identity
     (`<root>.<step>...<step>`, the run-twice disambiguator).

   Both implementations (Nix compiler, runtime lowering) are written against
   this spec, and the golden vectors become fixtures on both sides. This is the
   DERIVE-1 drift mitigation: the ABI-digest analogy holds only if the two
   implementations share one normative source, so that source is created first.

Commits:

1. `docs: record the derivation spec and golden vectors for the composition algebra`
   — check: none beyond review; spike code is discarded, findings go in the
   commit/PR description.

## Phase 1 — invocation and the leaf task

The contract gains `invocation = { tools, run, env, cwd, timeoutMs, stdin }`
inline (INVOKE-1: anonymous, no registry) and the leaf task
`{ invocation, requires, exitPolicy, refs }`. `execs` dies. The child
environment becomes **hermetic** (declared env + runtime-owned variables only)
— a deliberate semantic decided here: the design's "shell env defaults become
typed attrs" claim is only sound under hermetic env, and append-to-inherited
patterns (`../mfm/nixfied.nix:19-21,69`) are consciously not carried.

Commits:

1. `model: carry inline invocations and leaf tasks through the wire contract`
   — model crate types + structural validation + `capability.txt` rotation +
   the mechanical lowering counterpart (no-`..`). Check: cargo floor.
2. `runtime: assemble leaf PATH from declared tool roots and verify tools at admission`
   — eval-resolved `run[0]` carried in the model; admission verifies every
   tool element like any closure (SEAM-1/PREPARE-1). Check: cargo floor.
3. `runtime: spawn task leaves with a hermetic declared-only environment`
   — check: cargo floor (env isolation tests).
4. `nix: author leaf tasks through the invocation surface and delete execs`
   — modules + compiler resolve/validate/derive/emit. Check: build the
   `minimal` example model.
5. `nix: move the adapters and examples onto inline invocations`
   — mechanical; workflows and membership untouched until phases 2–3.
   Check: build every example model; run the gate's example stage.

Phase boundary: `.#ci`.

## Phase 2 — composite tasks

Tasks become `leaf | composite`; `workflows` dies. Failure semantics are
today's cancellation applied recursively; run-once is per step (a task
referenced twice flattens twice, deterministically; no memoization);
`exitPolicy` stays leaf-only.

Commits:

1. `model: add composite tasks with named steps to the wire contract`
   — check: cargo floor.
2. `runtime: flatten composites onto the run plan with stable step paths`
   — recursive flattening in `plan.rs` per the phase-0 spec; cycle rejection
   through nesting; recursive cancellation. Check: cargo floor.
3. `runtime: qualify task evidence by step path`
   — registry rows, log/summary refs, and their snapshot guards change here,
   not as a later surprise. Check: cargo floor.
4. `nix: validate composite graphs at eval and add lib.seq sugar`
   — acyclicity, declared step refs, sibling `dependsOn`. Check: negative
   eval fixtures.
5. `nix+model: replace example workflows with composites and delete the workflows section`
   — contract deletion + example ports in one slice. Check: build every
   example model; run the gate's example stage.

Phase boundary: `.#ci`.

## Phase 3 — derived facts, membership deletion, task selection

The DERIVE-1 phase, and the most adopter-visible one.

Commits:

1. `nix: derive servicesRequired, operationBindings, and operation ids per the spec`
   — derived facts carried in the model; explicit `operationBindings` survives
   as an optional narrowing gate, explicit ids/terminals as overrides.
   Check: the golden vectors as Nix eval fixtures.
2. `runtime: re-derive and compare derived facts at admission, fail closed`
   — mismatch is `MODEL_ADMISSION` naming both values. Check: cargo floor with
   the same golden vectors as fixtures.
3. `model+nix: delete environment membership; environment becomes the isolation key`
   — `environments.<e>.{services,tasks}` leaves the module surface and the
   wire format; adapters drop their membership lists; `placement.rs` keying
   unchanged. Check: cargo floor + build every example model.
4. `runtime: select runs by task and refuse with the declared-task list`
   — `run --task <id>`; service set = the derived union for the selected tree,
   started eagerly (today's observed behavior, `main.rs:304-321`).
   Check: cargo floor; run one example end to end through the gate stage.

Phase boundary: `.#ci`, plus the `mkForce` proof — importing both adapters
adds zero startup to a task that requires neither.

## Phase 4 — endpoint-optional services and prepare-as-task

The service kind's two changes, together because both touch the lifecycle type.
PORT-1's behavior is restated scoped here (ownership verification where an
endpoint exists, unchanged; no claim where none does); its invariant text
changes in phase 7.

Commits:

1. `model+nix: make service endpoints optional with probe and placeholder coherence`
   — at most one endpoint form; endpoint-less ⇒ `tcp` probes rejected, named
   placeholders resolving to it rejected in every scope, bare
   `${port}`/`${host}` in its own lifecycle rejected (the task rule at
   `lower.rs:373-388`, applied to services). `requires`/`connectsTo` toward it
   stay legal. Check: cargo floor + negative eval fixtures.
2. `runtime: scope endpoint-ownership readiness to declared endpoints`
   — SVC-ID-1 identity over the empty endpoint set covered by an identity
   test; the planner needs no change (`plan.rs:134-189`). Check: cargo floor.
3. `model+runtime: enforce effects coherence in both directions`
   — endpoints ⇒ `network-listener` on the start closure; endpoint-less +
   `network-listener` ⇒ rejected. Check: cargo floor.
4. `model+nix+runtime: bind prepare to a task reference with combined-graph acyclicity`
   — cross-service prepare `requires`; the combined `connectsTo` +
   prepare-`requires` graph acyclic. Check: cargo floor + negative fixtures.
5. `runtime: render heterogeneous prepare and connectsTo cycles with edge kinds`
   — the one place the design's explainability was flagged unfalsifiable: the
   cycle error names which edges are wiring and which are prepare
   requirements. Check: error-message tests, not just rejection tests.

Phase boundary: `.#ci`.

## Phase 5 — the generated surface

Commits:

1. `nix: derive project verbs from surface.verbs with a reserved control namespace`
   — one app per exported task id; `run`, `ps`, `down`, `clean`, `admit`
   reserved; dangling verb or colliding task name = eval error.
   Check: negative eval fixtures + build one example's apps.
2. `nix: rename the generated check app to admit and free the verb`
   — check: build the generated apps; grep the scaffold output.
3. `nix: update the install and upgrade scaffolds to the new surface`
   — check: the gate's `adoption` stage.

Phase boundary: `.#ci`.

## Phase 6 — adapters and examples in final form

Commits:

1. `nix: rewrite the postgres adapter on the new algebra`
   — invocations in lifecycle positions, prepare-as-task for init, no execs,
   no hand bindings, no membership; smoke task remains a named definition
   (the converse-reuse pattern). Check: gate's postgres example stage.
2. `nix: rewrite the reth adapter on the new algebra`
   — check: gate's example stage with the reth-consuming example.
3. `examples: rewrite downstream and the small examples on the new algebra`
   — check: gate's example + `slots` stages.
4. `examples: add the toolchain-shaped standing example`
   — heterogeneous per-leaf `requires`, a real multi-tool PATH, nested
   composites, and one deliberately retry-shaped leaf documenting in-line
   where STATIC-1's boundary is and what the sanctioned escape looks like.
   Check: gate runs it like every other example.
5. `examples: add the endpoint-less worker service to the gate`
   — no listener, invocation-probed readiness, `connectsTo` postgres, an e2e
   leaf that `requires` it; may live inside the toolchain example.
   Check: gate stage.
6. `nix: extend the negative gate over the new validation rules`
   — every rule added in phases 1–5 gets a fail-closed proof.
   Check: the gate's `negative` stage.

Phase boundary: `.#ci`.

## Phase 7 — documentation and acceptance

The design and its reasons become the recorded contract; the stale docs die.

Commits:

1. `docs: record the algebra and its reasoning in ARCHITECTURE.md`
   — a condensed section: the wire format is not the authoring surface;
   vocabulary as names over a closed algebra; why two kinds; why invocations
   are anonymous; why membership died; why durable ≠ listening. The "what v1
   taught us" table gains its new rows (the mfm adoption lesson; the
   endpoint-requirement lesson). `DESIGN_COMPOSITION.md` remains the decision
   record.
2. `docs: update AGENTS.md invariants for the composition design`
   — add KIND-2, INVOKE-1, STATIC-1, DERIVE-1, VERB-1; SURFACE-1 split into
   its control/project halves; PORT-1 restated scoped to declared endpoints;
   the hermetic child-env semantic recorded; the deferred list re-cut
   (membership and workflows no longer exist to defer against; durable
   lifetimes/leases stay deferred and are now the only home of the "stack up"
   surface).
3. `docs: rewrite ADAPTERS.md for the new adapter shape`
   — the provides-table loses Execs and Environment rows; gains the
   smoke-task-as-named-task convention and the endpoint-less guidance.
4. `docs: rewrite the README adopter story`
   — leaves, composites, `surface.verbs`, `admit`.

The acceptance run (no repo commit): rewrite `../mfm/nixfied.nix` for real
against the final rev and check every row of the design's dissolution map plus
the residue ledger — 0 lines of mfm-authored shell; 0 hand-synced derivable
facts; no `mkForce`; flake overrides reduced to mfm's own package/app;
`surface.verbs` of three; the stale port window the only surviving residue
(B3, tracked separately). The mfm diff is the acceptance artifact.

Final validation: `.#ci`.

## Risks and mitigations

| Risk | Phase | Mitigation |
| --- | --- | --- |
| Dual-derivation drift (DERIVE-1) | 3 | normative spec + golden vectors written *first* (phase 0), fixtures on both sides, gate view-diff, fail-closed compare at admission |
| Evidence-identity churn (step paths) | 2 | priced explicitly; summary/registry schema and snapshot guards land with the flattening commit, not after |
| Heterogeneous-cycle error UX (prepare-requires) | 4 | error-message tests, not just rejection tests |
| Hermetic-env behavior change (lost `${VAR:-}` append) | 1 | decided deliberately in its own commit, recorded as a semantic in phase 7 |
| Interim old-style remnants mid-phase confusing review | 1–3 | the commit sequence names what is intentionally still old-style; deletions are their own slices |
| Scope creep into deferred land (leases, memoization, multi-env, B3) | all | named non-goals below |

## Non-goals

Unchanged from the design's out-of-scope list: B3 (staleness signal — needs
the ABI-keyed capability-descriptor channel, tracked separately), durable
service lifetimes (`until-idle`/`persistent-until-down` — the only home of the
"dev stack up" surface), cross-reference memoization within a run, multiple
environments, per-tool effects granularity. None of these gate the release.

## Done means

The final `.#ci` is green; the gate runs every example including the two new
standing ones; the negative stage proves every new validation rule fails
closed; the real mfm rewrite reproduces the dissolution map with only the B3
residue; AGENTS.md, ARCHITECTURE.md, ADAPTERS.md, and README describe the
shipped system and record why it is shaped this way; adopters see exactly one
ABI rotation.
