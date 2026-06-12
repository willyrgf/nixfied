# DESIGN: the task–service algebra

Status: accepted design, resolving `PROBLEM_COMPOSITION.md`. That document is
the problem statement and evidence base; this one records the decided
architecture and the reasoning behind each decision. It is a **full rewrite**
of the model's semantic surface — no compatibility layer, no migration, one
ABI rotation (ABI-1 makes that clean). Implementation order and task breakdown
are out of scope here; this is the contract the implementation must satisfy.

Reading order: `PROBLEM_COMPOSITION.md` (why) → this file (what and the logic
behind it) → `docs/ARCHITECTURE.md` / `AGENTS.md` (the standing invariants this
design preserves).

## Decision in one paragraph

The model's semantic kinds reduce to **two** — **task** (bounded, composable
execution) and **service** (durable execution: orchestrated, probed, owned,
cleaned) — connected by one shared structural type, the **invocation** (tools +
argv + env + cwd: the one way anything in the model says "run this program").
Adopter vocabulary (a *command*, a *toolchain*, a *workflow*, a *phase*, a
*verb*) enters the contract as **names over this algebra, never as schema**:
`check` is not a concept Nixfied knows, it is the name an adopter gave a
composite task, and the generated flake surface derives its apps from those
names. Everything else follows from holding that line.

## The reasoning chain

Each step below is forced by the previous one; together they contextualise
every decision in this document.

1. **The root flaw** (`PROBLEM_COMPOSITION.md`): the authoring surface is the
   runtime's wire format, so adopter concepts with no runtime equivalent
   escape the model — below it into shell, above it into the adopter's flake.
   The fix must give those concepts a typed home.
2. **The altitude correction**: the model can never own *behavior* (every
   system bottoms out in opaque executables); it can own the **interface** —
   command identity, tool sets, wiring, selection, verbs. The design targets
   exactly that altitude: leaves are opaque, and that is the honest floor.
3. **No vocabulary as schema** (constraint 9 of the problem doc): hard-coding
   `command`/`toolchain`/`phase` nouns into the contract makes the schema
   chase every project shape forever. Therefore the contract grows a small,
   *closed* algebra, and vocabulary becomes naming.
4. **The minimal algebra is two kinds**, because bounded and durable execution
   have genuinely different contracts (exit-as-result vs exit-as-failure;
   endpoints/readiness/identity; marker-gated state). One kind would smear
   them; three or more is vocabulary creep. The burden of proof sits on any
   future unification or addition.
5. **Composition belongs to task**, because ordering is contextual (a
   pipeline's choice) while service readiness is intrinsic (a leaf's wiring
   fact). This split is why `servicesRequired` becomes derivable and why
   `dependsOn` and `requires` both exist (see below).
6. **Reuse belongs to Nix, not to the model.** The model carries fully-applied
   instances; functions/modules/`let` provide abstraction at authoring time.
   This kills `exec` (a shared *instance* that entangled reuse with wiring —
   problem A2) and replaces it with `invocation`, a shared *type*.

## The algebra

```
closure    = store path + executable + effects            (attestation record)
invocation = tools + run + env + cwd + timeout + stdin    (a TYPE — inline only, never named)
task       = leaf      { invocation, requires, exitPolicy, refs }
           | composite { steps : name → { task ref, dependsOn } }
service    = endpoints? + state + identity             (endpoints optional — durable ≠ listening)
           + lifecycle( prepare : task ref            — full task semantics, composites allowed
                      , start   : invocation + containment
                      , ready   : probe(invocation | tcp, retry)
                      , health  : probe(invocation | tcp, retry)
                      , stop    : signal policy
                      , clean   : runtime primitive )
```

Vocabulary mapping (names, not schema):

| Adopter concept | Becomes |
| --- | --- |
| command | a leaf task |
| toolchain | a leaf invocation's `tools` (realised Nix-side into store paths; the runtime only assembles PATH — generic data) |
| workflow | a composite task |
| phase / verb | the **name** of a task the adopter exports to the surface |
| environment | demoted to a pure isolation namespace (an invocation parameter of the runtime); **membership no longer exists in the model** |

### closure — the attestation record

Unchanged in role, reduced in ceremony: store path + executable +
`effects`. `operationBindings` is **derived** from the graph (which
invocations reference the closure, for which operations); `effects` remains
the one hand-declared field, deliberately — it is the one thing only a human
can attest (what this binary is expected to do to the world: `process`,
`network-listener`, `source-read`, `file-write`). Today effects are
declarative only (`nixfied-model/src/types.rs:157`; the runtime never reads
them); a first cheap enforcement lands with this design: a service start
closure lacking `network-listener` contradicts endpoint-ownership
verification and is rejected at admission.

### invocation — a shared type, never a registry

`{ tools, run, env, cwd, timeoutMs, stdin }`. The factoring exists because
"task" was doing two jobs: a **mechanism** (what to run) and an
**orchestration contract** (when it may run, what its exit means, what
evidence it leaves). Service lifecycle positions need the mechanism with
*position-owned* semantics, so the mechanism is its own type:

- a **leaf task** = invocation + orchestration (`requires`, `exitPolicy`, refs)
- a **probe** = invocation + retry policy
- a **start op** = invocation + containment
- **stop/clean** = no invocation at all (signal policy / runtime primitive)

**INVOKE-1 (new invariant): invocations are anonymous and inline in the wire
format. No invocation ids, no invocation registry.** Naming them for reuse
recreates `exec` and re-imports problem A2 (shared instance + heterogeneous
per-position wiring = scope conflicts). `exec` failed as a shared *instance*;
`invocation` succeeds as a shared *type*. Content reuse is a Nix `let`; the
model carries the applied copies, duplicated and diffable — consistent with
"the model is the fully-applied form."

`tools` is a list of realised store paths whose `bin/` roots form the
invocation's PATH. `run` is argv; `run[0]` is resolved against the tool set
**at eval** and the model carries both the resolved executable store path and
the PATH set, so admission only verifies (existence, executability, target,
declaration) and the runtime resolves nothing on the host — SEAM-1 and
PREPARE-1 hold exactly as today.

### task — leaf or composite

A **leaf** is today's `exec`+`task` collapsed into one fully-applied record.
A **composite** generalises today's workflow body (`primitives.nix:443-462`)
— a named-step DAG — with one change: a step may reference *any* task, leaf
or composite. Steps are named nodes referencing tasks (not a set of task
refs): the same task may legitimately appear twice in one composite, and
evidence (logs, summaries, registry) needs stable step names.

Rejected alternatives, for the record: inline recursive trees (awkward
recursive module types, schema recursion on the wire, anonymous children with
no evidence identity) and `seq`/`par` combinators in the contract (a DAG
subsumes both; they are **Nix-side sugar** — `nixfied.lib.seq [ … ]` compiles
to a `dependsOn` chain).

### Why both `dependsOn` and `requires`

They relate different kinds and mean different things; neither can absorb the
other:

| | `requires` (leaf → service) | `dependsOn` (step → step) |
| --- | --- | --- |
| Means | "must be **ready** (alive, probed, addressable) *while* I run" | "must have **completed successfully** *before* I start" |
| Drives | resource semantics: service startup, the derived service union, and placeholder scoping — `${port:postgres}` in a leaf is valid only because `requires` declares it | control flow: ordering and failure propagation (a failed step cancels dependents) |
| Belongs to | the leaf — **intrinsic**; the requirement travels with the task | the composite — **contextual**; "clippy after fmt" is one pipeline's choice, not a property of clippy |

Collapsing them (task-requires-task) would bake one pipeline's ordering into
the task globally and conflate an ongoing state ("ready") with a terminal
event ("succeeded"). Keeping them apart is also what makes the service union
derivable: leaf `requires` is a leaf-intrinsic fact.

### service — durable execution, unchanged where it works

The lifecycle algebra keeps its house style: each position binds exactly the
mechanism its semantics need, so illegal states are **unrepresentable** rather
than rule-checked (the principle at `primitives.nix:34-35`):

| Position | Carries | Why this and not a full task |
| --- | --- | --- |
| `prepare` | a **task reference** (leaf *or composite*) | Genuinely task-shaped: bounded, once, success/failure. Full semantics buys **typed cross-service initialization** — `api.lifecycle.prepare.task = "api-migrate"` where the migrate leaf `requires = [ "postgres" ]` — and multi-step init (create → migrate → seed) is just composition. |
| `start` | invocation + **containment** | The runtime must *own* this process indefinitely: stop signals, OS-reconciled liveness, cleanup gates all depend on the ownership boundary (`process-group` vs `process-tree`, enforced at `process.rs:492-512`, part of layered identity). A bounded task has no such contract. |
| `ready` / `health` | **probe** = invocation + retry policy (or `tcp`) | A probe is a *retried mechanism*, not a bounded run: per-attempt timeout/interval/maxAttempts deliberately override the invocation's own timeout (as today, `primitives.nix:75-79`). A probe structurally cannot carry `requires` — it *defines* readiness of its target; the type makes the circularity unrepresentable. |
| `stop` | signal policy | No invocation, as today. |
| `clean` | runtime primitive | Marker-gated, path-confined, as today. |

The converse reuse is free and replaces environment membership as the way
adapter checks enter an adopter's pipeline: adapter smoke tasks
(`smoke-query`, `reth-smoke`) are ordinary named tasks the adopter references
as steps in its own composites.

### Endpoint-optional services — durable is not listening

The inherited schema conflates two properties the algebra must keep apart:
**durable** (owned, probed, contained, cleaned) and **network-addressable**.
Today a service must bind an endpoint (`primitives.nix:188-216` — `endpoint`
xor `endpoints`, "exactly one form must be set"), and the requirement cannot
be faked: PORT-1 verifies endpoint *ownership* at readiness, so a declared
but never-bound endpoint fails the ready gate. That leaves the most common
second-adopter shape — a queue consumer, an indexer, any non-listening worker
daemon — unrepresentable: it is durable execution by every criterion this
section names, and it binds no socket. Without this correction the shape is
forced below the seam (a fake listener) or out of the model entirely —
problem A1 wearing a service costume.

The fix is subtraction, not addition: KIND-2 holds, there is no third kind
and no new lifetime machinery — the endpoint requirement was an accidental
constraint imported by "unchanged where it works." A service declares **at
most one** endpoint form (was exactly one); a service with neither is
**endpoint-less**, with every consequence type- or validation-enforced in the
house style (illegal states unrepresentable, fail-closed):

- **Probes must be invocations.** A `tcp` ready/health probe on an
  endpoint-less service has no target and is rejected at eval and admission
  (the `ProbeExecOnTcp` pattern at `lower.rs:423-437`, applied one level up).
  Readiness means "the probe answers" — a heartbeat file under
  `${stateDir}`, a queue-depth query through the broker the worker connects
  to.
- **PORT-1 is restated scoped, not weakened where it applies.** Where an
  endpoint exists, readiness still requires verified ownership, unchanged.
  An endpoint-less service makes **no addressability claim at all** and
  nothing may rely on one (next rule), so there is no claim left for PORT-1
  to protect. This is a deliberate carve-out, recorded here so it cannot be
  read as a loophole.
- **Nothing can address it.** `${port:<id>}` / `${host:<id>}` naming an
  endpoint-less service is rejected in every scope placeholders are checked
  today (leaf `requires`, service `connectsTo`, probes); bare
  `${port}`/`${host}` inside its own lifecycle invocations is rejected by
  the same rule tasks already have (`lower.rs:373-388`, applied
  symmetrically to services).
- **It can still be depended on.** `requires` and `connectsTo` *toward* an
  endpoint-less service stay legal and keep their meaning — ready ordering,
  failure semantics, and the derived service union — minus addressability:
  an e2e leaf may `require` the worker so it is alive while the test runs;
  the worker itself `connectsTo` postgres and addresses it normally.
- **Effects coherence gains its converse.** The forward rule stands
  (declared endpoints ⇒ the start closure declares `network-listener`); now
  also: an endpoint-less service whose start closure declares
  `network-listener` is rejected — it announces a listener the planner
  cannot reserve, the exact bypass `docs/ADAPTERS.md` already forbids for
  unmodelled sockets.
- **Identity and planning need no special case.** The `endpointIdentity`
  layer of SVC-ID-1 hashes the empty endpoint set deterministically; the
  planner already sums per-endpoint port demand (`plan.rs:134-189`), so an
  endpoint-less service consumes no ports from the slot window.

This lands inside the same single ABI rotation as the rest of this design —
it is a wire-contract change (`endpoints` required → optional, probe-kind
coherence) — and deferring it would cost a second rotation the moment the
first non-listening adopter arrives. It is distinct from the deferred
durable-lifetime work (`until-idle`/`persistent-until-down`): leases govern
how long an admitted service lives; this governs whether the service can be
declared at all.

## What dies, and why

| Removed | Replaced by | Problem it closes |
| --- | --- | --- |
| `execs` (the registry) | inline `invocation` on each position; reuse via Nix `let`/functions | A2 — the reuse/wiring entanglement dies with the shared instance |
| `workflows` (the section) | composite tasks; `servicesRequired` **derived** (union of transitive leaf `requires`, closed over `connectsTo`) | the environment⇄workflow overlap loses one of its two halves |
| environment **membership** (`environments.<e>.{services,tasks}`) | nothing — running a task brings up exactly the services its leaves require; `environment` survives only as the runtime's isolation namespace (state roots, slots, registry key) | A3 (`mkForce` has nothing to force; adapters contribute only definitions) and B2's phase half — **without** un-gating multi-environment |
| hand-declared `operationBindings` | derived from the graph; optional explicit override remains as a narrowing gate | A4 |
| hand-declared `operationId` / terminal tokens | derived by default (`task.<name>.run`, `service.<name>.<op>`); declared only to override | ceremony of the same disease as A4 |
| framework-named project verbs (`.#check`/`.#test`/`.#ci` semantics fixed in `project-apps.nix:33-42`) | verbs derived from adopter-exported task names + a reserved control namespace | B2 |

## Semantics

- **Failure**: today's workflow cancellation semantics, applied recursively. A
  failed step cancels not-yet-started dependents; a composite succeeds iff all
  steps succeed. `exitPolicy` exists only on leaves; composites are pure
  conjunction.
- **Run-once is per step, not per task.** A task referenced from two places in
  one run executes twice, deterministically. No memoization ("already
  succeeded this run") — that is build-system semantics with a task-identity
  rabbit hole; revisit only on demonstrated adopter cost. Natural authoring
  (composites referencing composites, as mfm's `ci` referencing `check`)
  produces no duplicates anyway.
- **Service start is eager**: the derived union for the whole selected task
  tree starts upfront, preserving today's observed workflow behavior and
  keeping the plan a pure function of model + slot. Lazy per-leaf start is
  explicitly rejected for now.
- **No parameter passing**: a composite passes nothing to children — no env
  overlays, no arg injection. Parameterization is Nix-side expansion; the
  model is the fully-applied form.
- **Placeholders**: unchanged in spirit — bare `${port}`/`${host}` resolve to
  the primary requirement (tasks) / own primary endpoint (services); named
  `${port:<id>}`/`${host:<id>}` require the id in `requires` (leaves),
  `connectsTo` (services), or own endpoints; `${stateDir}` is the slot state
  root. Probe invocations resolve against the owning service's endpoints.

## Validation (eval, re-proven at admission)

- Task reference graph acyclic; step `task` refs name declared tasks;
  `dependsOn` names steps of the same composite.
- Leaf named placeholders ⊆ `requires`; undeclared ref = compile/admission
  error (the existing `lower.rs:574-580` check, per leaf).
- A service's prepare task must not `require` the owning service; the combined
  `connectsTo` + prepare-`requires` graph is acyclic.
- `run[0]` resolution is computed at eval, carried in the model, verified at
  admission; every `tools` element is verified like any closure.
- Derived facts (`servicesRequired` union, `operationBindings`,
  `operationId`s) are computed identically by the Nix compiler and the
  runtime's lowering, like the ABI digest today.
- Exported verb names must not collide with the reserved control namespace
  (eval error, fail-closed).
- A service declares at most one endpoint form. An endpoint-less service's
  ready/health probes must be invocation probes (`tcp` is an eval/admission
  error); named placeholders resolving to an endpoint-less service are
  rejected in every scope; bare `${port}`/`${host}` in its own lifecycle
  invocations are rejected (the task rule, applied to services).
- Effects coherence (first enforcement), both directions: declared endpoints
  require `network-listener` on the start closure; `network-listener` on the
  start closure of an endpoint-less service is rejected.

## The generated surface

SURFACE-1 splits into its two honest halves (the problem doc's "invariant
pressure"):

- **Reserved control namespace, framework-owned**: `run`, `ps`, `down`,
  `clean`, and `admit` (admission sanity — renamed from `check`, freeing the
  most common adopter verb). Task names colliding with these are rejected.
- **Project verbs, adopter-owned**: `nixfied.surface.verbs = [ "check" "test"
  "ci" ]` — an explicit list of task ids that become flake apps
  (`.#check` → `runtime run --model … --task check`). This is the one place
  explicit selection survives membership's death, deliberately: which tasks
  form the public surface is a *choice*, not a derivable fact — and making it
  adopter-explicit is what prevents imported adapter tasks from silently
  becoming public apps (the A3 lesson, applied to the surface).
- `.#run -- --task <id>` runs any declared task; `run` with no selection
  refuses and lists the declared tasks (no implicit default).
- The durable-stack case ("bring postgres up and leave it") is **out of
  scope** with membership: it is a *lifetime* question and lands with the
  deferred lease work (`until-idle`/`persistent-until-down`) as an invocation
  surface (`run --service …`), not a model concept. Today only `run-scoped`
  exists, so nothing is lost.

## New invariants

- **KIND-2**: the model has exactly two semantic kinds, task and service. A
  new adopter concept must be expressible as names over the algebra; growing
  the schema requires demonstrating the algebra cannot express it.
- **INVOKE-1**: invocations are inline, anonymous structural values; no
  invocation registry, no invocation ids in the wire format.
- **STATIC-1**: composites are static, fully-applied DAGs. No parameters,
  conditionals, retries, or loops in the contract — dynamism is Nix-side
  expansion or inside an opaque leaf. This is the line against growing a
  workflow language inside the seam.
- **DERIVE-1**: a fact derivable from the graph is derived
  (`servicesRequired`, `operationBindings`, operation ids). Hand-declaration
  is reserved for *choices* (surface verbs) and *attestations* (effects).
- **VERB-1**: control verbs are framework-reserved; project verbs derive only
  from adopter-exported task names. (The corrected split of SURFACE-1.)

Standing invariants preserved unchanged: SEAM-1 (resolution at eval, nothing
Nix-shaped at run time), RUNTIME-GENERIC-1 (the runtime gains *structure* —
PATH assembly, composite lowering onto the existing flat plan in `plan.rs`,
derived unions — never vocabulary or domain), MODEL-SEAM-1/SINGLE-MODEL-1,
HASH-1, MODEL-ORIGIN-1, ABI-1 (one rotation), PREPARE-1, SOURCE-1, SVC-ID-1,
PORT-1 (restated scoped to declared endpoints — see endpoint-optional
services), REG-*/LIVE-1, GC-*, PROC-*, REDACT-1, SHELL-1/NIX-1.

## Acceptance: the mfm dissolution map

The design is accepted when the mfm rewrite produces these outcomes (each row
answers an evidence row of `PROBLEM_COMPOSITION.md`):

| Problem evidence | Outcome under this design |
| --- | --- |
| 110-line shell dispatcher | ~0 lines: each `case` arm becomes a leaf (`tools` + `run`); the dispatcher's only non-dispatch content (`CARGO_TARGET_DIR`, `RUST_BACKTRACE`, darwin link env) becomes typed env attrs |
| Port→env rebuilt in shell | per-leaf env with named refs: `env.DATABASE_URL = "postgresql://postgres@${host:postgres}:${port:postgres}/postgres"` validated per leaf |
| `lib.mkForce` on `environments.dev` | nothing to force; membership does not exist |
| `operationBindings` hand-synced | derived |
| Verb collision + flake reimplementation | `.#check`/`.#ci` derived from mfm's own task names via `surface.verbs`; admission lives at `.#admit` |
| Toolchain pinned twice | one Nix value consumed by both `flake.nix` and the model's `tools` |
| Stale pin + dead workaround (B3) | **unchanged — independent**; needs the ABI-keyed staleness channel, tracked separately |

And the methodology fix that makes this class of flaw detectable forever
after: the gate gains a **toolchain-shaped example** (heterogeneous per-task
`requires`, a real multi-tool PATH, nested composites) run like every other
example — a daemon-shaped example can never catch a vocabulary gap again.
That example (or a sibling) also declares an **endpoint-less worker service**
(no listener, invocation-probed readiness, `connectsTo` a database, a leaf
that `requires` it) so both halves of the service kind — listening and
non-listening — stay provably expressible, in the same standing way.

## Out of scope / deferred

- **B3** (staleness/upgrade feedback): real, independent, needs an ABI-keyed
  capability-descriptor signal at admission — not a model change.
- **Durable service lifetimes** (`until-idle`/`persistent-until-down`, the
  "dev stack up" surface): lands with leases as invocation surface.
- **Cross-reference memoization** within a run: rejected for now (see
  Semantics); reopen only with demonstrated cost.
- **Multiple environments**: stays deferred on its own (weaker) merits as a
  pure isolation question; it is no longer on the critical path of anything
  in the problem statement.
- **Per-tool effects granularity**: tool-set members synthesized from plain
  packages default to `effects = [ "process" ]`; finer attestation per tool is
  open, low-stakes while effects enforcement is minimal.
