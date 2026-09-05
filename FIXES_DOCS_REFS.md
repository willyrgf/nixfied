# Fixing option/reference drift and adopter discovery

Status: proposal, not yet implemented.
Origin: adopter report from MFM, "Reading Nixfied From the Store", filed against
pin `6b3a70b`.
Scope: the adopter-facing meaning of declared options, the placeholder language,
the adapter contracts, and the addressability of the reference material.

**Verification basis.** Every `file:line` in this document was read on `dev` at
`ae11760`. The adopter report was written against `6b3a70b`, 33 commits behind;
citation drift between the two is recorded in §2.8. Every citation in Appendix A
was read directly rather than taken from the review that produced this document.

---

## 1. Problem situation

### 1.1 What the adopter reported

An adopter engineer asked one authoring question: *what are the alternatives to
`stateRefs = [ "slot" ]`?* Answering it took seven hops through a `/nix/store`
checkout of the pinned rev. Four of those hops went into framework internals —
Nix compiler sources and Rust runtime sources.

Their report proposed a packaging fix: export the option-reference derivation
that `nix flake check` already builds, and ship the prose docs as a second
derivation, so an adopter can read both at the rev they actually run.

### 1.2 The real problem

The packaging complaint is a symptom. The cause is this:

> **Nixfied states what an option means in three places. The three disagree. An
> adopter cannot tell which one is wrong, so they read all three. That is the
> seven-hop trail.**

The three places are the typed module description, the runtime code that
consumes (or ignores) the field, and the reference adapters that demonstrate it.

| Source | What it tells an adopter | What is true |
| --- | --- | --- |
| `nix/modules/primitives.nix:288` | `stateRefs` is "included in its identity" | False. `compute_service_identity` (`runtime/crates/nixfied-runtime/src/service/identity.rs:19-52`) derives four hashes from `endpoints`, `primaryEndpoint`, `lifecycle`, `containment`, `connectsTo`, `StatePolicy`, and `Target`. `state_refs` is absent from all four. |
| `nix/modules/primitives.nix:461` | `requires` = "Services that must be ready (alive, probed, addressable) while the leaf runs." | Silent on the rule the adopter needs next. A task binds no endpoints of its own (`service/task.rs:245`), so only service ids resolve in its placeholders. |
| `nix/adapters/*.nix` | Sets `stateRefs`, `logRefs`, `artifactRefs`, `summaryRefs` on every declared service and task | All five fields are inert (§2.2). The reference implementation teaches adopters that dead fields carry meaning. |

A fourth layer compounds it. `docs/CONTRACT.md` is the seam document, and it
contains no description of the placeholder language — the string grammar that
crosses the seam.

### 1.3 Why the existing gate does not catch it

`nix/packages/rust-workspace-check.nix:25` runs:

```
diff -u ${../../docs/OPTIONS.md} ${optionsDoc}
```

That proves `docs/OPTIONS.md` matches the typed module declarations. It is a
good gate and it currently passes. But nothing proves the *declarations* match
the *runtime*. The gate checks the copy; it never checks the claim.

So `primitives.nix:288` can assert that `stateRefs` participates in service
identity, the runtime can discard the field at `execution/lower.rs:127`, and
every gate in the repository stays green while the false sentence is published
verbatim to adopters at `docs/OPTIONS.md:1852`.

This is the structural hole. Sections 2.2 through 2.5 are its consequences.

---

## 2. Investigation

### 2.1 Method

Three reviewers worked the report in parallel, on separate lenses: the flake
output surface, the model semantics, and the adapter/placeholder contracts.
Their findings were then re-verified against the source on `dev`. Where a
reviewer's conclusion conflicted with another's, the conflict was resolved by
reading the code directly (§2.8).

### 2.2 Finding 1 — five declared options are inert

`stateRefs` is not "identity-only", as the adopter concluded. It is fully inert.
Four sibling fields share the condition.

| Option | Declared | Discarded |
| --- | --- | --- |
| `services.<n>.stateRefs` | `nix/modules/primitives.nix:285` | `execution/lower.rs:127` |
| `services.<n>.logRefs` | `nix/modules/primitives.nix:290` | `execution/lower.rs:128` |
| `tasks.<n>.artifactRefs` | `nix/modules/primitives.nix:468` | `execution/lower.rs:328` |
| `tasks.<n>.logRefs` | `nix/modules/primitives.nix:473` | `execution/lower.rs:329` |
| `tasks.<n>.summaryRefs` | `nix/modules/primitives.nix:478` | `execution/lower.rs:330` |

The five fields are emitted into `model.json` (`nix/compiler/derive.nix:311` for
`stateRefs`) and printed by the generated view (`nix/compiler/views.nix:61`).
Because they are emitted, editing one does change `computedModelHash` — every
byte of the model does. They do not reach service identity, state placement,
cleanup, or execution. Their only other appearance in the runtime is the
composite rejection check at `nixfied-model/src/validation.rs:509-517`, which
enforces that a composite carries no refs. On a leaf they are accepted and
dropped.

Why this happened is recorded in the source. `nix/compiler/derive.nix:247-249`
notes a deliberate migration:

> Service identity is no longer emitted: the runtime derives a service's reuse
> identity from its own lowered contract, so the model carries no identity
> hashes for it to trust.

The fields and their descriptions were left behind by that refactor. The
description at `primitives.nix:288` still describes the pre-migration world.

The reference adapters populate all five: `nix/adapters/postgres.nix:207-208`
and `:234-235`, `nix/adapters/reth.nix:182-183` and `:192-193`,
`nix/adapters/synthetic.nix:100-101` and `:117-118`. Four examples copy the
pattern. The adopter's confusion was induced by the framework, not by
misreading it.

All five fields appear in the capability descriptor
(`runtime/crates/nixfied-model/capability.txt:25` and `:36`), so removing them
rotates the runtime ABI.

### 2.3 Finding 2 — a task's placeholder namespace is never stated

`${port:<name>}` and `${host:<name>}` resolve `<name>` first against the exec's
own endpoint ids, then against dependency service ids
(`service/process.rs:2179-2188`, implemented at `:2198-2227`).

A task binds no endpoints. `service/task.rs:245` constructs an empty map:

```rust
let own_endpoints = std::collections::BTreeMap::new();
```

So from a task, only **service ids** resolve. Nix enforces the same rule at
eval: `nix/compiler/validate.nix:197-206` builds `allowed` by filtering
`task.requires`, and rejects any named reference outside it.

The rule reaches adopter documentation only on the service side, as a byproduct
of three option descriptions:

- `nix/modules/primitives.nix:262` — own endpoints are addressable via `${port:<endpointId>}`
- `nix/modules/primitives.nix:270` — `primaryEndpoint` is what bare `${port}`/`${host}` resolve to
- `nix/modules/primitives.nix:281` — `connectsTo` makes a dependency addressable via `${port:<serviceId>}`

The task-side option, `tasks.<name>.requires` at `nix/modules/primitives.nix:461`,
says only:

> Services that must be ready (alive, probed, addressable) while the leaf runs.

It names no placeholder spelling, and it does not say that the endpoint-id half
of the namespace is empty for a task. This is the single highest-traffic option
in the framework: every task with a service dependency passes through it.

The eval-time refusal does not recover the adopter. `nix/compiler/validate.nix:351`
throws a fixed string:

> leaf task named endpoint placeholders must reference addressable required services

It names neither the offending reference nor the legal set.

### 2.4 Finding 3 — the endpoint sugar makes the trap worse in two adapters

`nix/compiler/derive.nix:264-266` normalizes the singular `endpoint` sugar into
the `endpoints` map, keyed by `endpointId`.

`nix/adapters/postgres.nix:204-206` declares:

```nix
endpoint = {
  endpointId = "postgres-tcp";
};
```

An adopter reading that source sees the literal string `postgres-tcp` and has
every reason to write `${port:postgres-tcp}` in their task. Nix rejects it. The
correct task-side spelling is `${port:postgres}`, and that string appears in the
adapter source only as an attribute path segment.

`synthetic` has the same shape (`synthetic-tcp`). `reth` declares the explicit
map form with `reth-http`, `reth-ws`, `reth-authrpc` and
`primaryEndpoint = "reth-http"` (`nix/adapters/reth.nix:176-181`), which is what
the adopter reported. So two of the three shipped adapters carry the trap, and
`reth` is not the unlucky case.

No adapter demonstrates the named from-a-task spelling. All adapter tasks use
bare `${port}`. The only named example in the adopter docs is
`docs/GUIDE.md:184`, and it sits in a service `connectsTo` context, not a task
context. `docs/ADAPTERS.md:84` shows `${port:reth-http}` — correct for the
service's own start wrapper, wrong for the scope an adopter is usually writing.

### 2.5 Finding 4 — `cleanupPolicy` and `persistence` are one fact with two owners

`nix/modules/state.nix:20-25` declares `cleanupPolicy` as
`delete-on-clean | protected`. `nix/modules/state.nix:28-34` declares
`persistence` as `run-scoped | persistent`. Their descriptions are near
duplicates of each other.

They are not two axes. `runtime/crates/nixfied-runtime/src/state/cleanup.rs:267-284`
combines them into one AND gate:

```rust
if cleanup_policy == &CleanupPolicy::DeleteOnClean
    && persistence == &PersistencePolicy::RunScoped
{
    return Ok(());
}
```

Four representable states collapse to two outcomes; three of the four are
indistinguishable. `AGENTS.md:34-36` requires that every fact have one owner,
and `AGENTS.md:16-17` requires that invalid states be unrepresentable or
rejected at the owning boundary. This pair violates both.

A related but smaller issue: the string `run-scoped` is a legal value of both
`nixfied.state.persistence` and `nixfied.tasks.<name>.serviceLifetime`
(`nix/modules/primitives.nix:439-447`, whose enum is
`run-scoped | until-idle | persistent-until-down`). The vocabularies overlap
partially — `run-scoped` is shared, while `persistent` and
`persistent-until-down` diverge. Partial synonymy teaches a correspondence that
then breaks.

Collapsing the `cleanupPolicy`/`persistence` pair dissolves the collision as a
side effect. Renaming `run-scoped` on its own would cost a full contract
rotation (`docs/CONTRACT.md:232-248`) and break every adopter's `nixfied.nix`,
while buying only disambiguation that a cross-link also buys.

### 2.6 Finding 5 — two runtime defects, not documentation gaps

**`${stateDir}` fails open.** `service/process.rs` substitutes `${stateDir}`
unconditionally, then guards at `:2228`:

```rust
if out.contains("${port:") || out.contains("${host:") {
    return Err(RuntimeError::new(
        ErrorCode::LifecycleFailed,
        format!("unresolved endpoint placeholder in exec value: {value}"),
    ));
}
```

The guard tests only the two endpoint forms. A misspelling such as
`${statedir}` or `${stateDir }` is handed to the child process as a literal
string. `${stateDir}` also has no eval-time validation: it appears nowhere in
`nix/compiler/validate.nix` or in the module options. Every other placeholder
form fails closed. This one fails open, and it is the placeholder most likely to
be typed by hand.

**The named-reference refusal is uninformative.** `nix/compiler/validate.nix:351`
and `:353` both throw fixed strings that omit the offending reference and the
legal alternatives.

### 2.7 Finding 6 — the proposed packaging fix does not deliver pinning

The adopter's headline ask was `nix build nixfied#options-doc` and
`nix build nixfied#docs`. Traced against a scratch adopter flake:

1. **`nix build nixfied#docs` cannot resolve.** The bare name `nixfied` is a
   *registry* reference, not an input reference. It is not present in any
   registry (confirmed: `nix registry list` has no `nixfied` entry). It never
   consults the adopter's `flake.lock`. Had it resolved, it would have returned
   the registry's rev — reintroducing the drift the report set out to remove.
2. **`nix build github:willyrgf/nixfied#docs` resolves the default branch**, not
   the pin. That is the website problem relocated into a store path.
3. **The prose already ships at the pinned rev.** `docs/*.md` are git-tracked
   and `flake.nix` applies no source filter, so the files are already inside the
   input's store path. The report's own footer confirms the adopter read them
   there. A `packages.<system>.docs` derivation would copy files that are
   already present, at the same rev, in the same store.

What is missing is not a derivation. It is an **address**.

Two consequences follow. First, an immediate unblock exists today and should be
sent to the adopter now — it replaces their two-step `getFlake` trick, needs no
`--impure`, and works uniformly across input types:

```sh
nix flake archive --json | jq -r '.inputs.nixfied.path'
```

Second, the correct fix is an **app**, not a package (§3, Tier 2). It must be
store-baked static content that invokes no `nix` subcommand, and it must not
inherit the context-binding guard in `nix/help-app.nix:29-41`. That guard exists
because `#help` projects live flake state and is only meaningful for the flake
it runs in. A docs app has the opposite nature: its content is fixed at
evaluation and describes Nixfied, not the caller. `nix run
github:willyrgf/nixfied#docs` must work for a reader with no checkout.

### 2.8 Corrections to the adopter report

The report is accurate on `dev` except as noted.

| Report claim | Status on `ae11760` |
| --- | --- |
| `optionsDoc` is built only inside `checks` | Correct. `flake.nix:493`, consumed once at `:518`. |
| Its only consumer is a drift diff | Correct. `nix/packages/rust-workspace-check.nix:25`. |
| The runtime ignores `stateRefs` | Correct. `execution/lower.rs:127`. |
| `stateRefs` is "identity and hash only" | **Understated.** It is absent from `compute_service_identity` (`service/identity.rs:19-52`). It is fully inert. It perturbs `computedModelHash` only because it is emitted, as any byte would. |
| `run-scoped` names two unrelated axes | Correct, and it is the only cross-enum value collision in `nix/modules/`. The sharper defect on that surface is §2.5. |
| `views.nix:60` | Now `nix/compiler/views.nix:61`. |
| `service/process.rs:2088-2095` | Moved to `service/process.rs:2179-2188`. |

That last row is worth stating plainly. In 33 commits, one of the report's four
source citations moved by roughly 90 lines. This vindicates the instinct to read
the pinned store path. It also demonstrates that a Rust doc comment cannot serve
as an adopter contract surface.

---

## 3. Proposed solution

Five tiers, ordered by value per unit of cost. Tiers 1 and 2 are independent of
each other and of everything below. Tier 3 requires one ABI rotation and should
consume only one.

### Tier 1 — stop the false and missing statements

No ABI cost. Highest value per character in this document. All three changes
flow into `docs/OPTIONS.md` through the existing generator, and the existing
gate at `nix/packages/rust-workspace-check.nix:25` keeps the copy honest.

1. **Correct `nix/modules/primitives.nix:288`.** The sentence "included in its
   identity" is false, not merely thin. If Tier 3 proceeds, this field is
   deleted instead; if Tier 3 is deferred for any reason, this correction is the
   minimum acceptable action, because the false claim currently ships to
   adopters at `docs/OPTIONS.md:1852`.
2. **Add the placeholder rule to `tasks.<name>.requires`**
   (`nix/modules/primitives.nix:461`), symmetric with the three service-side
   descriptions that already exist:

   > Each required service is addressable from this task's exec args and env via
   > `${port:<serviceId>}` and `${host:<serviceId>}` — by service id, never
   > endpoint id, because a task binds no endpoints of its own. The first entry
   > is the primary, which bare `${port}` and `${host}` resolve to.

   This one sentence removes hops 2 through 7 of the adopter's trail, and of a
   second trail walked independently during review.
3. **Cross-link the `run-scoped` descriptions** in `nix/modules/state.nix:28-34`
   and `nix/modules/primitives.nix:439-447`, so each names the axis the other
   governs. Supersede this with Tier 3 item 2 if that lands first.

### Tier 2 — give the reference an address

1. **Add a `docs` app** to `apps` in `flake.nix:449` and to
   `nix/project-apps.nix`. Requirements, from §2.7:
   - store-baked static content; it must call no `nix` subcommand;
   - it must **not** carry the `help-app.nix` context guard, so a reader with no
     checkout can run it against a remote ref;
   - inside an adopted project, it reaches the framework through the locked
     input, so it is pinned by construction with no adopter wiring;
   - it never enters the runtime, so SEAM-1 holds.

   Topic routing over the existing `docs/*.md` set is enough for the first
   version. `#docs model`, which prints the project's own `views/docs.md`, is
   project-specific and belongs to the `projectApps` copy only.
2. **Add a pointer line to `.#help`**, naming `#docs`.
3. **Add a pointer comment to the scaffold template** at
   `runtime/crates/nixfied-cli/src/main.rs:258`. The generated `nixfied.nix` is
   the first file every adopter opens. It currently names `.#help` and `.#run`
   and no reference material at all.

### Tier 3 — subtract rather than annotate

One atomic contract change per `docs/CONTRACT.md:232-248`: capability
descriptor, ABI rotation, paired Nix producer and Rust consumer, adapters,
examples, `nix/compiler/views.nix`, fixtures, and golden vectors.

1. **Delete the five inert fields** listed in §2.2. `AGENTS.md:18-19` names
   "speculative machinery" as the thing simplicity excludes, and `AGENTS.md:40-42`
   says to prefer subtraction over partial workarounds. Five typed, defaulted,
   validated, wire-serialized fields that no consumer reads are the definition
   of the former; documenting them is the latter.
2. **Collapse `cleanupPolicy` and `persistence` into one option** whose values
   are the two outcomes the runtime actually distinguishes (§2.5). This also
   dissolves the `run-scoped` collision, so do not spend a second rotation on a
   rename.

Both items belong in the same rotation window.

### Tier 4 — generate what is currently prose

1. **Derive the adapter catalog; do not declare it.** Adapters are plain modules
   and `nix/compiler/resolve.nix` already accepts any module with `adapters` in
   `specialArgs`. Evaluating each adapter against a minimal project stub and
   running the real `nix/compiler/derive.nix` yields authoritative
   post-normalization facts. That reads *through* the endpoint sugar, so it
   prints `postgres-tcp` correctly, and it can render both placeholder spellings
   mechanically from the same facts. A new adapter is covered the moment it
   lands in `nix/adapters/default.nix` — no coverage gate to maintain, nothing
   to forget.

   A hand-written `meta` attribute is the worse option: it needs a coverage gate
   *and* a correctness gate, and the correctness gate would have to derive the
   truth anyway in order to compare against it.

   The one fact the generator cannot derive is the `${stateDir}` sub-layout
   (`pgdata/`, `reth/`), which lives inside shell wrapper strings. Leave that to
   prose, or make `stateLayout` the only declared field.

2. **Put the placeholder grammar into `docs/CONTRACT.md`** (Appendix B). The
   seam document should specify the seam's own string language.

### Tier 5 — fix the two defects

1. **Close the `${stateDir}` guard** at `service/process.rs:2228`, so an
   unresolved or misspelled `${stateDir}` fails closed like every other form.
   Consider validating the spelling at eval as well.
2. **Make `nix/compiler/validate.nix:351` and `:353` name the offending
   reference and the legal set.**

### The change that prevents recurrence

Tiers 1 and 3 fix the instances. This fixes the class:

> **Add a gate that proves declarations match behavior.**

`runtime/crates/nixfied-model/capability.txt` already enumerates every wire
field. Require each listed field to be read by the runtime, or to be explicitly
marked as identity-only or wire-only. Such a gate would have failed at the
commit that introduced the drift in §2.2, and it closes the hole described in
§1.3 — the repository currently proves that the docs match the declarations, and
never that the declarations match the code.

### Out of scope

No website, no hosted portal, no documentation pipeline. The adopter was
explicit that a store path they can `cat` is sufficient, and it is strictly
better than a website because it cannot disagree with the rev they run. Nothing
in this proposal adds a hosted surface.

---

## Appendix A — verified citations

Read on `dev` at `ae11760`.

| Fact | Location |
| --- | --- |
| `optionsDoc` defined inside the `checks` let-scope | `flake.nix:493` |
| `optionsDoc` consumed once | `flake.nix:518` |
| Drift diff against the checked-in copy | `nix/packages/rust-workspace-check.nix:25` |
| `state_refs` / `log_refs` discarded when lowering a service | `runtime/crates/nixfied-runtime/src/execution/lower.rs:127-128` |
| `artifact_refs` / `log_refs` / `summary_refs` discarded when lowering a task | `runtime/crates/nixfied-runtime/src/execution/lower.rs:328-330` |
| Service identity inputs; `state_refs` absent | `runtime/crates/nixfied-runtime/src/service/identity.rs:19-52` |
| Identity migration rationale | `nix/compiler/derive.nix:247-249` |
| `stateRefs` emitted into the model | `nix/compiler/derive.nix:311` |
| `stateRefs` printed in the generated view | `nix/compiler/views.nix:61` |
| False identity claim in the option description | `nix/modules/primitives.nix:288` |
| The same claim, published | `docs/OPTIONS.md:1852` |
| The five fields in the capability descriptor | `runtime/crates/nixfied-model/capability.txt:25,36` |
| Composite rejects any refs | `runtime/crates/nixfied-model/src/validation.rs:509-517` |
| Placeholder resolution order, documented | `runtime/crates/nixfied-runtime/src/service/process.rs:2179-2188` |
| Placeholder substitution implementation | `runtime/crates/nixfied-runtime/src/service/process.rs:2198-2227` |
| Fail-closed guard, endpoint forms only | `runtime/crates/nixfied-runtime/src/service/process.rs:2228` |
| A task's own-endpoint map is empty | `runtime/crates/nixfied-runtime/src/service/task.rs:245` |
| Eval-time task named-reference scope | `nix/compiler/validate.nix:197-206` |
| Fixed refusal strings | `nix/compiler/validate.nix:351,353` |
| `requires` description, no placeholder spelling | `nix/modules/primitives.nix:461` |
| Service-side placeholder descriptions | `nix/modules/primitives.nix:262,270,281` |
| Endpoint sugar normalization | `nix/compiler/derive.nix:264-266` |
| Postgres declares `postgres-tcp` via sugar | `nix/adapters/postgres.nix:204-206` |
| Reth endpoints and primary | `nix/adapters/reth.nix:176-181` |
| Adapters set the inert fields | `nix/adapters/postgres.nix:207-208`, `nix/adapters/reth.nix:182-183`, `nix/adapters/synthetic.nix:100-101` |
| `cleanupPolicy` and `persistence` declarations | `nix/modules/state.nix:20-25,28-34` |
| The combined AND gate | `runtime/crates/nixfied-runtime/src/state/cleanup.rs:267-284` |
| `serviceLifetime` enum | `nix/modules/primitives.nix:439-447` |
| Contract change procedure | `docs/CONTRACT.md:232-248` |
| Runtime ABI derivation | `nix/spec/constants.nix:13` |
| Design stance: subtraction, one owner, no speculative machinery | `AGENTS.md:16-19,34-36,40-42` |
| Help app context guard | `nix/help-app.nix:29-41` |
| Scaffold template names no reference | `runtime/crates/nixfied-cli/src/main.rs:258` |
| Adopter-facing placeholder mentions | `docs/ADAPTERS.md:84,103`, `docs/GUIDE.md:184`, `docs/OPTIONS.md:453,558` |

## Appendix B — placeholder grammar

Derived from `service/process.rs:2198-2234` and `nix/compiler/validate.nix`.
Substitution is literal string replacement in a fixed pass order. There is no
escaping, no nesting, and no default values.

| Form | Resolves against | In a service lifecycle exec | In a task invocation | Valid in | Failure mode |
| --- | --- | --- | --- | --- | --- |
| `${port}` / `${host}` | own primary | the service's own `primaryEndpoint` | the first entry of `requires`, via its `primaryEndpoint` | args, env, stdin | rejected at eval when no primary resolves |
| `${port:<name>}` / `${host:<name>}` | own endpoint ids first, then dependency service ids | own `endpointId`, or a `connectsTo` service id | a `requires` **service id only** — the endpoint-id namespace is empty for a task | args, env, stdin | rejected at eval; fails closed at runtime with `LifecycleFailed` (`process.rs:2228`) |
| `${stateDir}` | the materialised slot state root | always resolves | always resolves | args, env, stdin | **none — fails open** (§2.6) |
| `${secret:<id>}` | declared `nixfied.secrets` ids | env values only | env values only | `invocation.env` only | `LifecycleFailed` if used in args |

Pass order: own endpoints, then dependency services, then bare, then
`${stateDir}`, then secrets (env only). Own-first is stated as intent;
`nix/compiler/validate.nix` proves own endpoint ids and `connectsTo` service ids
are disjoint, so the order is not load-bearing for correctness.

`${projectId}`, `${environment}`, `${slot}`, and `${runId}` are runtime-owned
path templates and are not adopter-writable
(`runtime/crates/nixfied-runtime/src/state/placement.rs:65-69`).
