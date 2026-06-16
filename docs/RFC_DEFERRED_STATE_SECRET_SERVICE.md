# RFC: Deferred Capabilities — Purge, Immutable Source, Secrets, Service Leases

Status: **accepted** (decisions resolved; implementation in progress). Each
section graduates into the relevant normative doc (`AGENTS.md` invariants,
`docs/ARCHITECTURE.md` rationale, `docs/DERIVATION_SPEC.md` if it touches
derivation) and grows tests as it ships, then leaves the AGENTS.md "In progress"
list.

## Scope

Four deferred capabilities, in recommended implementation order, drawn from the
deferred-capabilities review:

1. **Explicit state purge** — clean Protected/Persistent state that `clean`
   refuses today. (*STATE*)
2. **Immutable source modes + `dirtyPolicy = reject`** — `snapshot` /
   `flake-input` codebases, and `reject` made provable for them. (*SOURCE*)
3. **Secret injection** — a references-never-values secret contract, resolver,
   hermetic-env injection, and the *actual* REDACT-1 machinery. (*SECRET*)
4. **Service lifetimes, reuse, and borrower leases** — a per-task
   `serviceLifetime` (`until-idle` / `persistent-until-down`), service reuse
   across runs, and "bring it up and leave it" as an ordinary exported task —
   **no new verb, app, or API**. (*SERVICE*)

**Rejected non-goals** (removed from the docs, *not* deferred): manifest
envelope, dynamic adapter protocol, multi-host, required daemon, UI, non-`fail`
port policies, richer inter-service DAGs, per-tool effects, cross-ref
memoization, environment membership, `validate --deep`. These are not a
backlog — the boundaries that forbid them live in the invariants (AGENTS.md "In
progress" note and ARCHITECTURE.md "Definitional boundaries"). `logs` is
independent and trivial and can land anytime.

## Cross-cutting summary

| # | Capability | ABI rotation? | Wire change | Relative size | Depends on |
| --- | --- | --- | --- | --- | --- |
| 1 | State purge | **No** (CLI flag) | none | XS | — |
| 2 | Immutable source + reject | **No** (enums already in `capability.txt`) | none, if `sourceIdentity` carries the store path | S | — |
| 3 | Secrets | **Yes** | new `secrets` primitive, `${secret:}` grammar, 2 error codes | L | REDACT-1 (built here) |
| 4 | Service leases | **Yes** | new `serviceLifetime` task field + enum + status values; **no new verb** | L (highest invariant entanglement) | reuses run-lease heartbeat/TTL |

**Key finding (ABI):** the model enums for items 1–2 are *already* counted in
the ABI digest — `capability.txt` already declares
`enum SourceMode: snapshot flake-input live-workspace`,
`enum DirtyPolicy: allow warn reject`,
`enum CleanupPolicy: delete-on-clean protected`, and
`enum PersistencePolicy: run-scoped persistent`. Items 1–2 are therefore
*purely additive runtime + Nix behavior over an already-frozen wire*: they
rotate nothing and reject no previously-admitted model. Items 3–4 are genuine
contract increments that rotate the ABI (`runtimeAbi` digest of
`capability.txt`, ABI-1).

**Control app naming:** rename the generated admission sanity app from
`.#admit` to `.#model-check`. `admit` is phase-accurate but not
user-discoverable; `model-check` says what the adopter is doing and keeps the
valuable adopter task name `.#check` free. This is only a generated flake-app
rename over the existing runtime command
`nixfied-runtime check --model <store>/model.json`; it is **not** a runtime
command rename and does not edit `capability.txt`'s `surface:` line.

**Key finding (REDACT-1 is not built):** AGENTS.md and ARCHITECTURE.md state
"persistent output is redacted before write," but there is **no redaction code**
in the runtime (`grep -rniE 'redact|sanitiz|scrub|mask|secret'
runtime/crates --include='*.rs'` is empty). The invariant holds only vacuously
(no secrets exist, so nothing leaks). REDACT-1 must be *implemented*, not
extended, and that work belongs to item 3. When item 3 lands, AGENTS.md
REDACT-1 should change from a present-tense claim to an accurate statement.

## Dependency shape (why this order)

The deferred list is not flat. Two of these items unblock surfaces the review
flagged separately:

- `dirtyPolicy = reject` is *free* for an immutable source (clean by
  construction), so item 2 delivers it without a live-workspace cleanliness
  prover.
- "bring it up and leave it" is downstream of item 4: without a non-run-scoped
  lifetime there is nothing to leave up — and it needs **no** new verb, just the
  `serviceLifetime` field on a task the adopter already exports.

Items 1 and 2 are independent, cheap, and rotate nothing — do them first to
close real gaps with no contract risk. Item 3 is the highest-value *new*
section. Item 4 is the highest-value capability but the most invariant-laden;
it goes last and gets its own phase.

---

## 1. Explicit state purge (STATE)

### Necessity

`clean` cannot remove Protected or Persistent state today. The Postgres
reference adapter declares persistent state, so an adopter who wants to destroy
their data dir **through the framework cannot** — they must `rm -rf` out of
band, defeating marker/lease/path gating. This is a missing half of a shipped
feature, reachable today, not a hypothetical.

### Current state

`runtime/crates/nixfied-runtime/src/state/cleanup.rs`:

- `refuse_cleanup_policy` (`cleanup.rs:244`) admits **only**
  `DeleteOnClean` + `RunScoped`; anything else →
  `CLEANUP_REFUSED "state cleanup policy requires explicit purge, which is not
  implemented"`.
- The full gate chain in `inspect_cleanup_target` / `clean_marked_state`:
  base-confinement (`starts_with` canonical base), target symlink rejection,
  tree symlink rejection, marker ownership match, `refuse_active_refs` (no open
  leases / live processes / reservations), then the policy gate. Cleanup is
  idempotent and crash-safe (missing-target path), records intent + terminal in
  the registry (GC-1/GC-2).

Today this gap exists only as that inline error string — it is **not** in the
AGENTS.md deferred list. Add it there when this lands (or now).

### ABI impact

**None**, provided purge is a **flag on the existing `clean` verb**, not a new
verb. `capability.txt` `surface:` is `… check run ps down clean` — adding a
`purge` verb edits that line and rotates the digest; `clean --purge` does not.
`CLEANUP_REFUSED` already exists. No model/enum change. → strongly prefer the
flag (also keeps VERB-1's reserved control set intact).

### Design

- CLI: `nixfied-runtime clean --purge [--model …]`. Thread a
  `CleanupMode { Standard, Purge }` (or `purge: bool`) from the CLI through
  `clean_marked_state` → `inspect_cleanup_target` → `refuse_cleanup_policy`.
- **Purge relaxes only the policy gate.** Under `--purge`,
  `refuse_cleanup_policy` permits `Protected` and `Persistent`. **Every other
  gate stays unconditional**: confinement, both symlink checks, ownership
  match, and `refuse_active_refs`. Purge means "I accept losing protected /
  persistent data," never "skip safety." You still cannot purge a slot with a
  live lease/process.
- Purge is a **superset** of clean: `--purge` on run-scoped/delete-on-clean
  state behaves exactly like `clean`.
- Registry: the cleanup intent/terminal records a `purge` flag, so the audit
  trail distinguishes a routine clean from a deliberate destruction of
  persistent data. Idempotency/crash-safety unchanged (re-runs hit the
  missing-target path).

### Invariants

- GC-1/GC-2 strengthened (operator-gated destruction is now *expressible*
  under the same safety gates, rather than forcing out-of-band `rm -rf`).
- VERB-1 untouched (flag, not verb).

### Decisions to make

- **D1.1** Flag (`clean --purge`) vs verb (`purge`). *Recommend flag* (no ABI
  rotation, VERB-1 intact).
- **D1.2** One `--purge` overriding both Protected and Persistent, vs separate
  intents. *Recommend single flag* (operator intent is "destroy it"; finer
  control is YAGNI).
- **D1.3** Extra confirmation token (`--purge --yes`)? Runtime is
  non-interactive; the flag is the intent and live-ref gating already prevents
  destroying an in-use slot. *Recommend flag-only*; any prompt belongs to the
  `projectApps`/CLI wrapper.

### Proof

- `tests/state.rs`: purge succeeds on Protected and Persistent markers; purge
  still refuses on active leases/processes, unmarked roots, base-escape,
  symlink target/tree, ownership mismatch; purge on run-scoped == clean;
  re-run after purge is idempotent.
- Gate `lifecycle()` shard: a persistent-state example purged after a run,
  asserting the state root is gone and the registry records a purge terminal.

---

## 2. Immutable source modes + `dirtyPolicy = reject` (SOURCE)

### Necessity

CI and reproducible runs want the runtime to observe an **immutable** source (a
store path / flake input), not a live mutable checkout. For such a source,
cleanliness is guaranteed by construction, so `dirtyPolicy = reject` becomes
*provable for free* — which is exactly why `reject` was deferred (the runtime
"cannot prove live-workspace cleanliness"). The reframing: don't build a
live-cleanliness prover; expose `reject` where it's already true.

### Current state

- Wire: `Codebase { codebaseId, logicalRoot, sourceMode, sourceIdentity,
  sourcePolicy }`; `SourceMode { Snapshot, FlakeInput, LiveWorkspace }`
  (`types.rs:68`); `DirtyPolicy { Allow, Warn, Reject }` (`types.rs:83`). All
  variants already in `capability.txt`.
- Admission (`admission/source.rs`): requires **exactly one** codebase, id
  `main`, mode `live-workspace`; anything else → `SOURCE_MISMATCH`
  (`source.rs:28`). `dirtyPolicy = reject` → `SOURCE_MISMATCH` "cannot prove
  cleanliness" (`source.rs:38`). `resolve_observed_root` resolves `logicalRoot`
  relative to the **CWD** (invocation root), confined (no absolute, no `..`, no
  escape).
- Nix (`nix/modules/source.nix`): only `main`; `dirtyPolicy` enum exposes
  `allow`/`warn` (reject hidden); `sourceIdentity = "live"`,
  `admissionFingerprintPolicy = "live-fingerprint"` are placeholders.

### ABI impact

**None**, if the store path for an immutable codebase is carried in the
existing `sourceIdentity` field (its meaning is already mode-dependent). Adding
a new typed `Codebase.storePath` field *would* rotate the digest. → prefer
overloading `sourceIdentity` semantics per mode unless a typed field proves
necessary.

### Design

- **`snapshot` / `flake-input`**: the observed root is an **immutable Nix store
  path**, not a CWD-relative dir. `sourceIdentity` carries that store path (or
  its content hash + path). Admission for these modes:
  - resolve `observedRoot = sourceIdentity-store-path / logicalRoot`, confined
    under the store path; verify it exists, is a directory, and is under the
    Nix store (reuse the MODEL-ORIGIN-1 store-prefix machinery). No CWD
    dependence → reproducible, location-independent.
  - cleanliness is **by construction**; `dirtyPolicy` is moot. `reject` is
    trivially satisfied; `warn`/`allow` are no-ops for these modes.
- **`dirtyPolicy = reject` stays deferred for `live-workspace`.** The fail-
  closed admission check (`source.rs:38`) remains for live; this RFC does not
  add a live-cleanliness prover. Nix exposes `reject` *only* for immutable
  modes (or omits it for live).
- Nix surface: `nixfied.codebases.main.sourceMode` selects the mode; for
  immutable modes the compiler bakes a store path into `sourceIdentity` (e.g.
  `self.outPath`, or a named flake input's `outPath`). Single-codebase `main`
  is retained for this increment; the only change is its mode may be immutable.

### Invariants

- SOURCE-1 strengthened: still "observe source only through declared
  `codebaseId`s," now optionally against an immutable, content-addressed root.
- MODEL-ORIGIN-1 store-prefix verification is reused for the observed root.

### Decisions to make

- **D2.1** Carry the store path in `sourceIdentity` (no ABI rotation) vs a
  typed `Codebase.storePath` (rotation, cleaner types). *Recommend
  `sourceIdentity`* to keep this a zero-rotation increment.
- **D2.2** For immutable modes, is `dirtyPolicy` ignored at runtime, or must it
  be `reject`? *Recommend* runtime ignores it (dirtiness impossible) while Nix
  only *allows* `reject` for immutable modes, keeping declared intent honest.
- **D2.3** Keep single-codebase `main`-only, or relax to allow an
  immutable-mode codebase under a different id? *Recommend keep single* this
  increment; multi-codebase is its own scope.
- **D2.4** Does `snapshot` differ from `flake-input` semantically, or are they
  one "immutable store path" path with different Nix-side provenance? *Likely
  one runtime path, two Nix sources* — confirm.

### Proof

- `tests/admission.rs`: snapshot/flake-input admit with a store-path
  `sourceIdentity`; reject when the path is absent / not under the store / the
  `logicalRoot` escapes it; `live-workspace + reject` still `SOURCE_MISMATCH`;
  immutable + reject admits.
- A gate example with an immutable codebase + `reject`, run through the runtime.

---

## 3. Secret injection (SECRET)

### Necessity

**High, and blocking for credentialed adopters.** The hermetic child
environment inherits nothing and `${VAR:-}` append-to-inherited is consciously
unsupported (AGENTS.md hermetic semantic; `execution/lower.rs`). Combined with
"the model carries no secret section," **a credential has no legal path to a
child today** except the model's `env` map — which is world-readable in the Nix
store. Trust/peer-auth adapters (Postgres today) sidestep this; any service
needing a real password is stuck. This is the most likely *new section* to be
demanded.

### Current state

- **Zero** secret presence in `nixfied-model`, `nixfied-runtime`, `nix/spec`,
  `nix/modules`.
- `SECRET_UNAVAILABLE` / `SECRET_LEAK_BLOCKED` are documented-reserved but are
  **not** in `error.rs` and **not** in `capability.txt`'s `error-code:` line.
- **REDACT-1 is unimplemented** (see top finding). There is no scrubbing of
  logs, summaries, registry payloads, or error JSON.

### ABI impact

**Major rotation.** New `secrets` field on `Model` + a `SecretDescriptor`
primitive; an extended `substitution:` grammar (`${secret:<id>}`); two new
`error-code:` entries. Old models are rejected by the identity check, which is
the intended behavior for a contract increment (ABI-1, no migrations).

### Design

The load-bearing rule: **descriptors, never values.** `model.json` is a
world-readable store output, so it carries only a *reference* to where a secret
is fetched at runtime — never the material.

- **Contract**: `nixfied.secrets.<id> = { source = <resolver>; }` compiling to a
  `SecretDescriptor { secretId, source }`, where `source` is a **closed,
  minimal** enum of resolvers:
  - `env-var` — read from the **runtime's own** ambient env at admission (the
    one sanctioned ambient read, at the runtime layer, operator-provided — not
    the child's, which still inherits nothing);
  - `file` — read from an operator path (optionally confined under a secrets
    dir).
  - `command`/`agent` (exec a fetch tool) deferred — keep the enum closed and
    extend deliberately.
- **Injection**: a new substitution form `${secret:<id>}`, resolved at spawn
  into the **hermetic child env** value. Start **env-only** — args land in
  process tables and `ps` output.
- **REDACT-1, actually built** (this is real work, not extension): the runtime
  holds the set of resolved secret values for the run and scrubs them from
  every persistent sink it owns — captured child stdout/stderr before it
  reaches logs, summaries, registry payloads, and error JSON — replacing them
  with a redaction token. Honest boundary: REDACT-1 covers
  **runtime-owned persistent output only**; it cannot stop a child writing a
  secret to a socket or a file the child controls. State that scope explicitly.
- **Admission**: resolve-check every descriptor at admission and fail closed
  with `SECRET_UNAVAILABLE` **before any process starts**; never persist values.
- **`SECRET_LEAK_BLOCKED`**: if a redaction guarantee cannot be met for a sink,
  fail closed rather than write a possibly-leaking record.
- **Cross-reference validation** (DERIVE-1 style): every `${secret:<id>}`
  references a declared descriptor; checked at eval, re-proven at admission,
  fail-closed.

### Invariants

- **SECRET-1 (proposed)**: the model carries secret *references*, never values;
  resolved values exist only in runtime memory and the hermetic child env;
  runtime-owned persistent output is redacted (REDACT-1).
- REDACT-1 promoted from vacuous/aspirational to implemented; AGENTS.md REDACT-1
  reworded to match.
- Hermetic-env semantic refined: the *only* ambient read is the resolver's, at
  the runtime layer; the child still inherits nothing.

### Decisions to make

- **D3.1** v1 resolver set = `env-var` + `file` only? *Recommend yes* (closed
  enum, extend later).
- **D3.2** `${secret:}` in env only, or args too? *Recommend env-only first*
  (args are visible in `ps`/process tables; redacting args is harder and
  lower-value).
- **D3.3 — RESOLVED**: resolve at admission (fail closed `SECRET_UNAVAILABLE`
  before any process starts), hold in memory for the run, zeroize on drop. One
  resolution per run avoids an admission→spawn TOCTOU; the model is immutable
  for the run anyway.
- **D3.4** Redaction token format, and the exact set of sinks (logs, summaries,
  registry JSON, error JSON, captured child output). Confirm the
  runtime-owned-output boundary is acceptable as the REDACT-1 scope.
- **D3.5 — RESOLVED**: confine the `file` resolver to an operator-configured
  secrets dir (`$NIXFIED_SECRETS_DIR` or a platform default), rejecting absolute
  or escaping paths — consistent with the path-confinement discipline in cleanup
  and source. Relaxable later if a fixed-absolute-path need (e.g. `/run/secrets`)
  appears.

### Proof

- `nixfied-model` contract tests for the new primitive (deny-unknown-fields,
  round-trip).
- `tests/admission.rs`: `SECRET_UNAVAILABLE` before process start for an
  unresolvable descriptor; undeclared `${secret:x}` rejected.
- A redaction test: a leaf that echoes its secret; assert the value never
  appears in logs/summaries/registry/error JSON (the value is in the child env
  but redacted from every persisted sink). `SECRET_LEAK_BLOCKED` on a sink that
  cannot guarantee scrubbing.
- Gate: an adapter/service consuming an injected credential end to end.

---

## 4. Service lifetimes, reuse, and borrower leases (SERVICE)

### Necessity

**Highest.** The product's stated motivation is dev/test/ci without
collisions, but today every run is run-scoped: a dev iterating 50× restarts the
whole service graph 50×. This item delivers service **reuse** across runs and
the declarative "bring the stack up and leave it" surface — which is just an
ordinary exported task, **not** a new verb or API (D4.4). It is also the most
invariant-entangled item, because there is **no guaranteed daemon** to watch a
service between runs.

### Current state

- Leases are run-scoped: `RUN_LEASE_TTL_SECS = 30`, heartbeated every
  `RUN_LEASE_HEARTBEAT_SECS = 5` by a background `RunLeaseHeartbeat`
  (`registry/leases.rs:13`), per `(run_id, owner_token)`; staleness is TTL
  lapse; `status RunLeaseStatus: active canceling canceled completed failed
  stale`.
- Services start eagerly for a run (`servicesRequired`), live under the run's
  process group, and die at run end. Service lifetime is *implicitly*
  run-scoped — there is **no lease-lifetime enum** in the wire.
- `PersistencePolicy` is *state* lifetime (run-scoped/persistent), **not** lease
  lifetime — do not conflate.
- SVC-ID-1 already specifies the reuse key:
  `serviceInstanceId = hash(serviceAddress, endpointIdentity, stateIdentity,
  runtimeCompatibilityHash, targetIdentity)` — built and ready to be *used* for
  reuse, currently unexercised.

### ABI impact

**Rotation** (accepted — D4.1 chose the declarative model concept). New
`serviceLifetime` task field + `ServiceLifetime` enum (`run-scoped` /
`until-idle` / `persistent-until-down`) + new `ServiceStatus` values (D4.5).
**No new verb** (no `up`), so `capability.txt`'s `surface:` line is unchanged.
Net adopter surface *shrinks* — one task field replaces a would-be verb *and* an
invocation flag — so this is rotation **without bloat**, per the stated
principle.

### Design (revised per D4.1 / D4.4 — declarative, task-driven lifetime)

- **Lifetime is a property of a task's service bring-up**, declared per task,
  defaulting to `run-scoped` (today's exact behavior — zero change for existing
  tasks). A task may declare `serviceLifetime ∈ { run-scoped, until-idle,
  persistent-until-down }`, applied to the **whole `servicesRequired(task)`
  closure** it brings up (a dependency must outlive its dependent, so the
  lifetime covers the closure, not just the directly-required set). Crucially
  the **service definition stays lifetime-agnostic** — this is exactly what the
  old "never a model concept" note protected: the *task that brings a service
  up* chooses the lifetime, the service itself never encodes one.
- **A "bring up the stack" task** = an ordinary adopter-defined task with
  `serviceLifetime = persistent-until-down`, requiring the stack's services, a
  confirming invocation, and optionally exported through `nixfied.surface.verbs`.
  If the adopter names that exported task `prod-up`, then `nix run .#prod-up`
  is their "up" command; `prod-up` is not framework-owned and there is **no
  `up` verb** (D4.4). `down` releases (existing reserved verb). This is the
  declarative production-environment surface (D4.1): structured, composable,
  reviewable — not an ad-hoc invocation flag.
- **Reuse, keyed by SVC-ID-1 (D4.2)**: before starting a required service S,
  look up a **live** (OS-reconciled, LIVE-1) instance of S's exact
  `serviceInstanceId` in the slot; if present, **borrow** it (a run-scoped
  borrower lease) rather than starting a second one. For example, a later
  run-scoped `smoke` test can borrow the services left standing by an exported
  persistent task. A model change rebuilds `runtimeCompatibilityHash`, so reuse
  never crosses incompatible configs.
- **One instance, many leases.** A service instance may carry leases of
  different lifetimes at once. It **lives iff** a `persistent-until-down` lease
  exists (and the slot is not downed) **or** ≥1 live borrower lease exists.
  Refcount is **derived** from live borrower leases reconciled against the OS,
  never a stored counter (the v1 sin). Borrower leases heartbeat on the same TTL
  machinery as run leases; a crashed borrower's lease goes stale and stops
  holding the service up. So a persistent task's lease keeps a service up even
  after a borrowing `smoke` run ends; `down` drops the persistent lease.
- **No daemon → lazy reconciliation (D4.3)**: nobody watches between runs, so
  idle/down transitions are evaluated at the **next runtime invocation**. Each
  `run`/`ps`/`down` reconciles leases against the OS, expires stale borrowers,
  and tears down `until-idle` services with zero live borrowers. An
  `until-idle` service may outlive its last borrower until the next invocation
  notices — the accepted no-daemon semantic.
- **Status (D4.5)**: `ServiceStatus` gains values that make the standing/borrowed
  distinction explicit (`standing` for a persistent instance with no current
  borrower, `borrowed` for a reused instance) so `ps` reports the lifetime
  state, not just `running`.

### Invariants

- **LEASE-LIFETIME-1 (proposed)**: a service lives iff
  `(lifetime = persistent-until-down ∧ not downed)` or
  `(≥1 live, OS-reconciled borrower lease)`. Refcount derives only from live
  borrower leases, never a stored integer. With no daemon, idle/down
  transitions are evaluated lazily at the next runtime invocation.
- SVC-ID-1 finally exercised (reuse path).
- LIVE-1 becomes load-bearing for borrower liveness (a borrower is "alive" only
  if OS-reconciled, surviving PID reuse).
- GC-1/GC-2 extended to **cross-run** lease gating: `clean`/`down`/purge must
  respect live borrower leases held by *other* runs (today `refuse_active_refs`
  is within-slot; this generalizes it).
- Must compose with the v2 marker-upgrade semantics (a borrow must respect an
  in-place state upgrade).

### Decisions (resolved)

- **D4.1 — RESOLVED: declarative per-task `serviceLifetime`** (a model concept),
  not an invocation flag. The production-environment bring-up case needs
  declarative structure; the service stays lifetime-agnostic, so the old "never
  a model concept" concern (lifetime baked into a *service*) is honored.
- **D4.2 — RESOLVED: yes** — auto-borrow on exact SVC-ID-1 match.
- **D4.3 — RESOLVED: accept** lazy idle/down evaluation at the next invocation
  (no daemon).
- **D4.4 — RESOLVED: no `up` verb.** A `persistent-until-down` bring-up task
  exported as a verb is the surface.
- **D4.5 — RESOLVED: new `ServiceStatus` values** for standing/borrowed, so the
  lifetime state is visible in `ps`.

Open sub-points for implementation review: per-task single `serviceLifetime`
(mixed lifetimes via composition — recommended, minimal) vs per-requirement
granularity; exact new status variant names; whether `down` gains a `--service`
selector or stays all-slot.

### Proof

- `tests/service.rs` / `tests/lifecycle.rs` (white-box — these need registry
  access and timing control a bounded task cannot have): borrow an existing
  instance on exact SVC-ID-1 match; refuse-borrow on any identity-layer
  mismatch; `until-idle` torn down when the last borrower's lease goes stale;
  `persistent-until-down` survives a borrower exit and is released only by
  `down`; a crashed borrower's stale lease stops holding the service up;
  cross-run `clean`/purge refuses while a foreign borrower is live.
- Gate: a `persistent-until-down` bring-up task leaves a standing instance that
  a later run-scoped run borrows, with evidence of one start and one borrow,
  released by `down`.

---

## Decisions summary (review checklist)

| ID | Decision | Recommendation |
| --- | --- | --- |
| D1.1 | purge: flag vs verb | flag (`clean --purge`) |
| D1.2 | one purge vs split protected/persistent | single flag |
| D1.3 | extra confirmation token | flag-only |
| D2.1 | store path in `sourceIdentity` vs new field | `sourceIdentity` (no rotation) |
| D2.2 | immutable mode: ignore vs require `reject` | runtime ignores; Nix allows only `reject` |
| D2.3 | single-codebase `main` only | keep this increment |
| D2.4 | snapshot vs flake-input separate semantics | one runtime path, two Nix sources |
| D3.1 | resolver set v1 | `env-var` + `file` |
| D3.2 | `${secret:}` env-only vs args | env-only first |
| D3.3 | resolve once vs per-spawn | resolve at admission, hold for run, zeroize on drop |
| D3.4 | redaction sinks + boundary | runtime-owned output only |
| D3.5 | `file` path confinement | confined secrets dir (`$NIXFIED_SECRETS_DIR`) |
| D4.1 | lifetime: invocation-flag vs model field | **declarative per-task `serviceLifetime`** |
| D4.2 | auto-borrow on SVC-ID-1 match | yes |
| D4.3 | idle teardown at next invocation only | accept (no daemon) |
| D4.4 | `up` verb vs task | **no `up` verb** — bring-up task exported as a verb |
| D4.5 | new vs reused service status domain | new values (`standing` / `borrowed`) |

## Sequencing & rotation plan

All four are being built now (the decision to close the backlog); the
trigger-gating in earlier drafts no longer applies. Pile 2 is rejected and
removed from the docs.

1. **Doc corrections** (done): MODEL-CONTRACT-1 secret strike; DERIVATION_SPEC
   prepare-phase language; Pile 2 removal (AGENTS.md "In progress",
   ARCHITECTURE.md "Definitional boundaries"). **Pending**: REDACT-1 reword —
   lands *with* item 3, since REDACT-1 only has meaning once secrets exist.
2. **Item 1 (purge)** — XS, zero rotation. First.
3. **Item 2 (immutable source + reject)** — S, zero rotation (enums already in
   `capability.txt`).
4. **Item 3 (secrets)** — L, **ABI rotation**. New section + the real REDACT-1.
5. **Item 4 (service leases)** — L, **ABI rotation**. Declarative per-task
   lifetime, SVC-ID-1 reuse, borrower leases; the full lease matrix tested.

Items 3 and 4 each rotate the ABI; batch them into **one** `capability.txt`
rotation to avoid two churns of every emitted model. `logs` lands anytime,
independently.
