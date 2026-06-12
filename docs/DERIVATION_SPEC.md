# DERIVATION SPEC: the composition algebra's derived facts

Status: **normative**. Both implementations — the Nix compiler
(`nix/compiler/derive.nix`) and the runtime lowering
(`runtime/crates/nixfied-runtime/src/execution/lower.rs`) — are written
against this document, and the golden vectors in §6 land as fixtures on both
sides. This is the DERIVE-1 drift mitigation: like the ABI digest, a derived
fact is trustworthy only because two independent implementations share one
normative source. If this spec is wrong, fix the spec first, then both
implementations; neither implementation may drift from the text.

Scope: the exact algorithms for

1. composite **flattening** and stable **step paths** (§2);
2. **`servicesRequired`** of a task (§3);
3. **`operationBindings`** of a closure (§4);
4. default **operation ids** and default **terminal tokens** (§5).

Vocabulary is `DESIGN_COMPOSITION.md`'s: a *task* is a leaf
`{ invocation, requires, exitPolicy, refs }` or a composite
`{ steps : name → { task, dependsOn } }`; an *invocation* is
`{ tools, run, env, cwd, timeoutMs, stdin }`, inline and anonymous (INVOKE-1).

## 1. Identifiers and canonical order

- **Task ids, service ids, and step names** match
  `[A-Za-z0-9][A-Za-z0-9_-]*`. In particular they contain no `.`, so the
  step-path grammar in §2 is unambiguous by construction. Validated at eval
  and at admission (fail-closed).
- **Canonical order** everywhere this spec says "sorted" is byte-wise
  ascending order of the UTF-8 identifier — the order `builtins.attrNames`
  yields in Nix and `BTreeMap`/`BTreeSet` iteration yields in Rust. The two
  implementations therefore agree on ordering with no extra code; any other
  collation is a spec violation.
- **Dedup** is set semantics under byte equality.

### 1.1 `run[0]` resolution and the executable closure

An invocation's `tools` is a list of declared closure ids. Each tool closure
contributes one **PATH root**: the parent directory of its absolute
`executable`. The invocation's PATH is the roots joined in `tools` order
(first wins).

`run[0]` resolution is **declarative, not a filesystem scan** (so eval and
admission cannot drift and no realisation is needed at eval):

```
executableClosure(invocation) =
  the FIRST element c of invocation.tools
  where basename(c.executable) == invocation.run[0]

invocation.executable = executableClosure(invocation).executable   (absolute)
```

No matching tool is an eval error; admission re-derives the same rule and
requires the carried `executable` to equal it (fail closed). A program that
is on the assembled PATH but is not any tool closure's declared executable
(e.g. `rustc` inside a toolchain whose closure declares `bin/cargo`) is
reachable by child processes through PATH, but cannot be `run[0]` without its
own tool closure entry.

## 2. Flattening and step paths

A run executes one selected task. The planner flattens the (acyclic,
validated) task reference graph into the existing flat plan; evidence
identity (registry rows, log refs, summary refs) is the **step path**.

### 2.1 Step-path grammar

```
path     = rootTask                       (the selected task is a leaf)
         | rootTask "." segments          (the selected task is a composite)
segments = stepName | segments "." stepName
```

`rootTask` is the selected task's id; each `stepName` is the key of the
composite step traversed, outermost first. Because ids and step names contain
no `.`, splitting on `.` recovers the traversal exactly.

### 2.2 Flattening algorithm

```
flatten(taskId) -> ordered list of PlanNode { stepPath, leafTaskId, dependsOn: set<stepPath> }

flatten(root):
  emit(root, path=root, inherited=∅)

emit(task, path, inherited):
  if task is a leaf:
    output PlanNode { stepPath = path, leafTaskId = task.id, dependsOn = inherited }
  else:                                  # composite
    for stepName in sorted(task.steps):  # canonical order, §1
      step = task.steps[stepName]
      deps = inherited
           ∪ ⋃ { nodes(dep) | dep in step.dependsOn }   # see below
      emit(step.task, path + "." + stepName, deps)

nodes(stepName) = the set of stepPaths of every PlanNode emitted by the
                  sibling step `stepName` (the entire flattened subtree).
```

Pinned consequences:

- **Run-once is per step.** A task referenced from two steps flattens twice,
  producing two PlanNodes with distinct step paths. No memoization.
- **`dependsOn` on a composite step means the *whole* subtree.** A step
  depending on sibling `a` depends on **every** node flattened from `a`, not
  just `a`'s sinks: a composite succeeds iff all its steps succeed, so a
  dependent may start only after all of them. (Edges to sinks alone would let
  a dependent start while a parallel branch of `a` is still running or has
  failed.)
- **Emission order** is depth-first, steps in canonical order. The executable
  **plan order** is the deterministic topological order over that emitted list:
  repeatedly select the first emitted node whose dependencies have all been
  placed. This preserves canonical sibling order where there is no dependency,
  and it moves a dependency before its dependent when the byte-sorted emission
  order would otherwise put the dependent first.
- **Failure** is today's cancellation applied to the flattened graph: a
  failed node cancels every not-yet-started node that (transitively) depends
  on it. A composite "succeeds" iff all nodes under its path prefix succeed;
  no PlanNode exists for the composite itself.
- **Cycles**: the task reference graph (composite → step → task) must be
  acyclic. Eval rejects it; admission re-proves it (the existing
  workflow-cycle rejection, applied through nesting).

### 2.3 Evidence identity

Registry rows, log refs, and summaries for a flattened node are keyed by
`stepPath`. The default log ref of a node is `task.<stepPath>` and the
default summary ref is `summary` (unchanged). Two nodes never share a
`stepPath` within a run (guaranteed by the grammar plus per-composite step
name uniqueness).

## 3. `servicesRequired(task)`

The derived service set of a selected task — the services started (eagerly,
upfront) for the run, the planner's port-demand input, and the runtime's
admission-compare target.

```
servicesRequired(task):
  base  = ⋃ { leaf.requires | leaf in leaves(flatten(task)) }
  closed = least fixpoint of:
             S ∪ ⋃ { connectsTo(s) | s in S }
               ∪ ⋃ { prepareRequires(s) | s in S }
           starting from base
  return sorted(closed)        # canonical order, §1; dedup is set semantics

prepareRequires(s) = ⋃ { leaf.requires | leaf in leaves(flatten(prepareTask(s))) }
                     (∅ when s binds no prepare task)
```

The closure pulls in **prepare requirements** as well as wiring: starting a
service means running its prepare task first, and that task's leaves may
require other services (typed cross-service initialization), so they belong
to the union. The combined `connectsTo` ∪ prepare-requires edge set must be
acyclic (validated at eval, re-proven at admission with the edge kinds named
in the error).

Pinned consequences:

- The result is the **transitive** closure over both edge kinds: if a leaf requires
  `api`, `api` connectsTo `postgres`, and `postgres` connectsTo nothing, the
  set is `[ "api", "postgres" ]`.
- Ordering is canonical (§1) — **not** declaration or discovery order. Port
  assignment consumes the sorted list, so a service's port within a slot
  window depends only on the set's membership, never on authoring order.
- A task whose leaves require nothing yields `[]`; the run starts no
  services.
- `requires` targets and `connectsTo` targets must name declared services
  (eval + admission error otherwise; the existing undeclared-reference
  checks).
- The runtime re-derives this set at admission and compares it with the
  model-carried value; a mismatch is `MODEL_ADMISSION` naming both values
  (DERIVE-1, fail closed).

An explicit `operationBindings`-style narrowing does **not** exist for
`servicesRequired`: the set is a fact, not a choice.

## 4. `operationBindings(closure)`

The derived authorization map: which operations a closure is dispatched
against. With invocations inline, every executed program position is an
invocation whose `run[0]` was resolved at eval to an executable provided by
exactly one declared closure (its *executable closure*).

```
operationBindings(closure c):
  return sorted({ opId(P) | P is an invocation position in the model,
                            executableClosure(P) == c })
```

where the **invocation positions** and their operation ids are:

| Position | opId(P) |
| --- | --- |
| leaf task `t`'s invocation | the leaf's operation id (§5) |
| service `s` start invocation | `s`'s start operation id (§5) |
| service `s` ready/health invocation probe | `s`'s ready/health operation id (§5) |

Pinned consequences:

- **Tool-set members bind nothing.** Every element of an invocation's
  `tools` is verified at admission like any closure (existence,
  executability, target, declaration — SEAM-1/PREPARE-1), but only the
  closure providing the resolved `run[0]` is *dispatched against* the
  operation, so only it gains the binding. PATH availability is attested by
  presence, not by binding.
- Composites bind nothing (they carry no invocation). A prepare-as-task
  reference binds nothing at the service: the referenced task's own leaves
  carry their own bindings.
- A closure referenced by no invocation has `operationBindings = []` (legal;
  e.g. a pure tool-set member).
- **Explicit `operationBindings` survives only as an optional narrowing
  gate**: if declared, it must be a *superset-equal check* target — the
  derived set must be a subset of the declared set, and every declared
  binding must name a declared operation. A derived binding outside the
  declared list is an eval + admission error ("closure `c` is dispatched
  against `op` but its declared bindings do not allow it"). Absent a
  declaration, the derived set is authoritative.
- The runtime re-derives and compares at admission like §3 (fail closed,
  `MODEL_ADMISSION` naming both values).

## 5. Default operation ids and terminal tokens

Derived by default; declared only to override. Overrides are per-position
strings with the same global-uniqueness obligation as today (duplicate
operation ids are rejected at eval and admission).

### 5.1 Operation ids

| Owner | Default operation id |
| --- | --- |
| leaf task `<name>` | `task.<name>.run` |
| service `<name>` lifecycle op `<op>` | `service.<name>.<op>` |

`<op>` ranges over `start`, `ready`, `health`, `stop`, `clean` — and
`prepare` only while prepare is invocation-shaped (phases 1–3). Once prepare
binds a task reference (phase 4), the prepare position has **no operation id
of its own**: its evidence is the referenced task's flattened nodes, keyed by
step path with root `<taskId>` as in §2.

Composites have no operation id: only invocation positions execute.

A flattened PlanNode's operation id is its **leaf's** operation id (the same
leaf referenced from two steps runs the same operation twice, distinguished
by step path — operation id is *what* runs, step path is *where*).

### 5.2 Terminal tokens

Service lifecycle defaults (override per position via `terminal`):

| Op | success | failure |
| --- | --- | --- |
| prepare (phases 1–3 only) | `initialized` | `failed` |
| start | `spawned` | `failed` |
| ready | `ready` | `not-ready` |
| health | `healthy` | `unhealthy` |
| stop | `stopped` | `failed` |
| clean | `cleaned` | `failed` |

**Tasks carry no terminal tokens.** The registry's typed status domain
(`task-succeeded` / `task-failed` / `canceled`) is the complete terminal
vocabulary for bounded execution; a per-task terminal field would be a
hand-declared synonym for it (the A4 disease) and does not exist on the
wire.

## 6. Golden vectors

Each vector is input (the relevant model fragment) and the exact expected
derivation. These land verbatim as fixtures in **both** implementations:
Nix eval fixtures under the compiler tests, and cargo fixtures next to the
runtime lowering. A fixture diverging from this section is a bug in the
fixture.

Shorthand: `leaf(name, requires=[...])`, `comp(name, steps={...})`;
`step = { task, dependsOn }`.

### V1 — nesting, step paths, canonical step order

```
tasks:
  fmt    = leaf(fmt)
  clippy = leaf(clippy)
  check  = comp(steps = { fmt: {task: fmt}, clippy: {task: clippy, dependsOn: [fmt]} })
  ci     = comp(steps = { check: {task: check}, tests: {task: tests, dependsOn: [check]} })
  tests  = leaf(tests)

flatten(ci) =
  [ { stepPath: "ci.check.fmt",    leaf: "fmt",    dependsOn: [] }
  , { stepPath: "ci.check.clippy", leaf: "clippy", dependsOn: ["ci.check.fmt"] }
  , { stepPath: "ci.tests",        leaf: "tests",
      dependsOn: ["ci.check.clippy", "ci.check.fmt"] }
  ]
```

(Emission order is sorted by step name at each level: `check` < `tests`,
`clippy` < `fmt`; plan order moves `ci.check.fmt` before
`ci.check.clippy` because clippy depends on it. `ci.tests` depends on
**every** node under `ci.check`.)

### V2 — the same task referenced twice flattens twice

```
tasks:
  unit = leaf(unit)
  twice = comp(steps = { again: {task: unit, dependsOn: [first]},
                         first: {task: unit} })

flatten(twice) =
  [ { stepPath: "twice.first", leaf: "unit", dependsOn: [] }
  , { stepPath: "twice.again", leaf: "unit", dependsOn: ["twice.first"] }
  ]
```

Two nodes, one leaf, one operation id (`task.unit.run`), two step paths.

### V3 — selecting a leaf directly

```
flatten(fmt) = [ { stepPath: "fmt", leaf: "fmt", dependsOn: [] } ]
```

### V4 — servicesRequired: union, connectsTo closure, canonical order

```
services:
  postgres = { connectsTo: [] }
  api      = { connectsTo: ["postgres"] }
  worker   = { connectsTo: ["postgres"] }     # endpoint-less is irrelevant here
tasks:
  e2e   = leaf(e2e,   requires=["worker"])
  smoke = leaf(smoke, requires=["api"])
  lint  = leaf(lint,  requires=[])
  all   = comp(steps = { e2e: {task: e2e}, lint: {task: lint}, smoke: {task: smoke} })

servicesRequired(all)   = ["api", "postgres", "worker"]
servicesRequired(e2e)   = ["postgres", "worker"]
servicesRequired(lint)  = []
servicesRequired(smoke) = ["api", "postgres"]
```

### V5 — operationBindings: run[0] closure binds, tools do not

```
closures: cargoC (provides bin/cargo), gitC (provides bin/git),
          psqlC (provides bin/psql)
tasks:
  build = leaf(build, invocation = { tools: [cargoC, gitC], run: ["cargo", ...] })
  query = leaf(query, invocation = { tools: [psqlC],        run: ["psql", ...] },
               requires=["postgres"])
services:
  postgres.start  invocation resolves run[0] to pg-serverC
  postgres.ready  invocation probe resolves run[0] to pg-isreadyC

operationBindings(cargoC)      = ["task.build.run"]
operationBindings(gitC)        = []                      # tool-set member only
operationBindings(psqlC)       = ["task.query.run"]
operationBindings(pg-serverC)  = ["service.postgres.start"]
operationBindings(pg-isreadyC) = ["service.postgres.ready"]
```

### V6 — defaults and overrides

```
tasks:
  fmt = leaf(fmt)                                  # no operationId declared
  odd = leaf(odd, operationId = "task.custom.fmt-check")
services:
  postgres = { ... }                               # no ids/terminals declared

opId(fmt) = "task.fmt.run"
opId(odd) = "task.custom.fmt-check"
opId(postgres.start) = "service.postgres.start";  terminal = spawned/failed
opId(postgres.ready) = "service.postgres.ready";  terminal = ready/not-ready
```

### V7 — one closure dispatched by several leaves

```
tasks:
  fmt    = leaf(fmt,    invocation = { tools: [cargoC], run: ["cargo","fmt",...] })
  clippy = leaf(clippy, invocation = { tools: [cargoC], run: ["cargo","clippy",...] })

operationBindings(cargoC) = ["task.clippy.run", "task.fmt.run"]   # sorted
```

### V8 — servicesRequired: diamond dedup

```
services:
  db = { connectsTo: [] }
  a  = { connectsTo: ["db"] }
  b  = { connectsTo: ["db"] }
tasks:
  e2e = leaf(e2e, requires=["a", "b"])

servicesRequired(e2e) = ["a", "b", "db"]
```

### V9 — servicesRequired: prepare task may be composite

```
services:
  dep = { connectsTo: [] }
  svc = { connectsTo: [], prepare: prep }
tasks:
  migrate = leaf(migrate, requires=["dep"])
  seed    = leaf(seed,    requires=[])
  prep    = comp(steps={ migrate: {task: migrate}, seed: {task: seed, dependsOn: [migrate]} })
  run     = leaf(run, requires=["svc"])

servicesRequired(run) = ["dep", "svc"]
```

### V10 — servicesRequired: connectsTo closure is a fixpoint

```
services:
  api    = { connectsTo: ["worker"] }
  worker = { connectsTo: ["db"] }
  db     = { connectsTo: ["cache"] }
  cache  = { connectsTo: [] }
tasks:
  e2e = leaf(e2e, requires=["api"])

servicesRequired(e2e) = ["api", "cache", "db", "worker"]
```
