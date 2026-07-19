# Fixes and cleanups for the cache/port RFC implementation

- Status: approved implementation handoff
- Base revision: `f331b83`
- Upstream repository: Nixfied
- Downstream repository: `../mfm3`
- Compatibility policy: none
- Final disposition: delete this handoff in the final local upstream commit,
  before that revision is pushed; task history and any earlier Git commit are
  sufficient

## Outcome

Keep the cache deletion and the endpoint-lock/kernel-observation correction, but
remove the automatic service-repair responsibility and consolidate the code
around one narrow contract:

1. Cache and accelerator lifecycle remains entirely child/tool/project-owned.
2. Nixfied serializes cooperating endpoint starts with private host OS locks.
3. Nixfied verifies exact kernel socket ownership before committing readiness.
4. A ready service's listening sockets are the steady-state host owners.
5. Exact healthy same-registry service reuse remains supported.
6. Endpoint acquisition/reuse never signals a pre-existing service to resolve a
   collision or repair a mismatch. A non-reusable live service fails closed and
   requires explicit `down` before replacement.
7. Registry facts have one writer and derived state is not persisted.
8. Linux and macOS retain separate kernel parsers, but share process primitives
   and endpoint representations where their semantics are identical.

The current upstream range from `b0681e4` to `f331b83` is 52 files,
12,089 insertions, 2,648 deletions, net `+9,441`. A realistic final target is
approximately `+4,000` to `+5,000` net. The target is directional, not a reason
to weaken correctness: every retained line must own a non-derived responsibility
or prove a distinct behavioral boundary.

## Non-negotiable rules

These apply to every commit:

- Optimize for fewer concepts, code paths, public types, duplicated
  responsibilities, and future change sites. Prefer deletion.
- Preserve no backward compatibility. Do not accept an old ABI, registry shape,
  status vocabulary, model field, output field, or command.
- Add no migration, alias, ignored field, compatibility parser, fallback,
  deprecated path, feature switch, or old-layout cleanup.
- Delete superseded code and tests. Do not leave wrappers around the old path.
- Do not introduce a daemon, guardian, global registry, lifetime lock holder,
  socket activation, dynamic port selection, retry policy, repair worker, or new
  semantic artifact.
- `model.json` remains the only semantic seam. The runtime remains Nix-free.
- Preserve exact fail-closed admission, endpoint ownership, containment,
  registry, redaction, and cleanup invariants.
- Keep all endpoint implementation types crate-private except the one endpoint
  value intentionally returned by the high-level runtime API.
- Do not create a platform trait, observer registry, shared Linux/macOS wire
  record, or generic state-machine framework. There are exactly two supported
  kernel implementations and their wire contracts are genuinely different.
- Do not make production internals public for integration tests. Move a test
  inward or test through the high-level API.
- Inspect both worktrees before editing and before every commit. Preserve
  unrelated user changes.
- Commit messages are lower-case, one concise line, with no body or trailers.
- Do not push. Downstream exact-pin work must wait until the final upstream
  revision is reachable; use a local override only for interim verification.
- Do not alter the existing signal authorities for a process started by the
  current run, explicit `down`, model-declared `until-idle` reconciliation, or
  marker-driven state upgrade. They are outside the endpoint-repair deletion.

If implementation exposes a genuinely new architecture decision, stop and ask
an architect. Do not preserve the old branch as a fallback while waiting.

Execution has three authority phases:

1. Implement all local upstream commits, including deletion of this temporary
   handoff and its whitelist, and pass the Linux/local/cross-compile floor on
   the resulting final revision.
2. After explicit push authorization, publish that already-final upstream
   revision and require its hosted macOS job to pass. If push is not authorized,
   stop and report this external verification as pending.
3. Only after that exact revision is remotely reachable, update MFM's pin and
   run the downstream floor. No upstream cleanup commit follows the pin.

## Facts established by review

The current diff is approximately:

| Area | Insertions | Deletions | Net |
| --- | ---: | ---: | ---: |
| RFC plus implementation plan | 2,674 | 0 | +2,674 |
| Rust production-file paths | 6,294 | 1,782 | +4,512 |
| Rust integration tests | 2,832 | 554 | +2,278 |
| Nix, gates, and CI | 208 | 252 | -44 |
| Other documentation and metadata | 81 | 60 | +21 |

The Rust production-file category includes roughly 833 lines of inline endpoint
unit tests. The downstream MFM adoption is already net negative and its single
`target/verification` anchoring path is correct.

Two correctness defects remain:

1. macOS endpoint correlation uses a dynamically sized process list, while
   descendant discovery and process-group liveness still use separate fixed
   8,192-PID buffers. They can miss live processes.
2. Three termination-failure paths discard `mark_process_escape` errors, and
   the transition does not validate affected-row counts. `PROC_ESCAPE` can be
   returned without durable escaped evidence.

The largest avoidable responsibility is terminate-before-replace repair. It is
not required to solve host-global endpoint acquisition and makes `run` a
destructive recovery controller.

## Normative endpoint acquisition contract

Implement one path for endpoint-bearing service acquisition:

1. Derive the resolved endpoints and exact service identity once.
2. Attempt exact healthy reuse from the local registry. Reuse requires:
   - exact service address and all four `SVC-ID-1` identity components;
   - the exact selected endpoint set;
   - a live primary process with matching start identity;
   - a ready process record and complete active port rows; and
   - current exact OS listener ownership by the recorded containment.
3. If exact reuse does not succeed, derive and sort every host endpoint lock key
   and acquire every lock non-blockingly in canonical order.
4. Reconcile dead processes, expired reservation-only evidence, and stale ports.
   Retry exact reuse once under the locks.
5. If live or otherwise nonterminal local evidence remains but is not exactly
   reusable, fail without signaling as an endpoint collision/repair remedy:
   - an authoritative open lease blocks replacement with `LEASE_CONFLICT`, but
     does not block an additional exact healthy borrower;
   - missing or unprovable expected ownership is `PORT_UNVERIFIABLE`;
   - an observed conflicting outside listener is `PORT_CONFLICT`.
6. Preflight every endpoint with an exact temporary bind and a complete kernel
   listener snapshot.
7. Reserve the run's endpoint rows atomically before prepare or spawn.
8. Run prepare, spawn the child, and durably record its process identity.
9. Retain every startup lock while the declared readiness probe and exact
   ownership verification run.
10. Atomically mark all ports active and the process ready, and record readiness
    evidence. Partial ready state is forbidden.
11. Release the startup locks. The service sockets now own the endpoints.
12. If an external process wins the accepted bind race, classify an observable
    listener as `PORT_CONFLICT`; otherwise fail `PORT_UNVERIFIABLE`.

Endpoint acquisition/reuse must never signal a process that existed before that
acquisition to resolve a collision or ownership mismatch. Explicit `down` is
the destructive replacement authority for such a service. Existing
current-run cancellation/failure cleanup, `until-idle` teardown, and
marker-driven state-upgrade teardown remain unchanged. A broken persistent
service therefore requires `down`, then a new `run`. This is intentional.

Endpoint-less services retain their existing scoped carve-out and take no
endpoint lock.

## Retained and rejected behavior

Retain:

- exact same-registry healthy reuse;
- secure descriptor-anchored lock-directory traversal;
- network-namespace-aware Linux lock keys and host-scoped macOS keys;
- deterministic multi-endpoint locking;
- nonblocking lock acquisition;
- exact, wildcard, IPv4-mapped, IPv6-only, and `TIME_WAIT` semantics;
- lock ownership through the atomic ready transaction;
- the listening socket as post-readiness ownership;
- current cancellation and owned-child failure cleanup;
- model-declared `until-idle` teardown and marker-driven state-upgrade teardown;
- `ProcessTree`, which predates this RFC and is used by Postgres and Reth;
- persisted descendant identity for unresolved `PROC_ESCAPE`, `down`, and
  cleanup safety;
- optional `nixfiedOwner` only when the existing registry, process identity,
  containment, and observed socket records prove it exactly;
- macOS hosted execution and Darwin cross-compilation.

Delete:

- automatic terminate-before-replace repair;
- repair claims and repair-specific CAS state;
- durable-owner repair exceptions;
- repair-specific sibling-lease fencing and heartbeat behavior;
- crash-after-repair recovery;
- `recover_starting` reuse;
- repair events, helpers, fixtures, tests, and RFC requirements;
- proactive unrelated-service endpoint inspection before a kernel conflict;
- any implication that `run` may repair by signaling an existing process.

Startup-lock contention never proves an owner. Cross-state-root conflicts omit
`nixfiedOwner`. Owner proof is attempted only at the actual `PORT_CONFLICT`
serialization boundary and reuses the same process/socket evidence used by
healthy reuse. Do not retain a separate attribution state machine.

## Commit 1: fix macOS process enumeration

Suggested commit message:

```text
fix macos process enumeration
```

### Required changes

- Move the dynamically sized `proc_listallpids` implementation into the existing
  macOS process primitives in `service/process.rs`.
- Expose one `io::Result<Vec<u32>>` primitive that process containment and
  endpoint correlation map into their existing typed errors.
- Use that one primitive for:
  - endpoint FD-holder correlation;
  - direct-child/descendant discovery; and
  - process-group liveness.
- Delete both fixed 8,192-entry scanners and the endpoint-local duplicate.
- Preserve sort/dedup once in the shared primitive.
- Use checked capacity growth and checked byte-size conversion to `c_int`.
  Exhaustion or overflow returns an error; a saturating infinite loop is
  forbidden.
- Do not add a new platform module or trait solely for this function.

### Acceptance

- No `8192` process-list capacity remains.
- There is one macOS `proc_listallpids` sizing/growth loop.
- Capacity growth and byte-size overflow fail closed.
- Descendant and group-liveness errors remain fail-closed as `PROC_ESCAPE`.
- Endpoint correlation maps enumeration failure to `PORT_UNVERIFIABLE` before
  unsafe service admission.
- Linux behavior is unchanged.

## Commit 2: narrow service acquisition and delete repair

Suggested commit message:

```text
remove automatic service repair
```

This is the deliberate runtime responsibility reduction and contract event.

### Delete from `service/process.rs`

- `RepairOutcome`;
- `repair_requested_service`;
- `repair_candidate_endpoints`;
- every terminate-before-replace branch;
- every `recover_starting` argument and conditional;
- the third repair-dependent borrow attempt;
- escaped-versus-ordinary repair termination tails;
- repair-only reconstruction and comparison helpers;
- `inspect_unrelated_endpoint_evidence`.

Keep exactly two possible reuse attempts: one pre-lock exact-reuse attempt and
one under all startup locks after reconciliation. A successful attempt must
atomically revalidate identity/process/port evidence and admit the borrower
lease; it is not read-only.

### Delete from `service/registry.rs` and leases

- `refuse_unexpired_repair_leases`;
- `guarded_claim_service_repair`;
- `mark_repair_target_stale`;
- `repair_snapshot_matches`;
- `durable_owner_is_eligible`;
- repair-specific lease errors, events, and payloads;
- `open_port_service_instances`;
- repair-specific sibling fencing and heartbeat branches.

Reservation-only evidence left by a crash before process recording still needs
one ordinary reconciliation path. Move that responsibility into normal registry
reconciliation:

- an unexpired lease remains authoritative and blocks;
- an expired/stale lease with no process owner allows its owner-less reserved
  ports to become stale/released transactionally;
- endpoint reconciliation never signals a live process to resolve a collision
  or ownership mismatch; existing `until-idle` lifecycle reconciliation remains
  unchanged;
- no service-specific repair API remains.

### Tests to replace, not preserve

Delete tests for:

- completed persistent-owner automatic repair;
- active borrower blocking automatic repair;
- crash after repair claim;
- missing-listener terminate-before-replace;
- repair fencing sibling leases;
- repair-lock concurrency.

Add the narrower behavior matrix:

1. A ready exact service is reusable.
2. A live same-service process with a missing listener is preserved and the new
   run fails without sending a signal.
3. A live same-service process with wrong/unprovable ownership is preserved.
4. An unexpired owner or borrower lease blocks replacement of a non-reusable
   service without signaling; it does not block exact healthy reuse.
5. Explicit `down` terminates the preserved process; a later `run` succeeds.
6. A crash before process recording blocks until lease expiry, then ordinary
   reconciliation releases the reservation and retry succeeds.
7. A live `Starting` row is not promoted to ready or borrowed.
8. Missing-listener/ownership reconciliation never signals as a repair action;
   existing `until-idle` and state-upgrade lifecycle behavior remains covered by
   its current tests.

### Contract updates

- Delete the repair-only `lease-fencing` capability line and replace it only
  with any narrower lease fact that remains independently true after repair is
  gone. Do not retain a capability solely to preserve the old digest.
- Rotate the exact runtime ABI through the capability digest. Intermediate
  architecture commits may have different exact ABIs; downstream pins only the
  final revision.
- Update `AGENTS.md`, `docs/ARCHITECTURE.md`, and the RFC's normative algorithm
  in this commit. Commit 7 later compresses the implemented record; no commit
  may carry a capability/ABI whose normative documentation still promises
  automatic repair.

## Commit 3: delete derived service registry state and make escape exact

Suggested commit message:

```text
derive service state from process and leases
```

### Delete `services.status`

After repair is removed, the service status column carries no independent fact:

- readiness is `processes.status = ready` plus complete active ports;
- standing ownership is derived from service lifetime plus owner run/lease;
- borrowing is derived from open non-owner leases;
- stopped, canceled, failed, escaped, and stale mirror the primary process/run;
- reservation state before process recording is represented by the run lease,
  optional reserved ports, and process presence. The `services` row remains
  immutable SVC-ID/lifetime/state-root metadata once process ownership is
  recorded; endpoint-less services legitimately have no port rows.

Therefore:

- delete `ServiceStatus` entirely;
- delete `services.status` from the schema;
- delete unread `services.endpoint_json`; the `ports` rows are the endpoint
  authority;
- delete all writes and matches against Starting, ProbeReady, Standing,
  Borrowed, Stopped, Canceled, Failed, Escaped, and Stale service statuses;
- remove `serviceStatus` from `ps` JSON;
- delete `refresh_service_borrow_status`;
- delete both `reconcile_service_lifetime_statuses` passes;
- make borrow/release mutate only borrower leases and runs;
- make readiness/reuse read process status and port evidence;
- make unresolved escape read process status plus open ports;
- bump the registry schema once for both column deletions, with no migration,
  old-status parser, or compatibility view;
- update the capability descriptor and ABI in the same commit.

The final `ps` process object contains `processKey`, `runId`, optional
`serviceInstanceId`, `pid`, `pgid`, `registryStatus`, `reconciledStatus`,
optional `serviceLifetime`, `borrowerCount`, and `live`. It does not contain
`serviceStatus`. Record this output schema in the capability descriptor and
update the lifecycle gate assertions:

- a standing persistent process has `registryStatus = "ready"`,
  `reconciledStatus = "running"`, `serviceLifetime =
  "persistent-until-down"`, `borrowerCount = 0`, and `live = true`;
- borrower assertions use the stable service/process identity plus
  `borrowerCount`, not a service-status transition;
- after `down`, no matching process is live.

If a supposedly independent service status is discovered, stop for architecture
review. Do not retain the entire column or add a derived-state synchronizer.

### Make escape settlement mandatory

- Create one private escape settlement transaction.
- Require exact process identity and expected pre-state.
- Update process and run evidence, retain every open port, append the redacted
  event, and commit atomically.
- Validate affected-row counts.
- Return the registry failure if durable settlement fails; never discard it.
- Merge `escape_error` and `escape_error_unrecorded`.
- Delete duplicate repair escape tails already made unreachable by commit 2.

Add tests for missing/mismatched transition rows and transaction failure. No
test may accept `PROC_ESCAPE` while durable evidence is absent.

## Commit 4: use one endpoint value and one observation state machine

Suggested commit message:

```text
simplify endpoint ownership types
```

### One endpoint value

Use `SelectedEndpoint` as the single resolved endpoint placement and store the
existing typed `LoopbackHost`, not a reparsed `String`:

```text
SelectedEndpoint {
  endpoint_id,
  host: LoopbackHost,
  port,
}
```

- Delete `PlannedEndpoint`.
- Delete `EndpointFamily`; derive address family from `host.ip()`.
- Delete `planned_endpoints` and every string-to-`IpAddr` reparsing path.
- Keep exactly one ordered `BTreeMap` endpoint collection per started service,
  keyed by endpoint id. Derive the primary endpoint by lookup from
  `service.primary_endpoint`; add no second persistent index or vector.
- Use private borrowed serializers where the same endpoint must be named
  `address` rather than `host` in diagnostic/evidence JSON. Do not create a
  second stored semantic type for serialization.
- Flatten or borrow the endpoint in ownership and conflict evidence rather than
  copying endpoint id/address/port fields.

### One observation enum

- Delete `EndpointDecision` and `classify_endpoint_observation`.
- Match `OwnershipObservation` directly at the policy boundary.
- Delete the classifier round-trip test.

### Local endpoint cleanup

- Derive lock tags, socket domains, IPv6 checks, and diagnostic family strings
  from `IpAddr`.
- Sort lock acquisition by the already-derived digest/filename; do not retain
  an encoded key solely for sorting.
- Replace the no-op `EndpointLockGuard` wrapper with owned descriptors directly.
- Delete unused `Display`, derives, wrappers, and wider-than-required
  visibilities.
- Build ownership evidence from the already collected matching listeners in one
  pass; do not filter and clone the full snapshot again.
- Delete the unreachable tracked-holder branch.
- Use one matching-snapshot helper and compare ordered identities directly.
- Delete redundant post-filtering in preflight.
- Use one bounds-checked `read_array<const N>` helper for fixed-width byte
  extraction while retaining separate Linux and macOS parsers.
- Remove one-use parser result structs and duplicate inode-key sets where doing
  so reduces code without hiding the kernel protocol.
- Replace unsupported-platform runtime fallback arms with an explicit supported
  build boundary; Linux and macOS are the only runtime targets.

Expected reduction for this commit is roughly 250–400 lines without removing a
behavioral test.

## Commit 5: restore single registry authorities

Suggested commit message:

```text
consolidate registry state transitions
```

### One run-row creator

`record_run_created` is the sole authority for creating a run. It already runs
before service acquisition.

- Delete every per-service `INSERT OR IGNORE INTO runs`.
- Delete the fallback `INSERT OR IGNORE` that recreates a missing reservation
  lease during process recording.
- Process recording must validate the exact pre-existing run, active lease,
  owner token, and reserved endpoints and fail closed if they are absent.
- Delete public `RunRecord`; pass only existing internal run context or the
  minimal borrowed values required by each transition.
- Update direct tests to create the run explicitly. Do not make
  `start_service_for_slot` silently recreate it.

### One event writer

- Add one transaction-aware borrowed event insertion function to
  `registry/events.rs`.
- It must preserve the caller's transaction and apply the registry redactor.
- Make the existing standalone append path call that primitive.
- Delete event SQL and local event structs from service registry, control, and
  cleanup.
- A repository search must find exactly one `INSERT INTO events` implementation.

### One terminal service/process settlement path

- Merge stopped, canceled, and failed service settlement into one private
  transaction body.
- Keep thin callers only where they choose distinct statuses or event payloads.
- Introduce no public terminal-transition descriptor.
- Consolidate task/service canceling transaction mechanics where the same run
  and lease transition is performed.

### Other registry deletions

- Centralize exact stored-service identity matching so adding/removing an
  identity component touches one function.
- Build one borrowed `BorrowServiceRequest` and pass the under-lock policy as a
  call argument; clone endpoint/probe values only after a borrow commits.
- Reuse the process module's process-group termination primitive from `down` and
  until-idle control paths; retain the distinct unresolved-ProcessTree escape
  path.
- Reuse reconciliation row snapshots across phases without caching through a
  mutation that can terminalize a process.
- Delete the non-atomic pre-reservation `ensure_service_start_allowed` check
  when the immediately following immediate transaction performs the same gates.
- Collapse Connection/Transaction duplicate check helpers around the one
  transactional authority.

### Public API cleanup

- Make `VerifiedEndpointActivation` and `activate_service_ready` crate-private.
- Make `service::process`, `service::registry`, and `service::task` private.
- Re-export only intentional high-level library operations and result types.
- Move low-level registry tests inward or test through high-level operations.
- Audit every remaining `pub` item touched by this RFC. Visibility must be
  justified by a non-test external caller.

Expected reduction is roughly 500–750 lines including associated tests.

## Commit 6: reduce Nix and test duplication

Suggested commit message:

```text
remove duplicated validation and test scaffolding
```

### Nix

- Move the endpoint-demand capacity assertion next to the canonical
  `servicesRequired` derivation in `nix/compiler/derive.nix`.
- Delete the second derivation and its brittle coherence guard from
  `nix/compiler/validate.nix`.
- Replace `persistentMinimalModel` and `endpointCoordinationModel` with one
  persistent endpoint fixture containing `keep-up` and the prepare sentinel.
  Point both lifecycle and cross-root scenarios at that one compiled model;
  their state roots continue to provide test isolation.
- Delete the fixture's exported `keep-up` surface verb because the gate invokes
  the runtime task id directly.
- Retain separate port windows where `TIME_WAIT` isolation is intentional.

### Rust tests

- Delete the redundant external-listener/no-owner integration test; retain the
  CLI cross-process proof and the lower-level preflight/zero-mutation proof.
- Move generalized available-port-window, recursive named-file search, path
  waiting, and child-output waiting helpers to `tests/common/mod.rs`.
- Share setup/settlement assertions for cancellation-after-prepare, prepare
  failure, and spawn failure while retaining all three fault boundaries.
- Parameterize only repeated setup and assertions. Do not hide race sequencing
  in a broad test framework.

Retain all distinct coverage for:

- independent roots and simultaneous contenders;
- persistent sockets after runtime exit;
- external bind after preflight;
- multi-endpoint all-or-nothing readiness;
- lock-root ownership, permissions, symlinks, file kind, and CLOEXEC;
- exact/wildcard/IPv4-mapped/IPv6-only/TIME_WAIT behavior;
- namespace-derived lock keys;
- cancellation at each mutation boundary;
- unresolved escapes and ProcessTree descendants;
- ready transaction CAS behavior;
- macOS runtime execution.

## Commit 7: make documentation describe only the final contract

Suggested commit message:

```text
document narrow endpoint ownership contract
```

- Rewrite `RFC_CACHE_PORT_PROB.md` to retain:
  - the MFM forcing case;
  - why cache responsibility was deleted;
  - the host-global port problem;
  - the narrow lock/check/spawn/readiness/socket contract;
  - truthful error and owner-attribution rules;
  - rejected daemon/global-registry/socket-activation/auto-port alternatives;
  - consequences and residual external-race limitation.
- Delete implemented file inventories, commit instructions, exhaustive test
  checklists, migration phases, old exact paths, and automatic-repair text.
- Finish pruning `AGENTS.md`, `docs/ARCHITECTURE.md`, `README.md`, and capability
  commentary after the normative changes made with commits 2 and 3. None may
  describe endpoint acquisition as a repair/termination authority.
- Delete `IMPL_PLAN_RFC_CACHE_PORT_PROB.md` and its `.gitignore` whitelist.
- After the complete local upstream floor passes, delete this
  `FIXES_CLEANUPS.md` and its `.gitignore` whitelist in this final local upstream
  commit. The resulting revision is the one later pushed, run on hosted macOS,
  and pinned downstream; do not create another upstream cleanup revision after
  those phases. Rerun the complete local floor on that resulting revision.
- Keep one concise RFC as rationale and `docs/ARCHITECTURE.md` as the current
  architecture. Do not leave two normative operational algorithms.

The final documentation commit should be substantially negative.

## Downstream MFM commit

Do this only after the final upstream revision exists and is remotely reachable.
Do not push it from the engineer task without explicit authorization.

Suggested commit message:

```text
adopt narrowed endpoint runtime
```

- Update `flake.lock` to the final upstream revision and exact ABI.
- Preserve the sole universal `target/verification` export in `cargoLeaf`.
- Do not restore `cacheEnv`, nested crate-local targets, cache policy, or
  framework-owned cleanup.
- Update any historical text that reads as current cache responsibility. It may
  retain explicitly labeled historical observations where useful.

Pruning MFM's historical slow-build RFC/governance material is an optional,
separate cleanup and is not a completion condition for this implementation.

## Verification during implementation

After every Rust commit:

```sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test --workspace'
```

Run focused endpoint/service/registry suites while iterating, but the final
result requires the complete floor:

```sh
nix run .#ci
nix build .#nixfied-runtime
```

The required macOS hosted job must execute, not merely cross-compile. Darwin
cross-compilation remains an additional proof, not a substitute.

For downstream, first verify with an isolated local override, then after the
exact upstream pin is reachable run:

```sh
nix run .#check
nix run .#test
nix run .#test-db
nix run .#ci
```

Use isolated state for ABI/schema-negative checks. Do not make old state pass.

## Required behavioral acceptance

The final implementation must prove:

1. Cache is absent from the Nix API, model, runtime, output, state modules,
   views, capability descriptor, examples, and normal documentation.
2. Old `cacheEnv` input is rejected as an unknown field; no compatibility path
   exists.
3. Two independent roots contending for one endpoint have one prepare/spawn
   winner and one pre-mutation `PORT_CONFLICT` loser.
4. A persistent service remains protected by its socket after its starting
   runtime exits and does not inherit the startup lock.
5. Exact healthy same-registry reuse succeeds only with complete live process,
   identity, active-port, and OS-ownership proof.
6. Endpoint acquisition/reuse and missing-ownership reconciliation never signal
   a live non-reusable service as a collision/repair remedy; it fails closed
   until explicit `down`. Existing current-run, `until-idle`, state-upgrade, and
   explicit-down authorities remain unchanged.
7. Explicit `down` stops that service, after which retry succeeds.
8. Reservation-only crash evidence blocks while its lease is authoritative and
   is reclaimed after expiry without a service-repair path.
9. The same reservation-only expiry/retry behavior works for an endpoint-less
   service whose reservation has no port rows.
10. Every declared endpoint is verified before one atomic ready commit; partial
   endpoint readiness is impossible.
11. An external post-preflight bind race is `PORT_CONFLICT` when a listener is
    observed and `PORT_UNVERIFIABLE` otherwise.
12. Wildcards conflict but never satisfy exact declared ownership.
13. Owner attribution is absent unless registry, start identity, containment,
    and every observed socket record prove it exactly.
14. Failed termination cannot report a durably settled escape unless the exact
    escaped evidence transaction committed; ports remain protected otherwise.
15. macOS process enumeration has no fixed cap, uses checked growth, and is
    shared by endpoint, descendant, and process-group proofs.
16. `services.status`, `ServiceStatus`, `services.endpoint_json`, repair state,
    and repair events do not exist.
17. `ps` exposes the final process-derived schema above, and the lifecycle gate
    proves persistent, borrowed, and stopped projections without `serviceStatus`.
18. One event insertion implementation and one run-row creation authority
    remain.
19. Public endpoint/registry internals introduced only for tests are gone.
20. Linux execution, Darwin cross-compilation, and hosted macOS execution pass.
21. MFM emits the final ABI, contains no cache objects, and has exactly one
    `target/verification` anchoring path.
22. Both worktrees are clean and the final LOC/stat report is included in the
    handoff.

## Deletion audit

At minimum, repository searches must show no production definitions or calls
for:

```text
RepairOutcome
repair_requested_service
repair_candidate_endpoints
guarded_claim_service_repair
mark_repair_target_stale
refuse_unexpired_repair_leases
repair_snapshot_matches
durable_owner_is_eligible
recover_starting
inspect_unrelated_endpoint_evidence
open_port_service_instances
ServiceStatus
serviceStatus
endpoint_json
PlannedEndpoint
EndpointFamily
EndpointDecision
classify_endpoint_observation
```

Additional structural checks:

- exactly one macOS `proc_listallpids` enumeration loop;
- exactly one `INSERT INTO events` implementation;
- exactly one production `INSERT` path for `runs`;
- no fixed 8,192-PID buffer;
- no signal path reachable from endpoint reuse/acquisition as collision or
  repair resolution; existing lifecycle teardown authorities are unchanged;
- no unsupported-platform runtime fallback branch;
- no completed implementation-plan files after final acceptance;
- no new public model type, command, registry table, semantic artifact, or
  runtime dependency.

Intentional negative tests may still contain removed vocabulary such as
`cacheEnv`. Historical rationale may mention it only where clearly described as
deleted behavior.

## Final review checklist

Before committing the final upstream state, review the complete diff against:

- the narrow endpoint acquisition algorithm above;
- `MODEL-SEAM-1`, `SVC-ID-1`, `PORT-1`, `REG-1`, `LIVE-1`, `PROC-1..3`,
  `REDACT-1`, `DERIVE-1`, and the negative `CACHE-1` boundary;
- absence of automatic repair or implicit destruction;
- one authority for each durable registry fact;
- derived facts not persisted;
- no compatibility or fallback code;
- public API contraction;
- deletion of completed scaffolding;
- final test evidence and unverified-platform disclosure;
- final upstream and downstream diff statistics.

Do not declare completion because tests pass while any required deletion remains.
The finished endpoint design should be easier to explain: lock, observe, start,
verify, release; reuse only exact ready ownership; explicit `down` before
replacement of a non-reusable live service.
