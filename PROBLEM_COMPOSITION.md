# PROBLEM: the model speaks the runtime's vocabulary, not the adopter's

Status: implementation-aware problem statement, derived from the first real
external adoption (`../mfm`) and revised after an independent architectural
review of the first draft. The first draft decomposed the trouble into two
faces — "composition openness" and "semantic alignment" — and crowned the
single-`dev` environment gate the principal inducer. Both framings survive
below as evidence structure, but both are now understood as views of **one**
deeper absence, and the gate is demoted from cause to symptom. The document
deliberately does **not** prescribe the final design; it ends with the
direction under evaluation and the questions that direction must answer.
**Resolution:** the direction was developed and accepted — see
`DESIGN_COMPOSITION.md` for the resulting task–service algebra and the logic
behind each decision.

Review basis:

- `README.md` (the advertised adopter surface)
- `AGENTS.md` / `docs/ARCHITECTURE.md` (invariants: RUNTIME-GENERIC-1, NIX-API-1,
  SURFACE-1, SEAM-1, MODEL-SEAM-1)
- `nix/modules/primitives.nix`, `nix/modules/project.nix`, `nix/project-apps.nix`
- `nix/compiler/validate.nix` (the `["dev"]`-only environment gate)
- `nix/adapters/{postgres,reth}.nix`, `docs/ADAPTERS.md`,
  `examples/downstream/nixfied.nix`
- `runtime/crates/nixfied-runtime/src/execution/lower.rs`,
  `runtime/crates/nixfied-runtime/src/service/process.rs` (substitution + lowering),
  `runtime/crates/nixfied-runtime/src/main.rs:258-265` (run selection)
- `runtime/crates/nixfied-runtime/src/{slot.rs,state/placement.rs,state/marker.rs}`
  (the runtime is already keyed by environment)
- the deferred lists in `AGENTS.md` ("Deferred"), `docs/ARCHITECTURE.md`
  ("Deferred by design"), and `docs/ADAPTERS.md`
- the adopter under review: `../mfm/nixfied.nix`, `../mfm/flake.nix`,
  `../mfm/flake.lock` (pinned to nixfied rev `edfe201`)

## One Sentence

Nixfied has one typed vocabulary where its thesis requires two: the authoring
surface is a field-for-field transcription of the runtime's wire contract, the
concepts adopters actually think in — a *command*, a *toolchain*, a *phase* —
have no home anywhere in the ontology, and so everything an adopter needs to
say in those terms escapes the model in exactly two directions: **below** it,
into an opaque shell closure (the 110-line dispatcher), or **above** it, into
their own `flake.nix` (the verb reimplementation, the duplicated toolchain
pin). mfm did both; every symptom in this document is one of those two escapes.

## Non-Claim

The core is not broken. The adapter model works: `../mfm/nixfied.nix:165-168`
imports Postgres and Reth and writes **zero** lifecycle, probe, or cleanup code;
multi-slot isolation, deterministic port windows, and the workflow DAG all come
for free. This problem statement is about the **seam between the framework and the
adopter**, not the runtime engine or the Nix↔Rust split.

## The root flaw

The thesis — "typed Nix is the authority for what the project is *allowed to
be*" (`README.md:22-24`, NIX-API-1, MODEL-SEAM-1) — presumes a Nix layer that
speaks the *adopter's* language and compiles it down to the runtime's
language. What was built is the runtime's input format exposed raw as the
authoring surface: `nix/modules/primitives.nix` mirrors the wire contract
field for field (closures, execs, operationIds, terminal tokens, effects —
runtime bookkeeping, every one), and there is no lowering distance between
what an adopter writes and what the runtime eats. The only existing examples
of "compile an adopter concept down to primitives" are the service adapters —
framework-authored, and service-shaped.

The adopter-vocabulary layer is missing, and each missing noun produces its
own symptom class:

- **command** (a named invocation + its tools + its wiring) is missing →
  behavior sinks below the seam into a shell dispatcher (A1, A2);
- **toolchain** (the tool set a command needs on PATH; also the project's
  build toolchain) is missing → `writeShellApplication { runtimeInputs }` is
  the only assembler (A1) and the pin duplicates between flake and model (B4);
- **phase** (a selection of services + tasks + ordering, bound to a verb) is
  missing → membership smears across `environments.*` and
  `workflow.servicesRequired` (A3, the environment⇄workflow overlap) and the
  generated verbs collide with the adopter's (B2).

The first draft's two faces are this one absence seen from two sides:
"composition openness" is the missing *downward* compilation (adopter
concepts → primitives); "semantic alignment" is the missing *upward*
projection (primitives → adopter verbs). They never needed a joint solution;
they need the same missing layer.

What survives from the first draft unchanged: the shell wrapper is the door.
`docs/ADAPTERS.md:36-40` blesses `writeShellApplication` for one
non-idempotent `initdb`; an adopter authoring a task *matrix* pushes the whole
matrix through the same hole. That is a sanctioned pattern outgrowing its
scope, not a deferral — the distinction matters because the fix is not
"un-defer a feature."

## The decisive evidence: mfm typed everything it could

mfm did not refuse the model — it typed everything the model let it type. It
declared ten tasks (`../mfm/nixfied.nix:218-233`) and three workflows, one
task per command, each passing its command name as `args[0]`
(`mfmTask`, `:135-145`). The shell dispatcher's `case` arms (`:91-131`) are
1:1 shadows of those typed tasks: the dispatch **keys** are already in the
model; only the binding of key → tools + env + argv had nowhere typed to
live, so it sank below the seam. The model is one string away from owning the
interface. This rules out "the adopter just prefers shell" as an explanation —
the adopter wanted to be modelled and the vocabulary wasn't there.

## Evidence from the mfm adoption

| Symptom | Evidence | What it reveals |
| --- | --- | --- |
| 110-line bash dispatcher owns the real behavior | `../mfm/nixfied.nix:23-133` | mfm's `clippy -D warnings`, `nextest`, contract + parity suites are string `case` arms, invisible to the model. The model is a launcher. |
| Port→env mapping rebuilt in shell | `../mfm/nixfied.nix:78,88` passes `--postgres-port ${port}` then reassembles `DATABASE_URL` / `MFM_EVM_RPC_SOURCES_JSON` in bash | The runtime already substitutes validated `${port:postgres}` into **env** (`process.rs:2108`, lowering rejects undeclared named refs at `lower.rs:574-580`). Capability unused. |
| `lib.mkForce` on the environment | `../mfm/nixfied.nix:235-241` | Adapters self-register into `dev` (`reth.nix:239-242`, `postgres.nix:283`); list-merge drags their smoke tasks in. Forcing it out also drops the adapters' service registration, so mfm re-lists `postgres`/`reth` by hand. |
| `operationBindings` hand-synced | `../mfm/nixfied.nix:194-205` re-lists 10 `task.mfm.*.run` ids already declared as task `operationId`s | An allowlist fully derivable from the exec graph, maintained by hand. |
| Stale pin + dead workaround | `../mfm/flake.lock` pins `edfe201`; `../mfm/nixfied.nix:180-187` hand-tunes a port window for a Reth aux-port bug **already fixed** at `reth.nix:219-224` | No signal tells the adopter their pin predates the fix that obsoletes their workaround. |
| Verb collision + toolchain dup | `../mfm/flake.nix:80-106` reimplements `check`/`ci`; toolchain pinned twice (`flake.nix:38` and `nixfied.nix:8-13`) | Generated `.#check` = admission-only (`project-apps.nix:33`); mfm `check` = lint. `projectApps` hardcodes `ci → --workflow test` (`project-apps.nix:39-42`). |

Counter-example that proves the gap is ours: `examples/downstream/nixfied.nix`
*does* use named-port env substitution (`:226-231`) and merges the adapter's dev
tasks cleanly (`:251-260`) — because it happens to *want* the smoke task and has
homogeneous per-task deps. mfm's shape (heterogeneous deps per task, a real
toolchain) is the common case we never demonstrate.

## The altitude correction

One claim in the first draft (and in AGENTS.md) must be corrected before any
fix is designed. "Pull behavior into the model" and "invalid intent never
compiles" overstate what a model can ever do: the model cannot see inside a
binary, and even a perfectly factored mfm still carries
`cargo test -p mfm-integration-tests …` as an opaque string. Every system
bottoms out in opaque executables; the design question is only the
**altitude** at which the typed surface stops. Today it stops one level too
high — a whole-project dispatcher is a single opaque blob. The achievable
claim is model ownership of the **interface**: command identity, tool set,
wiring, selection, verbs. Behavior below the leaf was never on the table, and
the single-seam doctrine is sound once the claim is stated at that altitude.

## Problem A — composition openness (the downward face)

### A1. No first-class tool-environment composition (the door)

A closure binds exactly one executable (`primitives.nix:299`); `execType` has
no PATH/inputs field (`primitives.nix:248-291`). A cargo task needs
`cargo + nextest + git + pkg-config + toolchain + cc` on PATH
(`../mfm/nixfied.nix:25-34`). The only way to assemble that is a
`writeShellApplication { runtimeInputs = [...]; }` — and that wrapper then
becomes the natural home for dispatch and parsing. **This is the door that
opens all the others.** It is sanctioned (`docs/ADAPTERS.md:36-40`), validated
for an adapter authoring **one** service, and never re-scoped for an adopter
authoring a task matrix. Constraint: keep RUNTIME-GENERIC-1 (the runtime
learns no domain) and SEAM-1 (no Nix at run time) while letting an adopter
declare a *set* of tools the model can see.

### A2. Exec reuse vs. per-task wiring — exec is a mis-factored middle

`execs` are reusable specs; `tasks` reference one `execId`
(`primitives.nix:346-387`). A single shared exec cannot carry
`${port:postgres}` in its env, because tasks that don't depend on postgres
would fail the undeclared-named-ref check (`lower.rs:574-580`). So an adopter
with heterogeneous per-task deps either writes one exec per task (and loses
reuse) or pushes the wiring into shell (mfm's choice).

The deeper reading: `exec` carries per-invocation facts (env, cwd, timeout)
but is positioned as a per-program reuse point. In practice execs are ~1:1
with closures everywhere in the tree, except where one is minted solely to
carry env (`examples/downstream/nixfied.nix:226-231`). Reuse is an
*authoring-side* concern that Nix functions already provide (`mkAppService`
in the downstream example); the model does not need a reuse concept, and the
seam should carry fully-applied instances. The reuse boundary and the wiring
boundary are entangled because the middle concept should not exist.

### A3. Adapters self-register into environments

`environments.dev.tasks` is `listOf str` (`project.nix:39-44`), so module merge
concatenates; an imported adapter injects its smoke task into every adopter's
`dev` (`postgres.nix:283-287`, `reth.nix:239-243`). Composition of membership
should be the adopter's call — adapters should contribute service/task
**definitions**, not membership. The `mkForce` is the smell. Note the root:
adapters express "usable in dev" through the only membership slot that exists,
because the concept membership actually belongs to (a phase) is missing.

### A4. Derivable bindings declared by hand

`closure.operationBindings` (`primitives.nix:316-320`) is recoverable from
task/service `execId → closureId` plus the operation's id; the runtime already
enforces the pairing (`lower.rs:592-597`). This is the purest specimen of the
root flaw: a runtime-internal authority check exposed as a required authoring
field. Keep it as an optional explicit capability gate; stop *requiring* the
adopter to maintain it.

## Problem B — semantic alignment (the upward face)

### B1. The model becomes a launcher

When all behavior lives in one opaque exec, the authority claim is vacuous:
the intent never enters the model. Per the altitude correction, the fix pulls
the dispatch *structure* into the model — command identity, tools, wiring —
not behavior; leaves stay opaque, and that is the honest floor. We must create
*pressure* toward typed expression, not merely *permit* it.

### B2. Verb and workflow-shape mismatch

Generated `.#check` means cheap admission (`project-apps.nix:33`); the
adopter's `check` means fmt/clippy/contracts. Same verb, opposite semantics,
on the public surface. `projectApps` also assumes one canonical `test`
workflow and a fixed `ci = check + test` (`project-apps.nix:36-42`), so an
adopter with three distinct workflows reimplements the surface in its flake.

The sharpened reading: SURFACE-1 conflates two different things — the
runtime's *control* verbs (`run`/`ps`/`down`/`clean`) being framework-owned,
which is correct, and the project's *verification* verbs being
framework-**named**, which is the collision. The framework invents verbs in
its own vocabulary because the model has no adopter concept (a phase, a named
top-level selection) to derive verbs *from*.

### B3. No staleness / upgrade feedback (independent)

`nix run .#upgrade` is push-only. Nothing tells an adopter their pin predates
a fix that obsoletes a local workaround (the Reth case). ABI-1 already rotates
a digest on contract change; there may be a cheap signal ("your model was
built against an older capability descriptor") we are not surfacing. This
symptom is **independent of the root flaw** — a feedback-channel gap any
pinned-dependency system has, sharpened by ABI-1's no-migration stance. It
stays in this document because the mfm evidence surfaced it, but no
vocabulary/algebra fix will touch it.

### B4. No single source for shared build identity

The Rust toolchain is pinned in both `flake.nix:38` and `nixfied.nix:8-13` and
can drift silently. "The project's toolchain" is an adopter concept; the model
has no noun for it, so it cannot be the single source.

## Deferrals, re-attributed

The first draft crowned the single-`dev` gate (`validate.nix:97-99`) "the
principal inducer." Demoted: the gate is the most *visible* place the missing
phase concept has no home, not the cause. Two corrections:

1. **Un-gating environments as phases would re-commit the root error.**
   `environment` is the runtime's *isolation key* — state roots are
   `${projectId}/${environment}/${slot}` (`placement.rs:65`), and slots,
   markers, and service identity all hang off it — while a phase wants
   *different membership over shared state*. Concretely: mfm keeps
   `CARGO_TARGET_DIR` under `${stateDir}` (`../mfm/nixfied.nix:68-69`); model
   `check`/`ci` as environments and every phase gets a disjoint state root and
   recompiles the workspace from scratch. Phase ≠ environment. The first
   draft's proposed dissolution (`environments.check = …; environments.ci = …`)
   would force the adopter's phase vocabulary into a runtime isolation
   concept — the same category error, one level up.

2. **The environment⇄workflow overlap is second-order, not the unifying
   thread.** `environment` (isolation + membership, `project.nix:29-49`) and
   `workflow` (execution plan + `servicesRequired`, `primitives.nix:434-469`)
   are both *runtime* concepts; the adopter's phase is neither, so it
   oscillates between them and leaks into `servicesRequired`
   (`../mfm/nixfied.nix:268-271`). The overlap is what a missing concept looks
   like when two adjacent concepts cover for it. Asking which of the two
   "owns membership" is answering the wrong question — the direction below
   answers it by *deleting* membership instead.

Still standing from the first draft:

- **The existence proof.** Multi-endpoint port planning was deferred
  (`docs/ADAPTERS.md` once called it a known limitation), mfm hand-tuned a
  port window around it (`../mfm/nixfied.nix:180-187`), and implementing it
  (`primitives.nix:197-216`, `reth.nix:219-224`) **erased** the symptom class
  — while leaving mfm carrying a stale dead workaround because no staleness
  signal exists (B3).
- The other deferred items (secrets, source modes, `dirtyPolicy`, manifest
  envelope, multi-host, daemon, UI): **not implicated**, orthogonal.
  `logs`/`up`/`validate --deep` amplify B1 (no view into opaque-task output)
  but do not cause it. Service reuse / `until-idle` lifetimes: efficiency, not
  composition — mfm re-spawns postgres+reth per workflow run.

## Why this went undetected: the verification methodology

Before mfm, every adopter was an example the framework authored and graded
itself (the gate, `AGENTS.md`). Every example executable is purpose-built to
be expressible in the primitives: the downstream app is one self-contained
python file taking `--host/--port` (`examples/downstream/nixfied.nix:18-76`);
psql, curl, and pg_isready are single binaries. A proof system whose test
authors already think in the runtime's vocabulary cannot detect a vocabulary
gap by construction; the gate's `adoption` check exercises `install`/`upgrade`
*mechanics*, never authoring. The gate validated the runtime exhaustively and
the authoring surface never. Whatever fix lands, the gate must gain a standing
proof that a *toolchain-shaped* project (heterogeneous per-task deps, real
multi-tool PATH) stays expressible — a daemon-shaped example can never catch
this class again.

## Invariant pressure

- **SEAM-1, RUNTIME-GENERIC-1, MODEL-SEAM-1, HASH-1, MODEL-ORIGIN-1** —
  load-bearing, untouched by this problem; any fix must hold them unchanged.
- **ABI-1** — load-bearing, but it sharpens B3: pins rot silently and the only
  signal is hard rejection at admission. It owes adopters a staleness channel,
  not a migration layer.
- **SURFACE-1** — actively harmful as currently interpreted (see B2). Split
  it: control verbs framework-owned; project verbs project-derived.
- **NIX-API-1** — vacuous in practice: "Nix modules are the integration API"
  never required the API to speak adopter vocabulary. The absence of that
  requirement is this document. The invariant needs strengthening, not
  relaxing.

## What a unified solution must satisfy (constraints, not design)

1. **Preserves the invariants.** RUNTIME-GENERIC-1, SEAM-1, NIX-API-1,
   MODEL-SEAM-1 hold unchanged; SURFACE-1 holds in its corrected split form.
   The runtime stays domain-blind and never invokes Nix; `model.json` stays
   the only semantic seam.
2. **Pulls the interface into the model.** After the fix, mfm's command
   identity, tool sets, per-command wiring, and selection structure are
   visible, typed, and diffable in the model. Leaves stay opaque — that is the
   honest floor, per the altitude correction.
3. **Composes tool environments** without a per-task shell wrapper, and without
   the runtime gaining domain knowledge (the tool *set* is realised Nix-side;
   the runtime only verifies/execs generic store paths).
4. **Composes membership** so importing an adapter contributes definitions,
   never membership; `mkForce` disappears from honest adoptions.
5. **Keeps wiring validated per task** (named-port/host/stateDir substitution,
   undeclared-ref = admission error) without the reuse-vs-wiring trade-off.
6. **Derives verbs from the adopter's names**; framework verbs live in a
   reserved control namespace and can never collide.
7. **Single source of truth** for any fact shared between the build
   (`flake.nix`) and the model (`nixfied.nix`).
8. **Membership, ordering, and isolation each have exactly one owner** — and
   that owner need not be `environment` or `workflow` as they exist today.
9. **The public API must not grow adopter vocabulary as schema.** No
   `command`/`toolchain`/`phase` nouns hard-coded into the contract with their
   own semantics. The contract may grow at most a small, closed *algebra*;
   adopter vocabulary must be expressible as *names over* that algebra (attr
   keys, derived surfaces) — otherwise the schema chases every project shape
   forever.
10. **The next adoption looks like `downstream`, not like a launcher around
    bash** — and the gate proves it stays that way (see the methodology
    section).

## Direction under evaluation: a two-kind algebra

Not a design — the hypothesis the design document must confirm or break.

The model's semantic kinds reduce to **two**:

- **task** — bounded, *composable* execution. A task is either a **leaf**
  (tool set + executable + argv + env + cwd + service requirements + exit
  policy — today's `exec`+`task` collapsed into one, with tools added and the
  exec middle deleted) or a **composite** (a DAG of child task references —
  today's `workflow`, generalized to nest). `servicesRequired` becomes
  *derived*: the union of transitive leaf requirements. `operationBindings`
  becomes derived from the leaf graph. Reuse and parameterization are
  Nix-side (functions, modules); the model carries fully-applied instances.
- **service** — durable execution: lifecycle, endpoints, readiness, identity,
  state, cleanup — exactly the part the Non-Claim says works, unchanged.

Under this algebra the missing nouns become names, not schema:

| Adopter concept | Becomes |
| --- | --- |
| command | a leaf task |
| toolchain | a leaf's tool set (realised Nix-side into store paths; the runtime only assembles PATH — generic data) |
| workflow | a composite task |
| phase / verb | the **name** of a top-level task; the generated surface derives one app per top-level task, plus a reserved control namespace |
| environment | demoted to pure isolation namespace (an invocation parameter); **membership dies** |

Mapped to the symptoms: the dispatcher → leaves with `tools` (A1); shell port
wiring → per-leaf env with named refs, validated per leaf (A2, and `exec`'s
entanglement dies with `exec`); `mkForce` → nothing to force, adapters
contribute only definitions (A3 — fixed *without* un-gating
multi-environment); hand-synced bindings → derived (A4); verb collision →
verbs derived from top-level task names (B2); toolchain duplication → one
Nix value consumed by both flake and model (B4). B3 remains independent.

Load-bearing questions the design must answer:

- **Two kinds, not one.** Why is a service not "a durable task"? Because the
  contracts differ at the type level: exit-as-result vs exit-as-failure,
  endpoints/readiness/identity/reuse, marker-gated cleanup. The current
  lifecycle algebra's "illegal binding is unrepresentable"
  (`primitives.nix:34-35`) is the argument for keeping them apart. The burden
  of proof sits on further unification, not on two.
- **Composite semantics.** Failure/cancellation = today's workflow semantics
  applied recursively? Service start = eager union upfront (today's behavior)
  or per-leaf lazy? Eager is simpler and preserves observed semantics.
- **The line against a workflow language.** Composites stay static,
  fully-applied DAGs in the model: no parameters, conditionals, retries, or
  loops in the contract — dynamism is Nix-side expansion or inside an opaque
  leaf. Where exactly does that line bind, and what is refused at eval?
- **Carrying `tools` through the seam.** A realised closure set whose `bin/`
  roots the runtime prepends to PATH, with admission verifying each element —
  generic, SEAM-1/RUNTIME-GENERIC-1 clean. Ceremony: operationIds and terminal
  tokens should be derivable by default, declared only to override.
- **The durable-stack residue.** With membership deleted, "bring the dev stack
  up and leave it up" has no model noun. It belongs with the deferred lease
  work (`until-idle`/`persistent-until-down`) as an *invocation* surface
  (`run --service …` or a service-group argument), not a model concept.
  Confirm this is acceptable, since today only `run-scoped` exists anyway.
- **ABI impact.** The algebra goes through the seam — `model.json` shows the
  *authored* structure (named leaves, named composites), so the model remains
  the authority and views/diffs speak the adopter's names. The ABI rotates
  once (ABI-1 makes that clean); the runtime gains *structure* (PATH assembly,
  composite lowering onto the existing flat plan in `plan.rs`), never
  vocabulary.

## Out of scope

- The Nix↔Rust verb split and the runtime engine internals — not implicated.
- Most of the deferred scope in `AGENTS.md` (secrets, service reuse, additional
  source modes, `dirtyPolicy`, manifest envelope) — orthogonal to this flaw.
- **Multiple environments** — corrected twice now: the first draft moved it
  in scope as "the principal inducer"; this revision moves it back **out**.
  Under the two-kind algebra, A3/B2 are fixed by deleting membership, not by
  multiplying environments; `environment` remains a pure isolation namespace
  and stays deferred on its own (weaker) merits.
- **B3 (staleness signal)** — in scope as a real problem, out of scope for
  the algebra: it needs a feedback channel keyed off ABI-1, not a model
  change.
