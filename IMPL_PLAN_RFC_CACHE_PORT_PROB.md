# Implementation plan: remove cache semantics and make port conflicts host-aware

- Status: ready for engineer handoff
- Target: RFC_CACHE_PORT_PROB.md
- Upstream repository: nixfied
- Downstream repository: ../mfm3
- Contract event: one exact runtime ABI rotation
- Architecture review: completed under the zero-compatibility, delete-old-code,
  and fewer-concepts rules

## Outcome

Implement the RFC as a correction of existing boundaries:

1. Delete cache from Nixfied's model, Nix compiler, runtime, output, views,
   examples, gates, tests, and documentation.
2. Keep tool acceleration entirely child/tool/project-owned through ordinary
   invocation environment or arguments.
3. Complete the existing endpoint responsibility with one private,
   endpoint-keyed startup lock and one private host listener observer.
4. Keep the listening socket as steady-state ownership. Do not add a daemon,
   global registry, lifetime lock holder, socket activation, dynamic port
   selection, or another semantic seam.

The finished tree has fewer model fields, public types, status variants,
validation branches, output fields, and state modules. The endpoint correction
adds private OS machinery only where the existing service contract cannot be
made correct without it.

## Non-negotiable implementation rules

These rules apply to every commit in this plan:

- No backward compatibility. Old models containing cacheEnv fail as unknown
  input under the new exact ABI.
- No aliases, ignored fields, deprecation paths, migrations, old-layout
  discovery, old-cache cleanup, feature switches, or mixed-runtime handling.
- Delete superseded code. Do not leave forwarding modules, unused variants,
  dormant tests, commented implementations, or alternate scanners.
- Add no model field, public model type, public command, registry table,
  dependency, adapter protocol, or user-selectable lock path.
- Prefer one implementation path for observation, readiness, conflict
  construction, service termination, ready-state transition, and registry
  reconciliation.
- Keep all endpoint types crate-private. Stable JSON is a serialization
  contract, not a reason to expose Rust payload types.
- Preserve unrelated user work. Inspect each repository's status before editing
  and before every commit.
- Each commit message is lower-case, one concise line, with no body or trailers.
- Do not create the upstream commit until its full proof is green. If review
  later exposes a production defect, amend that same contract cut instead of
  adding a repair, workaround, or follow-up compatibility path.

## Decisions already closed

The engineer must implement these decisions as written. They are not design
questions left for implementation.

### Lock boundary

prepare_slot_state remains run-wide and executes before endpoint locking. The
RFC phrase “state-mutating lifecycle operation” means service-specific endpoint
reservation, service prepare, terminate-before-replace, spawn, and readiness
state transitions. It does not mean registry opening, mandatory reconciliation,
slot marker adoption, epoch upgrade, or run.created evidence.

Do not create a run-wide lock phase. Exact reusable services never take an
endpoint startup lock.

The per-service order is:

1. Reconcile and inspect a reusable candidate.
2. If exact endpoint ownership is proven, guarded-borrow it and return.
3. If repair/new start is required, acquire all endpoint locks in canonical
   order.
4. Reconcile, reload, and re-observe under the locks.
5. Claim and terminate an invalid local service when allowed.
6. Prove observer support and preflight every endpoint.
7. Reserve locally, run service prepare, spawn, and record the process.
8. Evaluate probe plus ownership through one readiness budget.
9. Atomically activate all endpoints and ready states.
10. Release the startup locks.

### Listener relations

Use two relations, both in the same private endpoint module:

- conflicts(planned, observed) follows host bind overlap and includes relevant
  IPv4/IPv6 wildcard and dual-stack listeners.
- satisfies_declared_endpoint(planned, observed) requires the exact canonical
  family, address, and port.

A wildcard listener can block an exact loopback endpoint, but it cannot satisfy
that declaration. An expected process listening only on a wildcard address
therefore consumes the existing readiness attempts and ultimately gets
READINESS_TIMEOUT. If it also owns an exact listener, the exact listener
satisfies readiness, but every distinct overlapping kernel socket record must
still correlate to the tracked containment without a visible outside holder.

TIME_WAIT is never a listener and never proves ownership.

### Readiness and error precedence

One loop owns the declared maxAttempts, retryIntervalMs, per-attempt timeout,
probe attempt, endpoint observation, cancellation check, and liveness check.
Do not run a full probe retry loop followed by another ownership retry loop.

The readiness predicate is:

~~~text
probe succeeds
AND every declared endpoint has an exact listener
AND every distinct overlapping socket record correlates to at least one live
    tracked holder
AND no distinct overlapping socket record has a visible outside holder
~~~

Classification is:

- startup lock contention or a proven outside listener: PORT_CONFLICT;
- incomplete or unsafe listener/ownership inspection: PORT_UNVERIFIABLE;
- escaped or unconfirmable runtime-owned containment: PROC_ESCAPE;
- successfully observed absence through the readiness deadline:
  READINESS_TIMEOUT;
- cancellation remains CANCELED after owned-child cleanup.

Before returning an early-exit, probe-failure, or timeout result, re-observe the
endpoints. Override the weaker result only with truthful OS evidence. Never
parse child stderr.

### Reuse, repair, and borrowers

A missing exact listener or a proven wrong listener prevents reuse. It is not
PORT_UNVERIFIABLE when inspection itself completed.

Classify leases from one joined transaction using existing rows. The primary
process run_id identifies the owner; other open rows for the service are
borrowers.

- Any unexpired finite-TTL owner or borrower lease returns LEASE_CONFLICT
  without signaling.
- An expired finite-TTL lease revokes that run/owner token as a unit: the repair
  CAS marks every open lease for that run/token Stale. A heartbeat must observe
  the fence before it can update any sibling row.
- A durable persistent owner is different from a live run lease. It is
  fenceable for this service only when serviceLifetime is
  persistent-until-down, reconciliation left the service Standing, the owner
  run is successfully terminal (Completed or TaskSucceeded), the owner lease is
  Active with the existing 9999 expiry, and no open borrower remains after
  expired borrower tokens are fenced. Mark only this persistent owner row
  Stale; a lingering heartbeat cannot refresh any sibling after observing it.

After acquiring endpoint locks, repeat the classification and lease mutations
inside the repair CAS. Use the existing ServiceStatus::Starting as the
non-borrowable repair claim; add no repair status or repair lease. Termination
and terminal/stale registry transitions occur only after all endpoint locks are
held and the candidate was re-observed. Use the existing declared stop signal
and stop timeout, capped by the run timeout; add no timeout field.

record_service_borrow becomes a guarded transaction/CAS. It may insert a borrow
while the service is in any reusable status (ProbeReady, Standing, or Borrowed)
and only while primary process identity plus the complete active endpoint set
still match the candidate just proven reusable. This permits multiple legitimate
concurrent borrowers. The repair claim must atomically apply the lease rules
above and move the service to Starting. A losing borrower/repairer changes no
rows.

Replace mark_process_escape with an unresolved-escape transition. When
termination is unproven, set that process and service to their existing Escaped
statuses, set the requesting run to ProcEscaped, retain the port rows and lease
history exactly as fenced/classified, and append the existing event evidence.
Escaped plus a PORT_OPEN row is the existing durable representation of an
unresolved resource holder. One central reconciliation/control query must
include PROCESS_ACTIVE rows plus Escaped rows with open ports, so ps, down,
cleanup safety, and later mutating reconciliation continue proving OS liveness.
When the group is later proven gone, release/stale the ports and leave the
process/service terminal Escaped. Never restore an Escaped service to reuse,
even if it reopens the exact listener before termination succeeds.

A repairer can die after the guarded CAS changes a live service to Starting and
before it signals the process. Starting is therefore a recoverable in-progress
state, not corruption. It is distinguishable from failed termination because
its service/process are not Escaped. On the next mutating request, after
acquiring the same endpoint locks, reload and re-observe that row:

- if the original process is gone, stale the process/service/ports and continue;
- if it again proves exact ownership and identity, transactionally restore its
  reusable standing state and guarded-borrow it;
- if it remains missing/wrong, repeat the guarded lease classification/repair
  claim and terminate-before-replace;
- if inspection is unverifiable, preserve Starting, process, and port rows and
  return PORT_UNVERIFIABLE; down remains available through process proof.

The ordinary reconciliation pass that runs before endpoint locking must
recognize a live Starting service with its active process/port evidence as
recoverable and preserve it. It must not terminalize, stale, or borrow that row.
Only the endpoint-locked mutating path above may restore or repair it. An
Escaped service with open ports follows the unresolved-escape rule instead and
is never restored.

ps stays process-only and does not perform this endpoint repair.

### Registry-only rows

After mandatory reconciliation:

- an unexpired open reservation lease with no process is LEASE_CONFLICT;
- no open lease and no live recorded process is staled/released and startup may
  proceed;
- a live recorded process with missing/wrong endpoints enters the repair path
  only for the requested service instance; an unrelated instance is preserved
  and returns PORT_UNVERIFIABLE until explicitly stopped;
- a row with neither a valid lease nor a valid live process after successful
  reconciliation is REGISTRY_CORRUPT.

No registry gate emits PORT_CONFLICT. PORT_CONFLICT is reserved for endpoint
lock contention or a proven kernel listener and always has the exact structured
details.

### ps and down

Do not add endpoint observation to ps reconciliation and do not add a
reconciliation-mode enum. ps remains process liveness and must not signal a
process solely because a listener is missing or unverifiable. Existing
until-idle lease reconciliation remains unchanged.

down continues to use proven process ownership and must remain available when
endpoint inspection is unverifiable.

### Platform observers

There is one private listener observer contract and no old-parser fallback.

On Linux:

- use NETLINK_SOCK_DIAG for complete TCP_LISTEN dumps of AF_INET and AF_INET6;
- retain family, canonical local address, port, inode/cookie, UID, and
  INET_DIAG_SKV6ONLY;
- delete /proc/net/tcp and /proc/net/tcp6 as listener authorities;
- use /proc/PID/fd only to correlate each distinct matching kernel socket record
  with accessible FD holders, then use existing process-group/start-identity
  primitives;
- compare kernel listener identity before and after holder mapping, using a
  bounded immediate resnapshot, and fail unverifiable on persistent churn.
- reject NLM_F_DUMP_INTR and require INET_DIAG_SKV6ONLY on every relevant IPv6
  listener whose dual-stack behavior changes overlap.

On macOS:

- use sysctlbyname(net.inet.tcp.pcblist_n) for the kernel listener snapshot;
- parse the self-describing xinpgen/xinpcb_n/xsocket_n/xtcpcb_n records;
- retain family/vflag, address, port, PCB/generation, UID/PID hints, and the
  flags needed to decide IPv6-only versus dual-stack overlap;
- dynamically size and retry proc_listallpids, proc_pidinfo, and
  proc_pidfdinfo buffers while correlating accessible FD holders;
- correlate xsocket_n.xso_so with socket_info.soi_so and validate xinpgen
  header/trailer generations plus every record length, kind, and alignment;
- treat kernel PID hints only as candidates; live FD/process/start-identity
  correlation remains the ownership proof;
- delete the current fixed PID/FD caps and silent truncation behavior.

Kernel listener evidence is enough to prove a preflight conflict even when its
holder is not attributable. Readiness/reuse treats each distinct kernel socket
record as the proof unit and assigns one result to that record: a live tracked
holder is proven, a visible outside holder is proven, or the record is
unmatched/unverifiable. Do not carry a global attribution-completeness boolean
and do not fail because an unrelated process table is inaccessible. The runtime
does not claim it can prove absence of inaccessible co-holders of the same
kernel socket. Unsupported record layouts, material flag absence, inability to
correlate a matching record as required, truncation, malformed dumps, or
unstable snapshots are PORT_UNVERIFIABLE.

Unsupported platforms fail with PORT_UNVERIFIABLE before service mutation.
There is no shell, lsof, netstat, proc-table listener fallback, or platform
feature switch.

## Commit topology

The work is divided into two coherent commits in two repositories. Nixfied's
exact contract, production code, tests, gate, CI, views, and documentation land
atomically. MFM adopts that completed revision in its own commit.

| Order | Repository | Commit message | Purpose |
| --- | --- | --- | --- |
| 1 | nixfied | remove cache contract and harden endpoint acquisition | Atomic contract cut: deletion, production, full proof, docs, descriptor, and the single ABI rotation |
| 2 | mfm3 | adopt project-owned verification target | Delete cacheEnv adoption, update the pin, and transfer target lifecycle responsibility to MFM |

Cache deletion and the public port correction intentionally share commit 1.
Splitting them into separate contract commits would rotate the capability ABI
twice and create an unsupported intermediate contract. Separating required
acceptance proof from the ABI change would also land an unproven public
contract. Commit 1 may be developed in smaller local steps, but no intermediate
semantic seam is committed.

## Commit 1: remove cache contract and harden endpoint acquisition

Commit message:

~~~text
remove cache contract and harden endpoint acquisition
~~~

This is the only upstream commit. It edits capability.txt and the runtime ABI
snapshot once and includes all required acceptance proof. It must compile, test,
and document one internally consistent exact contract.

### A. Delete cache from the Nix authoring/compiler layer

In nix/modules/primitives.nix:

- delete cacheKeyType and cacheEnvType;
- delete invocation.cacheEnv and all defaults/descriptions for it;
- leave Invocation with tools, run, executable, env, codebaseId, cwd, stdin,
  and timeoutMs only.

In nix/compiler/derive.nix:

- delete cacheScope and normalizeCacheEnv;
- delete collision calculation and assertions;
- delete cache field emission and optionalAttrs branches.

In nix/compiler/validate.nix:

- delete validCacheComponent, cacheScope, cache name/collision/spec helpers,
  invocationCacheEnvValid, serviceCacheEnvEmpty, allCacheEnvValid, and their
  checks/messages;
- retain ordinary environment-name validation and the existing hermetic PATH
  rule;
- do not add an “old field” parser at compiler level. The removed Nix option is
  rejected naturally by the typed module system.

In nix/compiler/views.nix:

- remove CacheEnvSpec and CacheKeySpec from generated model type projections.

In nix/gate-nix-negatives.nix:

- delete the six cache semantic negative cases;
- retain exactly one authored use of old cacheEnv proving it is now an unknown
  Nix option.

### B. Delete cache from the Rust model and views

In nixfied-model:

- delete InvocationSpec.cache_env;
- delete CacheEnvSpec, CacheKeySpec, CacheMode, and CacheScope;
- delete their exports, validation helpers, validation branches, errors, and
  round-trip/semantic tests;
- simplify validate_invocation by removing allow_cache_env and every call-site
  boolean;
- keep deny_unknown_fields unchanged;
- replace cache positives with one negative proving a cacheEnv JSON field fails
  deserialization as unknown.

In nixfied-cli:

- remove CacheEnvSpec and CacheKeySpec from schema_view;
- update schema/view tests and expected projections;
- add no replacement “resource” type.

Runtime admission must map a model containing cacheEnv to MODEL_INVALID. It must
not ignore the field and must not reach lowering.

### C. Delete cache from lowering, execution, state, and evidence

In execution/types.rs and execution/lower.rs:

- remove CacheMode/CacheScope re-exports;
- remove ResolvedInvocation.cache_env and ResolvedCacheEnv;
- delete cache lowering, allow-cache arguments, cache rejection variants,
  messages, empty initializers, and tests.

Delete runtime/crates/nixfied-runtime/src/state/cache.rs completely. Then:

- delete its module declaration and exports from state/mod.rs;
- make placement.rs materialize_owned_dir private because its only remaining
  callers are in that module;
- keep canonicalize_existing crate-private because cleanup still uses it;
- do not scan, migrate, clean, or mention old cache directories.

In service/task.rs:

- delete TaskCacheEvidence and TaskRun.cache_env;
- delete materialize_task_cache_env, cache-specific environment insertion, and
  task-command cache evidence;
- construct the hermetic child environment directly from declared env plus the
  runtime PATH;
- remove target_json, runtime_abi, and toolchain_id from RunContext;
- remove the same cache-only copies from StartedService and every constructor;
- keep run_id, computed_model_hash, source/state roots, secrets, and redactor
  because task execution and evidence still use them.

Update main.rs and all fixtures/callers to stop constructing the deleted
fields. No dependency should be added or retained solely to replace cache
digesting; sha2 and hex remain independently needed by endpoint lock keys.

In tests/state.rs:

- delete the two cache materialisation tests and cache imports/fixtures.

In tests/service.rs:

- delete the cache materialisation/evidence test;
- extend the existing hermetic environment test to declare an ordinary
  CARGO_TARGET_DIR (using either a project-relative value or the existing
  stateDir substitution) and prove it reaches the child;
- assert output no longer has cacheEnv.

### D. Delete cache from examples and framework gates

In examples/downstream/nixfied.nix:

- delete cacheEnv.NIXFIED_SLOT_CACHE and any cache-specific assertions.

In examples/toolchain/nixfied.nix:

- delete NIXFIED_RUN_CACHE/NIXFIED_EXAMPLE_CACHE declarations and directory
  assertions;
- retain the example's heterogeneous toolchain behavior using ordinary task
  execution.

In nix/gate-runtime/nixfied.nix:

- delete cache-run-scope and its run-path comparison;
- delete cache JSON/path assertions from slot isolation;
- delete the cache composite/task node;
- connect dependent gate tasks directly so no empty cache wrapper remains.

In flake.nix:

- delete the cache-run-scope model injection and any now-dead wiring.

Do not delete ordinary services named cache in graph/derivation fixtures. Those
are adopter identifiers, not the removed primitive.

### E. Reject statically infeasible port plans

In nix/compiler/validate.nix:

1. Reuse deriveFacts.servicesRequired with the existing tasks, services, and
   prepareTaskOf function.
2. For each declared task, sum endpointIds for its derived service closure.
3. Require the demand to be at most placement.ports.windowSize.
4. Place this check after task/service reference and cycle checks so invalid
   graphs retain their direct diagnostics.
5. Do not emit another derived fact or add a model field.

Add one nix/gate-nix-negatives.nix case with a one-port window and a selectable
task whose derived service closure needs two endpoints.

In execution/plan.rs:

- compute the total endpoint demand before assigning a cursor;
- if demand exceeds the candidate window, return MODEL_ADMISSION;
- after that proof, assign without exhausted/partial-binding state;
- update all feasibility tests that currently expect PORT_CONFLICT;
- keep Admission::admit_checks as the single caller of
  prove_all_plans_feasible.

After this change, post-admission planning has no capacity-exhaustion error path.

### F. Implement one private endpoint authority

Delete service/ownership.rs and create service/endpoint.rs in its place. In
service/mod.rs, replace pub mod ownership with mod endpoint and delete the
verify_endpoint_ownership re-export; leave no forwarding module, alias, or old
scanner. Update process.rs to use only the crate-private endpoint API.

The readiness refactor also removes the public wait_for_tcp_probe re-export and
makes readiness.rs crate-private; replace integration tests of that helper with
service-level behavior tests.

service/endpoint.rs owns:

- canonical endpoint parsing;
- the normalized endpoint key and full SHA-256 lock filename;
- fixed-root descriptor traversal and directory/file validation;
- sorted nonblocking flock acquisition and RAII guards;
- raw temporary socket/bind preflight;
- Linux/macOS kernel listener snapshots;
- conflict overlap and exact-satisfaction predicates;
- listener-to-process attribution and containment proof;
- private typed evidence for lock contention, listener occupancy, missing exact
  listeners, complete ownership, and unverifiable observation.

Registry SQL, service repair decisions, process signaling, and readiness-loop
control remain outside endpoint.rs. endpoint.rs does not construct RuntimeError
values and does not know the portConflict JSON or registry owner shape.

Use one private module. Platform-specific unsafe parsing may live in
service/endpoint/linux.rs and service/endpoint/macos.rs for auditability, but do
not add a public trait, observer registry, lock manager abstraction, or alternate
observer.

#### Normalized key

Use a fixed binary encoding and snapshot it:

~~~text
"tcp" + NUL
network tag and bytes:
  Linux: "linux" + NUL + stat.st_dev(u64 big endian) + stat.st_ino(u64 big endian)
  macOS: "host" + NUL
family tag: 4 or 6
canonical address bytes: 4 or 16 bytes
port: u16 big endian
~~~

Hash exactly those bytes with SHA-256. The filename is all 64 lowercase hex
digits plus .lock. Project, environment, service, endpoint id, state root,
model hash, ABI, and toolchain never enter the key or path.

Read Linux network scope from stat(/proc/self/ns/net). Failure is
PORT_UNVERIFIABLE. Mount-namespace separation emerges from resolving the fixed
temporary root in each namespace; do not encode mount identity.

#### Safe fixed root

Production starts from an O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC descriptor for
the filesystem root and walks only:

~~~text
Linux: private root -> tmp -> nixfied-<euid> -> endpoint-locks
macOS: private root -> private -> tmp -> nixfied-<euid> -> endpoint-locks
~~~

Validate the system temporary directory as a real root-owned sticky directory.
Create runtime-owned components with mkdirat mode 0700, then openat/fstat and
require effective-user ownership and exact 0700 permissions. Never trust a
pathname after validation.

Create a lock target with an O_CREAT | O_EXCL | O_RDWR | O_NOFOLLOW |
O_CLOEXEC attempt at 0600; on EEXIST open the existing inode with O_RDWR |
O_NOFOLLOW | O_CLOEXEC. fstat must prove a regular effective-user-owned 0600
file. Never truncate, write, rename, replace, or unlink it.

Acquire flock(LOCK_EX | LOCK_NB). Multi-endpoint acquisition sorts encoded keys
bytewise and releases the full partial set on first failure. Lock contention
returns typed endpoint evidence; process.rs maps it to PORT_CONFLICT with reason
startup-lock-contended and no nixfiedOwner.

Guards are stored privately on a newly StartedService. Borrowed services hold no
guards. All FDs are close-on-exec and cannot reach prepare, service, task, probe,
stop, or cleanup children.

#### Preflight

While holding all locks, establish that the platform observer can produce a
complete, parseable kernel socket snapshot even when bind succeeds. This support
check does not require global process-table/FD attribution when no listener
needs ownership proof. Then, for every endpoint:

1. Create a CLOEXEC TCP socket for the planned family.
2. Set no reuse option and bind the exact address/port.
3. Never call listen and never hand the FD to a child.
4. On success, close it and continue.
5. On EADDRINUSE, immediately take a second kernel snapshot:
   - a conflicting LISTEN yields listener-occupied evidence;
   - no conflicting LISTEN yields unverifiable evidence.
6. Any other unsafe/ambiguous socket or inspection failure yields
   unverifiable evidence.

Attribution is best-effort for a preflight conflict: kernel listener existence
is sufficient, and an unproven owner is omitted.

#### Listener proof

The observer returns all matching kernel listeners with per-record correlation
evidence. It must not collapse “listener exists” into “owner mapped” or add a
second global attribution-completeness result.

For readiness/reuse:

- at least one exact listener must satisfy every declared endpoint;
- every distinct exact, wildcard, mapped, or dual-stack kernel socket record
  that conflicts must correlate to at least one process inside the tracked
  process group/tree;
- every mapped process needs a non-null live start identity;
- an unmatched socket record, inaccessible material FD table needed to
  correlate that record, or unstable snapshot
  is PORT_UNVERIFIABLE;
- a distinct record whose kernel socket UID differs from the runtime effective
  UID, or with a visible proven outside holder, is PORT_CONFLICT;
- a complete snapshot with no exact listener is a pending/missing observation,
  not an error synthesized by the observer.

### G. Integrate locking, reuse, and one readiness loop

In service/process.rs, preserve this orchestration:

Pass the existing run CancellationToken into start_service_for_slot; do not add
a second token or timeout. Check it after lock acquisition, before and after
prepare, immediately before spawn, after process recording, and in every
readiness attempt. A cancellation observed before a child exists releases the
reservation and locks; after spawn it uses the same terminate-confirm finalizer.

1. Ordinary registry/process reconciliation.
2. Read the candidate service, process, endpoint, and borrower rows.
3. Observe endpoint ownership for a possible exact reuse.
4. Guarded borrow and return if exact.
5. Apply the finite-versus-durable lease classification above; return
   LEASE_CONFLICT for any unexpired finite owner/borrower.
6. Acquire every endpoint lock in sorted order for new/repair work.
7. Reconcile, reload all relevant rows, and repeat OS proof while locked.
8. If the service became reusable, guarded-borrow it and release the locks.
9. Transactionally recheck all leases, fence expired finite run tokens or the
   eligible durable persistent owner as specified above, and CAS the invalid
   service to Starting.
10. Terminate its contained group through the existing TERM/KILL implementation,
    confirm it is gone, and terminal/stale its rows. If confirmation fails,
    record the unresolved Escaped state without releasing ports.
11. For any unrelated local service row on a requested endpoint: reconcile dead
    evidence, return LEASE_CONFLICT for a lease-only reservation, or return
    PORT_CONFLICT for a proven live listener. A live unrelated process with no
    listener is preserved and returns PORT_UNVERIFIABLE until explicit down.
    Never terminate a different service instance as part of this repair.
12. Run complete-observer check and preflight for every endpoint.
13. Reserve the service/endpoints locally.
14. Run prepare, spawn, and record the process while retaining guards.
15. Run the combined probe/ownership readiness loop.
16. Commit every ready/active transition in one registry transaction.
17. Explicitly consume/release guards after that transaction commits.

The under-lock reload in step 7 must recognize a live Starting row left by a
crashed repairer and apply the recovery table above. It must separately
recognize Escaped plus open ports and retry termination without ever restoring
reuse. Record recovery in the existing event stream; add no repair table,
status, lease, or retry worker.

Move guards through the existing start/readiness API split as a private
StartedService field. Rely on RAII for unwinding/runtime death, but make success
and failed-start cleanup ordering explicit rather than depending on incidental
field drop order.

Before StartedService exists, keep the guards in the start stack. Replace the
failure-only release_service_reservation function with one private
outcome-aware settlement transaction used only when no child exists or child
termination is proven:

- primary cancellation records lease/run Canceled, releases ports, then guards;
- prepare or spawn failure records lease Failed/run ServiceFailed, releases
  ports, then guards;
- spawn success followed by process-record failure must terminate and confirm
  the new group before settling the reservation and releasing guards;
- if that confirmation fails, preserve reservation rows, release only the
  ephemeral guards, and return PROC_ESCAPE.

Refactor readiness.rs so probe functions execute one attempt. process.rs owns
the single retry loop and calls the endpoint observer in the same attempt. Use
the same machinery for TCP and invocation probes; do not create separate
endpoint retry counters.

Centralize failed-start finalization. It must:

- return CANCELED only when cancellation was the primary cause and owned-child
  cleanup/containment confirmation succeeds;
- preserve an already-proven non-cancellation failure if cancellation arrives
  later;
- re-observe before returning early-exit/probe/timeout failures;
- construct PORT_CONFLICT only for proven lock/listener evidence;
- return PORT_UNVERIFIABLE for incomplete per-record proof;
- preserve the original truthful lifecycle/readiness code when no stronger
  cause exists;
- terminate and confirm the group before releasing registry reservations and
  startup guards;
- return PROC_ESCAPE and preserve reservations whenever termination cannot be
  proven, overriding cancellation or the original startup error.

Keep the evidence-to-action/error precedence in one private pure classifier in
process.rs. Unit tests can feed complete, missing, foreign, and unverifiable
endpoint evidence into that classifier without an observer override. The real
start path is its only production caller.

Delete duplicated cleanup branches that the finalizer replaces. The expected
result is fewer failure paths in wait_for_probe_ready_cancellable, not a wrapper
around all old branches. In main.rs, keep the current StartedService local while
readiness and health execute. The ready transaction releases startup guards;
health then runs without them. Push the service into started only after health
succeeds. On either failure, pass the local value into one consuming private
finalizer, then tear down only the earlier services already in started. Merge
the duplicate readiness/health failure branches. The finalizer owns the local
child/borrow decision exactly once, so vector removal, Drop re-signaling, and
child.is_none() borrowed conflation cannot occur.

### H. Make registry transitions exact

In service/registry.rs:

- replace blind record_service_borrow updates with the guarded borrow
  transaction described above;
- add a guarded repair-claim transaction using existing ServiceStatus::Starting;
- centralize the joined read needed for service/process/port/lease proof instead
  of leaving duplicate SQL in process.rs;
- change active reservation handling to LEASE_CONFLICT;
- classify an impossible post-reconcile orphan row as REGISTRY_CORRUPT;
- delete the shape-less registry PORT_CONFLICT constructor;
- replace mark_endpoint_owner_verified plus mark_service_probe_ready with one
  transaction that:
  - marks every endpoint Active,
  - marks the process Ready,
  - marks the service ProbeReady,
  - records the ready lifecycle success,
  - appends all ownership/readiness events in the existing total order,
  - validates exact affected-row counts plus the expected endpoint keys and
    pre-transition statuses before commit;
- delete the superseded functions and tests.

In registry/leases.rs:

- make heartbeat_run_lease one atomic fence-aware transaction;
- if any lease for the run/owner token is Stale, update no sibling row and
  return LEASE_STALE;
- otherwise update every open sibling as today; if no row is open, return
  success only when all rows are clean terminal statuses (Completed, Canceled,
  or Failed), never when any row is Stale;
- delete the old “any updated row means success” path.

In control.rs and the shared registry read path:

- reconcile PROCESS_ACTIVE rows and Escaped rows that still have a PORT_OPEN
  record through the same OS-liveness machinery;
- express the latter as the joined process/service/port predicate; do not add
  Escaped to PROCESS_ACTIVE globally, or terminal rows whose ports were already
  released would remain actionable forever;
- keep such Escaped rows visible to ps/down and blocking cleanup while their
  group may live;
- after group death is proven, release/stale the ports while retaining terminal
  process/service Escaped evidence;
- do not add an endpoint-reconciliation mode or inspect event history as state.

In registry/status.rs:

- delete unused PortStatus::Binding and PortStatus::Bound;
- define PORT_OPEN as Reserved and Active only;
- update round-trip/classification tests.

Do not add a table, column, migration, or compatibility acceptance for old
status strings. The exact ABI/state identity already rejects the old contract.

Replace mark_process_escape rather than merely changing its port update. A
possibly live unconfirmable group becomes process/service Escaped while its
ports stay open; the requesting run/event records PROC_ESCAPE. The joined
control/reconciliation query keeps that composite state actionable. Only after
process/group death is proven are ports released/staled; Escaped evidence stays
terminal. A crashed repair claim remains Starting and follows the separate
recovery table.

### I. Make PORT_CONFLICT one exact public diagnostic

process.rs owns error precedence, local registry attribution, and the only
RuntimeError construction with ErrorCode::PortConflict. It maps the typed OS
evidence from endpoint.rs into a private borrowed serializer that emits:

~~~text
details.portConflict = {
  reason,
  projectId,
  endpoint: {
    transport,
    family,
    address,
    port,
    endpointId
  },
  nixfiedOwner?: {
    projectId,
    environment,
    slot,
    runId,
    serviceId,
    serviceInstanceId,
    processKey
  }
}
~~~

The requesting projectId is always present. The generic runId, environment,
slot, and failedService details remain siblings added by the current error
enrichment.

Include nixfiedOwner only when one local registry proves:

- an active endpoint row for the exact socket;
- an exact service/process-key association;
- a live primary PID/PGID with non-null matching start identity;
- matching current service identity and containment;
- every distinct observed listener socket record correlates to at least one
  holder in that containment with a live start identity, and no visible holder
  is proven outside it.

Lock contention never has owner attribution. Cross-state-root listener
conflicts normally omit it because this runtime cannot inspect another private
registry. Do not expose an external PID, command line, environment, secret, or
lock-file content.

Audit production constructors. Planner, registry, readiness, and endpoint code
may return/match typed evidence or ErrorCode::PortConflict, but only the one
private process.rs builder may construct that runtime error.

### J. Rotate the exact contract once

Edit runtime/crates/nixfied-model/capability.txt once:

- remove cacheEnv from Invocation;
- delete CacheEnvSpec, CacheKeySpec, task-cache-evidence, CacheMode, CacheScope,
  and cacheEnv output fields;
- change PortStatus to reserved active released stale;
- add output schemas for portConflict, its endpoint, and optional nixfiedOwner;
- add the closed reason values startup-lock-contended and listener-occupied;
- record the strengthened host-aware endpoint acquisition/reuse semantics,
  finite run-token fencing, and Escaped-plus-open-port reconciliation.

Do not change modelVersion, toolchainId, or the runtime ABI base. Recompute the
capability-derived suffix and update the single snapshot in
nixfied-model/src/constants.rs.

Extend capability_coverage.rs with explicit stale-token rejection because the
existing field-coverage test detects missing tokens but not obsolete extra
vocabulary. Assert that removed cache type/field/status tokens do not survive.

Update generated view/schema expectations and prove Nix and Rust calculate the
same suffix.

### K. Update contract documentation in the same commit

Update:

- AGENTS.md;
- README.md;
- docs/ARCHITECTURE.md;
- docs/ADAPTERS.md;
- RFC_CACHE_PORT_PROB.md only to mark implementation completion or keep exact
  code names synchronized. This planning change has already reconciled its
  normative sequencing and classification decisions.

Required documentation changes:

- replace CACHE-1 with the negative child/tool/project-owned boundary;
- describe hermetic child environment as declared env plus runtime PATH;
- remove cache placement, cache evidence, modes, scopes, and cleanup language;
- strengthen PORT-1 and clarify REG-1's transient OS-lock allowance;
- say wildcard collision and exact endpoint satisfaction are different;
- say ps does not signal solely for listener loss, while existing until-idle
  reconciliation remains;
- record active-borrower LEASE_CONFLICT, missing-listener READINESS_TIMEOUT,
  orphan REGISTRY_CORRUPT, and slot-preparation ordering;
- record finite run-token fencing, the completed persistent-owner exception,
  and fence-aware heartbeat behavior;
- record Escaped-plus-open-port as unresolved ownership visible to ps/down and
  cleanup until OS proof permits port release;
- record Linux SOCK_DIAG and macOS sysctl observer authorities;
- keep socket ownership after readiness and the endpoint-less carve-out
  unchanged.

Do not create a migration guide for old cache state. Git history is the only
code recovery mechanism.

### L. Focused tests required in commit 1

Add or update unit/component tests for:

- old cacheEnv JSON -> unknown field -> MODEL_INVALID;
- old Nix cacheEnv option -> evaluation rejection;
- ordinary declared CARGO_TARGET_DIR reaches a hermetic leaf;
- task/node/run/summary/error JSON contains no cache evidence;
- an ordinary child-written path under ${stateDir} is removed only by the
  existing whole-slot clean path, with no selective interpretation;
- static Nix capacity rejection and runtime MODEL_ADMISSION;
- deterministic endpoint-key bytes and SHA-256 filename goldens;
- Linux network stat device/inode and macOS host scope;
- multi-lock sort order and partial-set release;
- safe directory ownership/mode/no-follow/regular-file checks;
- nonblocking flock, stable inode reuse, no write/unlink, and FD_CLOEXEC;
- raw bind success, EADDRINUSE+LISTEN conflict, and bind failure without listener;
- exact IPv4, IPv4 wildcard, exact IPv6, IPv6 wildcard, mapped IPv4,
  IPV6_V6ONLY on/off, co-bound listeners, and TIME_WAIT exclusion;
- Linux dump interruption/missing-v6only rejection and macOS generation,
  record-kind/length/alignment, and socket-address correlation rejection;
- tracked, visible-outside, and unmatched per-record holder evidence;
- wildcard conflict versus exact satisfaction;
- error JSON snapshots for both reasons, with and without nixfiedOwner;
- lock filenames and public errors omit declared secrets, environment values,
  and arbitrary child command lines;
- guarded borrow CAS across ProbeReady/Standing/Borrowed and guarded repair
  claim races;
- unexpired finite owner and borrower leases each return LEASE_CONFLICT and
  prove that no signal or repair-state mutation occurred;
- expired finite lease fencing stales every open lease for the same run/owner
  token, a multi-service paused heartbeat updates none and receives LEASE_STALE,
  and clean all-terminal runs still stop heartbeat successfully;
- completed/task-succeeded persistent owners with the existing year-9999 lease
  can be fenced only for the broken Standing service when no borrower remains;
- runtime death immediately after the repair CAS, followed by exact-reuse
  recovery or terminate-before-replace on the next mutating request;
- ordinary pre-lock reconciliation preserves a live Starting row for the
  endpoint-locked recovery path;
- crash-before-signal Starting recovery is distinct from failed-termination
  Escaped recovery without reading event history;
- an unrelated live local service instance is never repair-terminated;
- LEASE_CONFLICT for a live reservation lease without a process;
- REGISTRY_CORRUPT for an impossible row after reconciliation;
- atomic all-endpoint/process/service/lifecycle-success transition, exact
  affected-row validation, and no partial ready state;
- unconfirmable live process/service becomes Escaped, keeps its ports, remains
  visible/actionable to ps/down/cleanup, and releases ports only after group
  death proof;
- one consuming current-service finalizer prevents double teardown across both
  readiness and health failures;
- pre-child cancellation records Canceled evidence, ordinary failure records
  Failed evidence, and unproven termination overrides either with PROC_ESCAPE;
- ps does not mutate solely for endpoint loss;
- down remains process-owned and independent of endpoint observation.

Test fixed-root safety through private pure metadata validators and
descriptor-relative helpers rooted at test-owned FDs. Add one cfg(test)-only,
thread-scoped root-FD injection inside endpoint.rs so a service unit test can
drive the real start path against an unsafe symlink/mode/non-regular target and
prove failure precedes its prepare sentinel. The injection is absent from
production builds and is not an environment variable, CLI flag, public
function, or alternate runtime path.

Test current Linux /proc/self/ns/net statting directly. The RFC is amended by
this planning change to make distinct-namespace acceptance a deterministic key
derivation proof from two device/inode identities, rather than requiring CI to
create privileged network namespaces. Production still has exactly one source
of network identity: stat(/proc/self/ns/net).

### Focused verification during commit 1

Run focused checks during development:

~~~sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-model --test model_contract'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --lib'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test admission'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test registry'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test service'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test lifecycle'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test state'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test capability_coverage'
nix run .#check
nix run .#test
git diff --check
~~~

### M. Prove endpoint acquisition across hosts in the same commit

#### New integration floor

Add runtime/crates/nixfied-runtime/tests/endpoint.rs and share only genuinely
common fixture code through tests/common/mod.rs.

Use child processes and synchronization files/pipes so contention is real
across runtime processes and independent NIXFIED_STATE_DIR values. Do not test a
global property with two in-process Registry handles alone.

Cover the RFC acceptance matrix:

1. Ready root A blocks root B before B's prepare sentinel or child.
2. Concurrent roots have one prepare/spawn winner and one immediate
   startup-lock-contended loser.
3. After A is down, B starts on the same endpoint.
4. A persistent service outlives its starter; B acquires the now-free startup
   lock but returns listener-occupied, proving the child did not inherit it.
5. SIGKILL of a runtime blocked in prepare releases the kernel lock; the inert
   file does not block a later root.
6. An external listener fails before prepare/spawn with no owner attribution.
7. A same-registry, fully proven listener returns the complete nixfiedOwner.
8. A harness-owned external process waits for a prepare sentinel and binds
   after preflight; the losing service is reclassified PORT_CONFLICT without
   stderr parsing.
9. A proven collision never becomes PROC_ESCAPE.
10. An unrelated early child exit keeps its existing containment/lifecycle
    classification.
11. A service that never opens its endpoint reaches READINESS_TIMEOUT.
12. A multi-endpoint service with one missing endpoint never commits partial
    ready state, terminates cleanly, and releases all locks.
13. Non-conflicting endpoints and different slots progress independently.
14. A live local service that loses its exact listener remains ps.live=true;
    the next mutating request repairs it only after termination proof.
15. Failed termination returns PROC_ESCAPE, writes process/service Escaped,
    preserves open ports, and remains visible/actionable to ps and down.
16. A private decision test feeds an incomplete ownership observation into the
    repair path and proves PORT_UNVERIFIABLE preserves process/rows and blocks
    replacement; a separate real-process test proves down remains independent
    of endpoint observation. Add no production observer override.
17. Cancellation after locking, during blocking prepare, immediately after
    process recording, and during readiness releases all locks and cleans any
    recorded child before a later start.
18. Separate prepare-failure and spawn-failure cases release all locks and let a
    subsequent corrected start acquire the same endpoints.
19. Synthetic bind failure with a complete empty LISTEN snapshot returns
    PORT_UNVERIFIABLE.
20. Reservation-before-process crash yields LEASE_CONFLICT; after lease expiry
    and reconciliation, the later start succeeds.
21. Runtime death immediately after a repair claim leaves a live Starting row;
    the next mutating request re-observes and deterministically recovers or
    terminates/replaces it.
22. Exact error details are snapshot-tested after generic run-context
    enrichment.
23. Unexpired finite owner and borrower leases each block repair with
    LEASE_CONFLICT and the tracked group receives no signal.
24. A different local service instance on the endpoint is never terminated by
    repair; listener, lease-only, dead, and live-no-listener cases follow the
    closed classification table.
25. After an unconfirmable termination, ps and down find the Escaped process
    through its open port. Once group death is proven, ports release while
    terminal Escaped evidence remains.
26. An expired finite lease stales every open sibling for its run/owner token;
    a paused multi-service heartbeat refreshes none and returns LEASE_STALE.
27. A broken persistent-until-down service whose owner run is Completed or
    TaskSucceeded repairs after fencing only its year-9999 owner row, while an
    active borrower still blocks all signaling.
28. Crash-before-signal Starting recovery may restore exact reuse; an Escaped
    failed-termination row never does, even if its exact listener reappears.
29. Cancellation before a child exists records Canceled lease/run evidence;
    prepare/spawn failure records Failed evidence; unproven termination returns
    PROC_ESCAPE and preserves reservations.

Make spawn failure deterministic in the non-store test fixture: admit an
executable, then have its prepare task remove that fixture executable before
spawn. This exercises the real spawn error without a production hook. The
follow-up attempt uses a restored executable and succeeds.

Use deterministic external synchronization instead of production test hooks.
For the post-preflight bind race, the prepare task writes a ready sentinel and
blocks waiting for an acknowledgement file. The harness process waits for the
ready sentinel, binds and listens, writes the acknowledgement, and keeps the
socket open. Only then may prepare exit and the service spawn. This two-way
barrier proves the external listener wins before spawn.

#### Framework gate

Add one adopter-shaped cross-root scenario to nix/gate-runtime/nixfied.nix,
using one explicit endpoint-coordination model compiled in flake.nix:

- import examples/minimal/nixfied.nix;
- add a bounded Bash leaf whose declared sentinel path is
  ${stateDir}/endpoint-prepare-sentinel;
- assign that leaf to synthetic.lifecycle.prepare.task;
- add/export the persistent keep-up task already used by the service-lifetime
  proof;
- pass this model to the gate through one ordinary declared environment value.

1. start persistent service in inner state root A;
2. assert A's prepare sentinel exists;
3. run the same selected endpoint from inner root B;
4. assert PORT_CONFLICT/listener-occupied and assert B's prepare sentinel is
   absent;
5. down A;
6. start B, assert B's sentinel exists, and down B successfully.

Keep detailed race/error matrices in Rust tests. The gate proves the packaged
runtime and model boundary, not private registry internals.

#### macOS CI

Add a focused macOS job to .github/workflows/checks.yml:

- checkout;
- install Nix using the existing installer action;
- use the existing Nix cache action;
- run the pinned dev shell;
- run nixfied-runtime library tests and the endpoint integration test.

The job must compile and execute the macOS sysctl/proc implementation. Do not
mark platform tests ignored, allow failure, or replace them with compilation
only.

### Final upstream verification before commit 1

~~~sh
nix develop --command bash -c 'cd runtime && cargo fmt --all -- --check'
nix develop --command bash -c 'cd runtime && cargo clippy --workspace --all-targets -- -D warnings'
nix develop --command bash -c 'cd runtime && cargo test -p nixfied-runtime --test endpoint'
nix develop --command bash -c 'cd runtime && cargo test --workspace'
nix run .#check
nix run .#test
nix run .#gate
nix run .#ci
nix build .#nixfied-runtime
git diff --check
~~~

No proof is deferred to another upstream commit. Fix any failure in this same
atomic slice and rerun the focused and full floors before committing.

## Commit 2 in ../mfm3: adopt project-owned verification target

Commit message:

~~~text
adopt project-owned verification target
~~~

This is a separate repository commit after the new Nixfied revision is
available. Start by inspecting ../mfm3 status and preserve unrelated work.

### Required code changes

In ../mfm3/flake.lock:

- update the Nixfied pin to the new exact ABI revision;

In ../mfm3/nixfied.nix:

- delete verificationCachePolicy;
- delete cargoTargetCache;
- delete cacheEnv.CARGO_TARGET_DIR;
- define the existing shared cargoEnv with the one ordinary declaration
  CARGO_TARGET_DIR = "target/verification";
- keep TMPDIR = "${stateDir}" because temporary files, unlike compiler
  artifacts, remain disposable slot state;
- keep the existing unset CARGO_TARGET_DIR behavior in nix develop and .#quick
  so direct development continues to use the normal target directory;
- in postgres-sqlx-check, make CARGO_TARGET_DIR absolute from the repository
  root before changing into crates/storages/stream-store-postgres, preventing a
  second crate-local verification target;
- delete all cache-specific helper plumbing rather than renaming it.

The exact shell ordering for the crate-local check is:

~~~sh
export CARGO_TARGET_DIR="$(pwd -P)/$CARGO_TARGET_DIR"
cd crates/storages/stream-store-postgres
~~~

The target is worktree/project-owned. One worktree has one verification target
shared across Nixfied slots; different worktrees isolate naturally by path.
Nixfied does not create it, identify it, lock it, report it, retain it, or
selectively clean it. Cargo/MFM own writer locking, fingerprints, inspection,
retention, cleanup, bypass, and corruption recovery. nixfied clean does not
touch it; ordinary project cleanup is cargo clean --target-dir
target/verification or worktree deletion.

Leave MFM's .gitignore unchanged because its existing /target rule already
covers target/verification. Hosted CI remains cold/ephemeral unless MFM later
chooses an independent CI-provider cache; do not encode such a policy in this
change.

Do not retain the old slot-cache directory, add an import fallback, or teach MFM
to discover both layouts. Update the pin and authored model together; mixed
contracts are unsupported.

### Required documentation changes

Update:

- docs/adr/0001-mfm-rust-build-architecture.md;
- RFC_SLOW_BUILDS.md;
- docs/slow-build-baseline.md;
- README.md;
- AGENTS.md;
- .config/nextest.toml;
- docs/UPGRADE.md.

Delete docs/nixfied-capability-gaps.md. The accepted upstream RFC retains the
useful forcing-case history; the MFM file is a draft request for responsibilities
that the final decision rejects and must not remain as a live capability plan.

The documents must:

- close the Nixfied cache capability request as a responsibility correction;
- state that broad gates use worktree-owned target/verification while direct
  development uses the normal target directory;
- remove requested Nixfied cache leases, inspection, retention, bypass, and
  cache-only cleanup;
- retain the port defect as fixed by the upstream endpoint contract;
- state that NIXFIED_STATE_DIR neither selects nor cleans Cargo artifacts;
- state that runtime output carries execution evidence, not cache evidence;
- preserve the measured observations in docs/slow-build-baseline.md while
  replacing its final architecture/rollout conclusion;
- mark RFC_SLOW_BUILDS.md as experimental history and remove its actionable
  Nixfied-cache rollout requirements;
- in docs/UPGRADE.md, delete obsolete cache/layout migration instructions and
  document only the coordinated pin cut plus optional cleanup performed with
  the old exact runtime before repinning. Do not describe a new-runtime data or
  layout migration.

### Downstream verification

Before changing flake.lock, run from ../mfm3 against the local upstream tree:

~~~sh
nix run --override-input nixfied path:../nixfied .#check
nix run --override-input nixfied path:../nixfied .#test
nix run --override-input nixfied path:../nixfied .#test-db
nix run --override-input nixfied path:../nixfied .#ci
~~~

Then update/commit flake.lock and rerun from the committed pin:

~~~sh
nix run .#check
nix run .#test
nix run .#test-db
nix run .#ci
git diff --check
~~~

If MFM performance depends on a different target placement, remeasure it as MFM
evidence. Do not add a Nixfied compatibility path to preserve the old number.

## Deletion audit

Run this before creating upstream commit 1 and again at final handoff:

~~~sh
test ! -e runtime/crates/nixfied-runtime/src/state/cache.rs
test ! -e runtime/crates/nixfied-runtime/src/service/ownership.rs

rg -n 'CacheEnvSpec|CacheKeySpec|CacheMode|CacheScope|ResolvedCacheEnv|TaskCacheEvidence|CacheIdentity|MaterializedCacheEnv|materialize_cache_env|nixfied-cache-env-v1|cache-run-scope|NIXFIED_RUN_CACHE|NIXFIED_SLOT_CACHE|NIXFIED_EXAMPLE_CACHE' \
  --glob '!RFC_CACHE_PORT_PROB.md' \
  --glob '!IMPL_PLAN_RFC_CACHE_PORT_PROB.md' .

rg -n 'cacheEnv|cache_env' \
  --glob '!RFC_CACHE_PORT_PROB.md' \
  --glob '!IMPL_PLAN_RFC_CACHE_PORT_PROB.md' .

rg -n 'PortStatus::(Binding|Bound)|status PortStatus:.*(binding|bound)' runtime/crates/nixfied-runtime/src/registry/status.rs runtime/crates/nixfied-model/capability.txt

rg -n 'PortConflict' runtime/crates/nixfied-runtime/src
~~~

Expected results:

- the first cache-symbol search is empty except explicit stale-token assertions
  in capability coverage, if those assertions use literal names;
- cacheEnv/cache_env appears only in the intentional unknown-field model test,
  the one Nix unknown-option negative, and explicit stale-token assertions;
- PortStatus Binding/Bound is absent;
- every PortConflict result is manually classified, and the sole production
  RuntimeError constructor exists in service/process.rs;
- references to an ordinary service id named cache and to the GitHub Nix store
  cache remain untouched.

Inspect every result. Do not weaken the audit with a broad exclusion.

In ../mfm3, separately confirm that verificationCachePolicy, cargoTargetCache,
and cacheEnv.CARGO_TARGET_DIR are absent from executable configuration.
Historical decision records may retain the old names only where they describe
measured history.

## Review checklist

### Scope and concepts

- New public model fields: zero.
- New public Rust types: zero.
- New commands/surfaces: zero.
- New registry tables/columns: zero.
- New dependencies: zero.
- New user configuration: zero.
- Listener observers: one.
- PORT_CONFLICT runtime-error constructors: one.
- Readiness retry loops per service start: one.
- Ready-state transactions: one.
- Durable host-global authorities: zero.

### Correctness

- Lock key reflects only the colliding OS resource.
- Same euid/network/mount scope converges on the same inode.
- Safe traversal is descriptor-anchored and no-follow.
- Lock FDs are CLOEXEC, never written, never unlinked, and always released.
- Preflight occurs before service prepare/reservation/spawn.
- Repair termination occurs while all endpoint locks are held.
- Reuse proves current exact listeners, process identity, and containment.
- Unexpired finite owner/borrower leases are not terminated; expired finite
  run tokens are fenced as a unit, and only a successfully completed durable
  persistent owner uses the target-row exception.
- A fenced run heartbeat updates no siblings and returns LEASE_STALE.
- Starting and unresolved Escaped recovery follow distinct existing states.
- Every distinct conflicting kernel socket record is considered.
- Wildcards conflict but do not satisfy exact readiness.
- Socket ownership replaces the lock after atomic ready commit.
- ps remains process liveness; down remains usable.
- Every PORT_CONFLICT has the exact details shape.
- Unknown/unproven owners are omitted, never guessed.
- Static capacity never becomes PORT_CONFLICT.
- Old cache models fail closed and old code/layout knowledge is gone.

### Simplicity

- Cache deletion should be a substantial negative diff.
- Port code may add unavoidable OS parsing, but it must replace the existing
  ownership scanner and fragmented readiness/error paths rather than sit beside
  them.
- Delete duplicated SQL, blind borrow updates, per-endpoint ready commits,
  unused port statuses, and cache-only identity plumbing.
- Do not introduce traits or generic abstractions for the two fixed supported
  platforms unless a concrete existing caller requires them.
- Do not create helper types merely to mirror JSON. Serialize private borrowed
  structs at the sole error boundary.

## Final definition of done

The handoff is complete only when:

1. The atomic upstream contract commit and coordinated downstream adoption
   commit exist in the stated order and each repository is green.
2. The capability descriptor changed once and Nix/Rust agree on one new ABI.
3. Every RFC cache-removal and port-coordination acceptance criterion is mapped
   to a passing test or gate.
4. Linux and macOS endpoint observers execute in CI.
5. The full upstream floor passes:

   ~~~sh
   nix run .#ci
   nix build .#nixfied-runtime
   ~~~

6. The deletion audit has only the intentional negative-test/history hits.
7. The upstream diff contains no fallback, compatibility shim, migration, dead
   old module, or public scope expansion.
8. The MFM adoption commit updates the pin and deletes the old integration in
   one slice.
9. MFM's .#check, .#test, .#test-db, and .#ci pass from the committed pin.
10. Any unverified platform behavior or remaining fragility is reported
    explicitly; it is not hidden with a skip or alternate code path.
