# RFC: Remove cache semantics and make port conflicts host-aware

- Status: proposed
- Date: 2026-07-18
- Scope: Nixfied model, Nix compiler, Rust runtime, generated views, and adopter
  guidance
- Origin: MFM verification-cache capability review
- Input: `../mfm3/docs/nixfied-capability-gaps.md` and the pinned MFM/Nixfied
  integration

## Summary

This RFC resolves two problems exposed by MFM without turning Nixfied into a
cache manager or a host daemon.

First, Nixfied's current `invocation.cacheEnv` is a partial ownership
abstraction. Nixfied defines cache-specific public types, derives identities,
chooses host placement, creates directories, and reports cache evidence, but it
does not own writer arbitration, inspection, retention, targeted cleanup, or
recovery. MFM reasonably interpreted the existing abstraction as the beginning
of full cache lifecycle ownership and requested those missing capabilities.
Accepting that request would expand Nixfied from a generic execution runtime
into a mutable-cache resource controller.

This RFC instead removes cache as a Nixfied concept. `cacheEnv` and all related
model, compiler, runtime, output, documentation, example, and test code are
deleted. Tool caches become ordinary child/project state expressed, when
needed, through the existing invocation environment. Nixfied neither identifies
nor manages them.

Second, the runtime coordinates service endpoint reservations only within the
SQLite registry selected by `NIXFIED_STATE_DIR`, while TCP endpoints are
host-global. Two runtimes with independent state roots can therefore choose the
same endpoint and discover the collision only after a service child fails. This
is an existing runtime responsibility defect because Nixfied already owns
service placement, startup, endpoint ownership verification, and the stable
`PORT_CONFLICT` error class.

This RFC adds a narrow, ephemeral, endpoint-keyed OS startup lock independent of
the selected state root for cooperating same-effective-user runtimes in the same
network/mount namespaces. The runtime holds the lock from before
availability checking and lifecycle mutation until the service has registered,
proven socket ownership, and passed readiness. It then releases the lock; the
listening socket is the host ownership authority for the remainder of the
service lifetime. No daemon, global durable registry, dynamic port fallback,
socket activation, or cache subsystem is added.

The change is intentionally breaking. There is no compatibility layer,
migration, fallback, deprecated alias, or adoption of old cache state.

## Normative language

The terms **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are
normative in this document.

## Context

### The framework boundary

Nixfied has one semantic seam and a deliberately split implementation:

- Nix evaluates typed adopter declarations, validates intent, derives graph
  facts, realises closures, and emits `model.json` plus disposable views.
- Rust admits the model against host reality and owns impure execution:
  processes, services, endpoints, registry reconciliation, evidence, and safe
  state cleanup.
- Child programs own their domain semantics. The runtime executes generic
  primitives and does not understand Cargo, Postgres, Python, or any other
  concrete tool.

The framework should add a concept only when correctness cannot be expressed
through the existing task/service algebra and invocation primitive. Performance
optimisations inside a child tool are not automatically framework resources.

### The MFM forcing case

MFM uses one persistent Cargo target across its broad local verification tasks.
The target materially improves unchanged verification time and is therefore a
real consumer need, not a hypothetical optimisation. It is also large,
path-sensitive, mutable, and operationally expensive:

- a stable target is approximately 10.49 GiB;
- warm verification improved by roughly 45 percent in the qualifying sample;
- an identical revision executed from another worktree accumulated hundreds of
  MiB of path-specific artifacts;
- changed profiles and targets grew one identity to approximately 16.92 GiB;
- concurrent runs can enter the same target;
- supported Nixfied cleanup removes the target together with slot service state
  and run evidence; and
- independent state roots using the same slot can choose the same Postgres
  endpoint and report `PROC_ESCAPE` rather than an actionable port conflict.

MFM's capability handoff requested worktree cache identity, exclusive writer
leases, inspection, bypass, byte/age retention, LRU eviction, cache-only cleanup,
lifecycle evidence, and host-global port diagnostics.

The cache evidence does not demonstrate corrupted Cargo artifacts: the recorded
cross-worktree and concurrent verification runs completed successfully. It
demonstrates reuse value, path-specific growth, shared-writer exposure, and the
absence of an owned operational lifecycle. The proposed quotas, retention ages,
and LRU behavior are downstream policy choices, not pre-existing Nixfied
semantics. This distinction matters: the framework defect is ambiguous partial
ownership, not a failed obligation to implement MFM's chosen cache policy.

Those requests contain two different architectural questions. Cache identity,
leases, inspection, retention, bypass, and cleanup are one proposed cache
lifecycle subsystem. Host-global endpoint collision handling completes an
existing service-placement responsibility. They MUST NOT be implemented as one
combined resource-management expansion merely because one adopter reported both.

### Current `cacheEnv` behavior

The current public cache contract includes:

- `Invocation.cacheEnv`;
- `CacheEnvSpec` and `CacheKeySpec`;
- `CacheMode` (`fast-dev`, `trusted-ci`, and `exact`);
- `CacheScope` (`run` and `slot`);
- Nix-side scope defaults and validation;
- a Rust digest over cache fields plus target, runtime ABI, and toolchain;
- runtime-owned directory materialisation; and
- cache-specific task and run evidence.

For a slot-scoped cache, the runtime creates:

```text
<state-base>/<project>/<environment>/<slot>/caches/<family>/<digest>
```

The cache digest does not contain the admitted live source root. Physical
placement supplies project, environment, slot, and selected state-base
isolation. Two invocations from different worktrees can therefore share a path
when they use the same state base and slot.

The runtime does not create a cache ownership marker, cache registry record,
cache/process relationship, cache lease, creation/last-use record, size
accounting, or retention state. It creates the directory, injects the path into
the child environment, and records the resolved binding. Child processes own the
contents.

This is neither a complete runtime-owned resource nor ordinary adopter-owned
state. That ambiguity is the cache problem.

### Current endpoint coordination

The runtime deterministically assigns declared service endpoints from the
selected slot's candidate window. Before prepare and spawn it transactionally
reserves each endpoint in the selected per-slot SQLite registry. It later proves
that the expected process owns every declared endpoint before reporting the
service ready.

This works when contenders share a registry. It fails across independent state
bases because each base has its own registry tree. The OS endpoint is shared, but
the reservation evidence is not. A losing service may start, fail to bind, and
surface as `PROC_ESCAPE` or a lifecycle failure even though the real cause is a
port collision.

## Problem statement

### Problem 1: cache placement implies responsibility Nixfied does not want

`cacheEnv` places a domain-specific performance resource behind framework-owned
types and paths. Once the runtime appears to own that resource, sound operation
requires answers to all of the following:

- What is the complete namespace and compatibility identity?
- Which process may write it?
- How is ownership reconciled after cancellation or runtime death?
- How are identities inspected without reading private runtime layout?
- How are creation, last use, size, and active ownership recorded?
- Which policy chooses an inactive identity for eviction?
- How is deletion marker-gated, lease-gated, idempotent, and recoverable?
- How does a cold/bypass execution avoid mutating the durable identity?

These are not isolated missing flags. Together they define a local mutable-cache
manager with a durable state machine, operator surface, accounting rules, and
garbage collector.

Nixfied has no need to own those semantics to execute the task correctly. A task
must execute whether a child tool is cold or warm. Cache contents do not prove a
result, are not runtime evidence, and remain opaque child-written files.

Keeping the partial abstraction would preserve the same ownership ambiguity and
invite incremental expansion. Completing it would add concepts and code paths
outside the framework's core purpose.

### Problem 2: a state-local registry cannot exclusively coordinate a host endpoint

A TCP endpoint is identified by OS networking semantics, not by a Nixfied state
base. Two runtimes can use different state roots, projects, environments, or
registries and still contend for the same address and port.

The runtime already makes an addressability claim for declared endpoints and
already refuses unverified ownership. Failing to classify a known collision as
`PORT_CONFLICT` is therefore not a new product requirement. It is incomplete
execution correctness inside the existing service primitive.

A durable host-global registry would solve more than this problem and would
introduce another authority, cleanup domain, corruption mode, and liveness
reconciliation path. The required coordination is narrower: serialize Nixfied
service startup for the same OS endpoint while the service transitions from
planned placement to actual socket ownership.

## Goals

This RFC MUST:

1. remove cache as a framework/model/runtime concept;
2. preserve ordinary invocation environment support for adopter-owned tool state;
3. leave cache compatibility, concurrency, inspection, retention, cleanup, and
   recovery with the child tool/project;
4. detect a pre-existing endpoint conflict before service prepare or spawn;
5. serialize cooperating same-effective-user Nixfied startups in the same
   network/mount namespaces for the same normalized OS endpoint across
   independent state roots;
6. retain the listening socket, not a Nixfied file lock, as steady-state endpoint
   ownership;
7. emit `PORT_CONFLICT` whenever OS evidence proves that another process owns the
   planned endpoint;
8. report owner identity only when Nixfied can prove it;
9. preserve the single-model seam, the Nix/Rust boundary, generic runtime, process
   containment, redaction, and marker-confined cleanup invariants; and
10. delete obsolete code instead of wrapping or deprecating it.

## Non-goals

This RFC MUST NOT add:

- cache result memoization;
- cache identity, family, mode, scope, policy, inspection, history, or lifecycle
  APIs;
- cache leases, registries, markers, byte accounting, quotas, LRU, age retention,
  pruning, eviction, or cache-only cleanup;
- Cargo-specific runtime behavior;
- a new semantic kind;
- a new runtime control command for caches;
- a global durable port registry;
- a daemon;
- cross-user startup serialization or trusted Nixfied identity attribution;
- coordination across distinct network or mount namespaces;
- arbitrary process command-line scanning;
- dynamic port selection or fallback to another candidate;
- socket activation or a service file-descriptor handoff protocol;
- a persistent service supervisor solely to hold an endpoint lock;
- compatibility with old models, registries, cache paths, or output schemas; or
- migration of old cache contents.

## Decision 1: delete cache as a Nixfied concept

### Revised responsibility boundary

After this RFC, the ownership boundary is:

| Concern | Owner |
| --- | --- |
| Toolchain closure and immutable package | Nix |
| Task selection and static graph | Nix-authored model |
| Child process execution and cancellation | Rust runtime |
| Service lifecycle, endpoint ownership, registry, and evidence | Rust runtime |
| Tool/compiler cache path convention | Adopter/project |
| Tool/compiler cache contents and compatibility | Child tool/adopter |
| Tool/compiler cache concurrency | Child tool/adopter |
| Tool/compiler cache inspection, retention, and cleanup | Child tool/adopter/OS |
| Task success | Actual child execution under the task exit policy |

The runtime MUST NOT infer task success, skip a task, or synthesize evidence from
the presence of adopter-owned state.

### Invocation contract

`Invocation` will contain:

- `tools`;
- `run`;
- `executable`;
- `env`;
- `codebaseId`;
- `cwd`;
- `stdin`; and
- `timeoutMs`.

It will not contain `cacheEnv` or any equivalent cache/resource sub-language.

An adopter MAY declare a tool-specific environment variable such as
`CARGO_TARGET_DIR` through ordinary `invocation.env`. This is not interpreted by
Nixfied. It is just a declared child environment value.

An adopter has two honest placement choices:

1. A project/worktree-owned relative path. The project owns isolation,
   concurrency, inspection, retention, and cleanup. Nixfied `clean` MUST NOT
   delete it merely because a task used it.
2. A path explicitly derived from `${stateDir}`. The files are ordinary
   child-written slot state. They are subject to the existing broad slot state
   lifecycle and `clean` semantics. Nixfied still does not interpret them as a
   cache or provide cache-specific operations.

Choosing `${stateDir}` does not create a cache lifecycle contract. Choosing a
worktree path does not make Nixfied responsible for source-tree cleanup.

### Output contract

Task, node, run, summary, and error output MUST NOT contain cache-specific
identity or evidence fields. In particular, the following output schema is
deleted:

```text
task-cache-evidence: envVar family mode scope digest path
```

The optional `cacheEnv` fields in task and node results are also deleted.

Runtime output continues to report execution evidence: task/step identity,
process identity, exit status, cancellation/timeout state, duration, logs, and
summaries.

### Required deletion

The implementation MUST delete, not retain behind unused compatibility paths:

- the Nix `cacheKeyType`, `cacheEnvType`, and invocation `cacheEnv` option;
- cache scope defaults, cache validation, cache normalization, and cache field
  emission in the compiler;
- `CacheEnvSpec`, `CacheKeySpec`, `CacheMode`, and `CacheScope` from
  `nixfied-model`;
- cache validation helpers and cache-specific validation errors;
- resolved/materialized cache execution types and empty cache initializers in
  non-cache execution plans;
- runtime cache digesting and directory materialisation;
- cache environment injection distinct from ordinary `env`;
- cache evidence from task commands, summaries, nodes, and run JSON;
- cache-only target/runtime/toolchain identity fields carried through
  `RunContext` and `StartedService`, together with their constructors;
- cache exports from runtime state modules;
- the cache-only external visibility of state directory materialisation helpers;
- cache schema/capability entries, generated schema/view entries, and the Rust
  CLI's cache primitive projection;
- cache-specific examples, gates, fixtures, tests, negative tests, and gate-only
  `flake.nix` model injection; and
- documentation and contributor-guide language that presents cache as a
  Nixfied primitive.

The implementation SHOULD delete `runtime/crates/nixfied-runtime/src/state/cache.rs`
entirely unless an unrelated, non-cache responsibility demonstrably remains in
that file.

### New negative boundary

The existing `CACHE-1` invariant is replaced with:

> **CACHE-1:** Cache and accelerator lifecycle is child/tool/project-owned. The
> model and runtime define no cache primitive, identity, placement, lease,
> inspection, bypass, retention, accounting, or selective cleanup. Tasks always
> execute; ordinary declared environment and arguments may configure
> adopter-owned tool state.

This negative boundary prevents reintroducing cache management piecemeal under a
different name.

## Decision 2: serialize startup and let the socket own the endpoint

### Ownership phases

A declared endpoint has three distinct ownership phases:

1. **Planned:** the runtime has selected the model-declared candidate endpoint,
   but no child has proven ownership.
2. **Starting:** one service-start attempt holds the endpoint's OS startup lock
   while it checks availability, performs prepare, spawns, records the process,
   verifies endpoint ownership, and waits for readiness.
3. **Ready:** the expected service process owns the listening socket. The startup
   lock has been released. The socket and OS process identity are the liveness
   truth.

The startup lock MUST NOT be treated as steady-state service liveness evidence.

### Normalized lock identity

The lock key MUST represent the OS resource that can collide, not a
service-specific Nixfied identifier. For the current TCP loopback primitive it
contains exactly the collision identity needed by the OS:

```text
transport = tcp
network   = OS network-scope identity
family    = canonical IP address family
address   = canonical loopback address bytes
port      = selected numeric port
```

On Linux, `network` is the device/inode identity obtained by `stat` on
`/proc/self/ns/net`; inability to obtain it is `PORT_UNVERIFIABLE`. On macOS,
which has no supported per-process network namespace in this contract, it is the
constant `host`. This prevents runtimes that share `/tmp` but use distinct Linux
network namespaces from falsely contending on endpoints that cannot collide.

Different projects, services, endpoint ids, state roots, and registries that
select the same normalized OS endpoint in the same network scope MUST contend on
the same startup lock. Runtime ABI, model hash, and adopter identities MUST NOT
enter the key.

The filesystem lock name SHOULD be a fixed-format digest of the normalized
endpoint. Untrusted project/service identifiers MUST NOT become path components.

IPv4/IPv6 loopback and platform dual-stack behavior MUST ultimately be decided
by the OS availability/ownership check. The lock key serializes
Nixfied-declared exact endpoints; it does not replace socket semantics or widen
the loopback-only model contract.

### Coordination root

The lock lives in one deterministic, per-effective-user, runtime-owned ephemeral
coordination root that is independent of `NIXFIED_STATE_DIR` and the project
state base. The exact roots are:

```text
Linux: /tmp/nixfied-<decimal-effective-uid>/endpoint-locks
macOS: /private/tmp/nixfied-<decimal-effective-uid>/endpoint-locks
```

They are derived from the numeric effective UID returned by the OS, never an
environment variable. Runtimes for the same effective user in the same network
and mount namespaces therefore resolve the same root. Callers cannot select or
override it. Other platforms return `PORT_UNVERIFIABLE` before service
mutation unless this RFC is amended with one exact equivalent.

The coordination root:

- MUST be selected by Rust, not carried in `model.json`;
- MUST be reached through the fixed, real, root-owned, sticky system temporary
  directory shown above; an unsafe base produces `PORT_UNVERIFIABLE`;
- MUST have every runtime-created directory component owned by the effective
  user and mode `0700`;
- MUST reject unsafe ownership, permissions, symlink components, and non-regular
  lock targets;
- MUST create/open regular, effective-user-owned lock files at mode `0600`
  without following symlinks and with close-on-exec semantics;
- MUST NOT contain semantic model data, secrets, registry history, or authoritative
  liveness records; and
- MUST NOT require cleanup for correctness because the kernel lock, not file
  existence or file contents, is the ownership truth.

Creation and traversal below the fixed system directory MUST be anchored to
validated directory descriptors and use no-follow/directory-only operations, so
validation is not separated from use by a path-replacement race.

An existing unlocked file is inert. A missing file is created safely. Stale file
contents MUST NOT block startup. Nixfied `clean` MUST NOT traverse this root,
and runtime code MUST NOT unlink or replace a lock file because removing a live
locked inode would split coordination between old and newly created files.

Each lock target is opened with the platform equivalents of
`O_CREAT | O_RDWR | O_NOFOLLOW | O_CLOEXEC`, then verified with `fstat` before
locking. The runtime takes a whole-file, exclusive, nonblocking `flock` whose
ownership follows the open file description and is released when the last
runtime-held descriptor closes or the runtime dies. No polling or timeout is
used. The lock filename is the lowercase full SHA-256 hex digest of the
normalized endpoint key plus `.lock`; it carries no adopter-controlled text.

### Endpoint availability and ownership observation

In this RFC, an endpoint is **occupied** only when OS inspection observes an
active TCP `LISTEN` socket whose local binding conflicts with the planned exact
loopback address and port. The platform observer MUST account for exact,
wildcard, and dual-stack listeners according to that host's bind semantics. A
`TIME_WAIT` entry, stale registry row, lock-file contents, or failed temporary
bind by itself is not proof of an occupying process.

The observer MUST enumerate every conflicting listener, not stop after the first
process match. Readiness and reuse succeed only when at least one listener exists
for each declared endpoint and every observed conflicting listener is proven to
belong to the tracked contained process group/tree. A co-bound external listener
therefore prevents ownership proof even if one expected child socket also
matches.

While holding the startup locks, preflight for each endpoint is:

1. Create a temporary socket of the planned address family without
   `SO_REUSEADDR` or `SO_REUSEPORT` and attempt to bind the exact planned
   address/port. Never call `listen` or hand this socket to a child.
2. If bind succeeds, close the socket; the endpoint was available at that
   instant.
3. If bind returns address-in-use, immediately inspect the OS listener table.
   If an active conflicting `LISTEN` socket is observed, the endpoint is
   occupied and the result is `PORT_CONFLICT`. Resolving that listener to a
   process is best-effort and affects attribution, not the conflict itself.
4. If bind fails but no active conflicting listener can be observed, return
   `PORT_UNVERIFIABLE`; residual kernel state or an unobservable race MUST NOT be
   described as another process owner.
5. If socket creation, bind, or listener inspection cannot be performed safely,
   return `PORT_UNVERIFIABLE`.

The successful temporary bind is only a point-in-time preflight. Closing it
creates the explicitly accepted external-race window; startup locks exclude
cooperating Nixfied contenders, not arbitrary binders.

### Startup algorithm

An endpoint-less service has no addressability claim and takes no endpoint lock.
The runtime first reconciles its selected local registry. This RFC strengthens
that path: before reusing or replacing an endpoint-bearing service, the runtime
MUST re-observe every declared listener and prove ownership against the recorded
PID/process group/start identity. Matching registry endpoint rows and a live
process alone are insufficient. A service proven live, endpoint-owning, and
otherwise reusable under `SVC-ID-1` is not a new start and does not take a
startup lock. A missing or proven-wrong listener prevents reuse and is never
silently accepted. On this mutating reuse/replacement path, if its recorded
process group is still live, the runtime MUST terminate that owned group through
the existing TERM/KILL path, confirm the group is gone, and only then mark the
existing service/process/port records terminal or stale and permit replacement.
If termination cannot be proven, the existing reservations remain closed and
the operation fails with `PROC_ESCAPE`; the runtime MUST NOT start a replacement
that the old process could race by rebinding. If listener inspection itself is
unverifiable, the runtime instead preserves the live process and reservations,
returns `PORT_UNVERIFIABLE`, and permits explicit `down` to stop the proven-owned
process; it neither reuses nor replaces it. For every new endpoint-bearing
service start, the runtime MUST perform the following sequence:

1. Derive all normalized endpoint lock keys for the service.
2. Sort the keys in one deterministic byte order.
3. Acquire every startup lock exclusively and non-blockingly in that order
   before any service prepare task, state-mutating lifecycle operation, local
   endpoint reservation, or service spawn.
4. If any lock is contended, release locks already acquired for this attempt and
   fail immediately with `PORT_CONFLICT`; contention alone does not prove the
   holder's identity, and no wait or retry policy is introduced.
5. Check OS availability for every planned endpoint.
6. If any endpoint is already occupied, release all startup locks and fail with
   `PORT_CONFLICT` before prepare or spawn.
7. Reserve the service and endpoints in the selected per-slot SQLite registry as
   today.
8. Run prepare, spawn the child, record its process identity, and perform
   readiness plus ownership verification while retaining all startup locks.
9. A service is ready only after the expected process owns every declared
    endpoint and the declared readiness probe succeeds.
10. Mark the existing registry endpoint/process state ready/active.
11. Release all startup locks.

Every failure path MUST release acquired locks. Lock lifetime SHOULD be expressed
through scoped/RAII ownership so ordinary error propagation cannot omit release.
The implementation MUST retain the guards in the in-progress service state
across the existing spawn/readiness API split and clear them only after step 10.
Lock file descriptors MUST be close-on-exec and MUST NOT be inherited by
prepare, service, probe, task, or stop children.

The existing fail-only port policy remains unchanged. The runtime MUST NOT pick a
different port after a conflict.

### Persistent services

A `persistent-until-down` service may outlive the runtime invocation that started
it. It MUST NOT inherit or depend on the startup file lock after readiness.

Once ready:

- the service's listening socket is the host-global endpoint owner;
- the registered PID/process-start identity is Nixfied's durable evidence;
- a later runtime acquires the now-free startup lock, observes the occupied
  endpoint, and returns `PORT_CONFLICT`; and
- stopping the service releases the endpoint by closing the socket, after which
  another runtime may start it normally.

No surviving Nixfied supervisor or lock-holder process is introduced.
If a ready service voluntarily closes a declared listener, it has relinquished
that endpoint; registry evidence cannot preserve socket ownership that the OS no
longer observes. `ps` remains process-liveness reconciliation and MUST NOT signal
the process or redefine `ps.live` as addressability. A later local
reuse/replacement request applies the terminate-before-replace rule above. A
cross-state-root runtime cannot discover an unbound old process, so a service
that closes and later rebinds participates in the same explicitly unsupported
external bind race. Eliminating that gap would require the rejected lifetime
lock, guardian, or socket-activation design.

### Owner diagnostics

For the new exact ABI, every `PORT_CONFLICT` error's existing `details` object
MUST contain this single `portConflict` shape:

```text
PortConflict = {
  reason: "startup-lock-contended" | "listener-occupied",
  projectId: string,
  endpoint: {
    transport: "tcp",
    family: "ipv4" | "ipv6",
    address: string,                 # canonical loopback IP literal
    port: u16,
    endpointId: string
  },
  nixfiedOwner?: {
    projectId: string,
    environment: string,
    slot: u32,
    runId: string,
    serviceId: string,
    serviceInstanceId: string,
    processKey: string
  }
}
```

The existing generic run-error details (`runId`, `environment`, `slot`, and
`failedService`) remain the requester context and MUST NOT be duplicated inside
`portConflict`.

Absence of `nixfiedOwner` has one exact meaning: the owner is unknown or not
provably Nixfied. The runtime MUST include `nixfiedOwner` only when an exact
trustworthy registry match identifies the service, the registered primary
process has a non-null live start-identity match, and every observed listener
process is proven to be that primary or a currently contained member of its
recorded process group/tree with a non-null live start identity. A PID, process
group, stale lock-file contents, open port, or matching command-line string by
itself is not sufficient. Startup-lock contention alone therefore never carries
`nixfiedOwner`.

After a cross-state-root service has reached readiness and released its startup
lock, the socket alone cannot prove which Nixfied project owns it without the
rejected global authority, so `nixfiedOwner` is absent. The runtime does not
expose an unproven external PID/command as a substitute. This shape and its
required/optional fields MUST be recorded in the capability descriptor and
snapshot-tested.

The runtime MUST NOT write owner or diagnostic metadata into lock files. Their
contents are ignored; all attribution comes from the live registry/process/socket
proof above.

Public diagnostics MUST NOT include service environment values, credentials,
secret placeholders, or arbitrary process command lines.

### Error classification

The stable error meanings are:

- `PORT_CONFLICT`: the normalized endpoint startup lock is contended, or OS
  evidence proves that another process owns the planned endpoint;
- `PORT_UNVERIFIABLE`: the runtime cannot perform the required endpoint ownership
  proof or cannot establish safe endpoint-lock coordination on the host;
- `PROC_ESCAPE`: the expected child violated process containment or escaped the
  runtime-owned process relationship; and
- `READINESS_TIMEOUT` or `LIFECYCLE_FAILED`: startup did not become ready and no
  port conflict or stronger typed cause was proven.

Candidate-window exhaustion is static placement infeasibility, not an OS port
conflict. The Nix compiler MUST reject it during evaluation, and the runtime's
all-plans feasibility proof MUST reject a hand-authored/dev model with
`MODEL_ADMISSION` before admission completes. The post-admission planner MUST
therefore never use `PORT_CONFLICT` for insufficient candidate capacity;
`PORT_CONFLICT` is reserved for runtime endpoint acquisition and always carries
the shape above.

A durable port reservation in the selected per-slot registry that still belongs
to an open run lease but has no observable listener is local lease ownership,
not host socket ownership. The registry reservation gate MUST return the
existing `LEASE_CONFLICT`, preserve the reservation, and run no prepare/spawn.
After the lease expires, ordinary reconciliation may stale/release it and a
later attempt may proceed. Registry reservation code MUST NOT emit a third
shape-less `PORT_CONFLICT` case.

A proven address-in-use condition MUST NOT leak as `PROC_ESCAPE`.

The runtime MUST NOT parse child stderr for phrases such as “address already in
use” to assign `PORT_CONFLICT`. Classification comes from OS ownership evidence.
When child exit, probe failure, or timeout may have followed a bind race, the
runtime MUST observe endpoint ownership again before returning the weaker
failure. It may override that failure with `PORT_CONFLICT` only when process
inspection proves that the active listener is outside the tracked process group
and start identity, or when the tracked process group is proven no longer live
and an active conflicting listener remains. A failed temporary bind alone is
never enough because a live expected child may own the socket and residual
kernel state may outlive a dead one. If the live owner cannot be distinguished,
the result remains `PORT_UNVERIFIABLE`, not a guessed conflict.

### External bind races and crash boundary

The startup lock coordinates cooperating Nixfied runtimes for the same effective
user in the same network and mount namespaces. An unrelated process does not
honor it and may race between the availability check and the service child's
bind. A different user or a runtime isolated into a different network or mount
namespace is likewise outside the lock guarantee.

If OS inspection proves that another process won such a race, the runtime MUST
return `PORT_CONFLICT`. If the competing process disappears before ownership can
be proven, the runtime MUST preserve the strongest truthful lifecycle/readiness
error and MUST NOT invent a conflict owner.

The runtime lock is released automatically if the starting runtime dies. A child
that survives a runtime crash may subsequently bind. A later invocation will
observe the socket if it is bound, or may participate in an unavoidable external
bind race if it is not yet bound.

This RFC deliberately does not promise atomic reservation against arbitrary
external processes or across runtime death before child bind. Providing that
stronger guarantee would require socket activation, file-descriptor handoff, an
inherited lifetime lock, or a guardian process, all of which are outside scope.
Mixed old/new Nixfied runtimes are unsupported; a runtime from before this exact
ABI is an uncoordinated external participant for purposes of this guarantee.

### Registry relationship

The per-slot SQLite registry remains the sole durable semantic registry and event
history. The endpoint startup lock is transient kernel coordination around a
host-global OS resource. It is not a second registry, lease table, liveness
oracle, or semantic artifact.

`REG-1` is clarified accordingly:

> **REG-1:** one transactional per-slot SQLite registry owns durable runtime
> records and their total per-slot event order. Ephemeral OS locks may serialize
> acquisition of host-global kernel resources; their files and contents are not
> registry state or liveness authority.

`PORT-1` is strengthened without adding a new invariant:

> **PORT-1:** For every declared endpoint, Nixfied serializes cooperating
> same-effective-user startups in the same network/mount namespaces
> independently of state-root placement, refuses an endpoint observed occupied
> or reserved before service mutation, and admits readiness only after the
> tracked process group owns every listener. Startup locks are transient; after
> readiness the socket is ownership and the registry is evidence, never a
> host-global oracle. The existing endpoint-less-service carve-out is unchanged.

`SVC-ID-1` retains its existing meaning, with point-in-time listener proof now
required for endpoint-bearing reuse. `LIVE-1` and `ps.live` retain OS process
liveness as their subject; this RFC does not turn `ps` into an availability
probe or destructive reconciler. The hermetic child-environment contract becomes
exactly declared invocation environment plus the runtime-owned `PATH`. REDACT-1
refers generally to child-written files and sockets rather than preserving cache
vocabulary.

### Implementation boundary

Endpoint coordination is private runtime machinery around the existing endpoint
primitive. It MUST NOT add a model field, public model type, registry table,
control command, or adapter protocol. A normalized endpoint key and an RAII
guard collection MAY exist as private Rust types; they do not cross
`model.json`, generated views, or stable output.

The implementation SHOULD extend the existing service-start and endpoint-owner
paths instead of creating a parallel supervisor or reconciliation engine. It MAY
reuse low-level safe-path validation used by state placement, but the per-user
coordination root is not project state and MUST NOT enter slot cleanup policy.
The existing listener-owner path MUST separate “a conflicting listener exists”
from “that listener maps to this process”: existence is sufficient for a
preflight conflict, while readiness/reuse still requires the stronger process
identity proof.

Across the whole RFC, the implementation is expected to be a net conceptual and
code reduction: the bounded private locking path replaces neither the deleted
cache contract nor any part of the socket/registry ownership path.

## Model and ABI changes

This RFC changes the exact contract by editing the capability descriptor. The
descriptor removes cache primitives, fields, enums, and output schemas and
records the strengthened port behavior. Its digest therefore produces a new
`runtimeAbi` automatically and identically in Nix and Rust.

This RFC does not separately change the model envelope or toolchain generation:

- `modelVersion` remains `1`;
- `toolchainId` remains `nixfied-toolchain:1`; and
- the runtime ABI base remains `nixfied-runtime-abi:1`.

The capability-derived suffix exists precisely so every wire/runtime contract
change rotates the exact ABI without manually incrementing unrelated version
axes. Bumping all three identities would duplicate the same invalidation and
make future changes touch more places without distinguishing another semantic
event. The ABI snapshot and Nix/Rust producer/consumer agreement tests MUST be
updated in the same change.

The runtime MUST continue denying unknown fields. A model containing `cacheEnv`
MUST fail admission as an old/invalid model; it MUST NOT be ignored.

Old registries and state markers carry the previous exact runtime ABI and are
not adopted by the new runtime. No registry migration is added.

## Surface changes

No new runtime or ergonomic CLI command is added.

The framework-owned surface remains:

```text
model schema docs capabilities check run ps down clean
```

The existing `clean` command retains whole-slot semantics. It does not acquire a
cache selector, family selector, identity selector, retention planner, or
cache-only mode.

The run output schemas lose their optional cache evidence fields. Because the
exact runtime ABI rotates, this breaking output change is intentional.

`PORT_CONFLICT` remains the stable public error code. Its structured details are
updated for the new ABI to carry the attempted project/endpoint alongside the
existing generic run context and only one optional `nixfiedOwner` object;
absence is the closed representation of an unknown or unproven owner.

## Security requirements

The endpoint startup lock implementation MUST:

- use the one runtime-selected, deterministic per-effective-user coordination
  root for the platform;
- validate canonical placement, effective-user ownership, and restrictive
  permissions before use;
- reject symlink traversal and non-regular lock targets;
- create/open mode-`0600` lock files without following symlinks and with
  close-on-exec semantics;
- derive filenames only from a fixed normalized endpoint digest;
- acquire multi-endpoint locks in deterministic order;
- release all acquired locks on every error and cancellation path;
- never unlink or replace coordination lock files, including during `clean`;
- treat kernel lock state, not file existence or contents, as truth;
- ignore lock-file contents and never store owner metadata there;
- avoid secrets and command lines in lock paths or contents; and
- fail with `PORT_UNVERIFIABLE` before lifecycle mutation when safe locking or
  endpoint ownership inspection cannot be established.

The implementation MUST preserve the current redaction contract for runtime
errors, registry payloads, summaries, and logs. Child-written files and sockets
remain outside REDACT-1 because they are not runtime-owned output.

## Alternatives considered

### Build the cache lifecycle requested by MFM

Rejected. It would add durable cache identity, leases, inspection, accounting,
retention policy, LRU planning, bypass, deletion recovery, output schemas, error
states, and operator commands. Those are internally coherent capabilities but
are not required for task execution correctness and materially expand framework
purpose.

### Keep `cacheEnv` but document that it is placement-only

Rejected. Documentation cannot resolve the ownership ambiguity created by
framework-specific cache types, identity, paths, and evidence. The next adopter
would encounter the same missing lifecycle questions.

### Keep `cacheEnv` and add only worktree identity plus a writer lock

Rejected. Worktree identities create retained orphan paths; a writer lock creates
ownership and crash questions; safe cold execution requests bypass; safe deletion
requests targeted cleanup. This is the incremental path toward the rejected
cache manager while retaining all current public concepts.

### Add cache-only `clean`

Rejected. A safe selector requires a first-class cache identity and active-use
relationship. Adding only deletion would create another partial lifecycle
abstraction rather than remove the existing one.

### Preserve old cache fields as ignored or deprecated

Rejected. Unknown old fields must fail closed. Compatibility aliases, ignored
fields, migrations, and deprecation periods violate the project's exact-contract
and no-backward-compatibility policy.

### Use only an OS availability preflight

Rejected as insufficient between cooperating Nixfied runtimes. Two runtimes can
both observe a free endpoint before either child binds. The ephemeral startup
lock closes that race for Nixfied participants without introducing durable global
state.

### Hold the endpoint file lock for the complete service lifetime

Rejected. Persistent services intentionally outlive their starting runtime. An
inherited lock would create a new file-descriptor/process protocol and duplicate
the ownership already enforced by the listening socket.

### Add a host-global port registry or daemon

Rejected. It adds another durable authority, corruption/reconciliation domain,
cleanup surface, and process lifecycle. The kernel socket plus a transient
startup lock is sufficient for the selected guarantee.

### Add socket activation

Rejected. Socket activation would require child/service participation in a
file-descriptor handoff protocol and would widen the generic invocation and
adapter contracts. The current consumer needs fail-closed collision diagnostics,
not atomic socket delegation.

### Select another free port automatically

Rejected. Nixfied's current port policy is deterministic and fail-only. Silent or
automatic fallback changes endpoint identity and service reuse semantics and is
an explicit non-goal.

## Acceptance criteria

### Cache removal

The implementation is complete only when:

1. `cacheEnv` is absent from the Nix module API, emitted model, schema, docs, and
   capability descriptor.
2. Rust contains no cache model structs, enums, validation, lowering,
   materialisation, injection, evidence, state module, cache-only run/service
   identity plumbing, or cache-only helper visibility.
3. Run/task/node output contains no cache-specific fields.
4. A model containing `invocation.cacheEnv` is rejected as an unknown field.
5. Ordinary declared environment variables such as `CARGO_TARGET_DIR` continue
   to reach leaf children hermetically.
6. `${stateDir}` remains an ordinary placement substitution with existing broad
   slot cleanup semantics and no cache interpretation.
7. Examples and gates no longer assert cache identities or paths.
8. The capability descriptor produces one new exact ABI in Nix and Rust, the ABI
   snapshot and generated views agree, and the unchanged model/toolchain/base
   version constants remain `1`.
9. A repository-wide search finds no obsolete cache contract outside historical
   discussion in this RFC and explicitly retained downstream decision records.
10. No compatibility parser, conversion, fallback, deprecated alias, or old
    layout cleanup code remains.

### Port startup coordination

Framework acceptance tests MUST prove:

1. A ready service in state root A causes the same endpoint selected from state
   root B to fail with `PORT_CONFLICT` before B's prepare sentinel or service
   child executes.
2. Two simultaneous starts from independent roots cannot both enter prepare or
   spawn for the same endpoint; one wins and the other fails with
   `PORT_CONFLICT`.
3. After the owning service stops and releases its socket, the other root can
   acquire the startup lock and start successfully.
4. A `persistent-until-down` service remains protected by its socket after the
   starting runtime exits; its child did not inherit the startup lock, a second
   runtime can acquire that lock, and the second runtime still detects the
   occupied socket.
5. Runtime death while holding a startup lock releases the kernel lock so a later
   attempt is not blocked by the lock file's existence.
6. An external listener produces `PORT_CONFLICT` before prepare/spawn and omits
   `nixfiedOwner` unless a Nixfied owner is independently proven.
7. Same-registry conflicts provide the complete `nixfiedOwner` object when the
   required listener/process/registry proof succeeds; all other conflicts omit
   it, and both JSON forms are snapshot-tested.
8. A synchronized external bind after preflight is reclassified as
   `PORT_CONFLICT` when a different owner remains observable; no child stderr is
   parsed to do so.
9. A proven collision never reports `PROC_ESCAPE`.
10. An unrelated early child exit without a proven endpoint conflict still
    reports the existing process/lifecycle error, and an unoccupied service that
    never becomes ready still reports `READINESS_TIMEOUT`.
11. Multi-endpoint services acquire locks deterministically; lock/preflight
    failure runs no prepare or child, readiness is admitted only when the child
    owns every endpoint, and a post-spawn partial-ownership failure terminates
    and reconciles the runtime-owned process group without reporting readiness.
12. Unsafe lock-root ownership, permissions, symlink components, and non-regular
    lock files produce `PORT_UNVERIFIABLE` before service lifecycle mutation.
13. Lock paths and public error JSON contain no injected secret, environment
    value, or child command line.
14. Different non-conflicting endpoints and different slots continue to start
    independently.
15. A local reuse/replacement request for a recorded process that is live but has
    a missing or proven-wrong listener terminates and confirms the owned group,
    records the terminal/stale transitions, and only then permits replacement.
    Failed termination preserves reservations and returns `PROC_ESCAPE`. A
    standalone `ps` does not signal that process and reports `ps.live = true`.
16. Unverifiable listener inspection preserves the live process and
    reservations, returns `PORT_UNVERIFIABLE`, blocks replacement, and still
    permits explicit `down` of the proven-owned process.
17. Prepare, spawn, readiness, cancellation, and partial multi-lock failures all
    release every startup lock.
18. A temporary preflight bind failure with no observed conflicting `LISTEN`
    socket, including a synthetic residual-kernel-state case, produces
    `PORT_UNVERIFIABLE` rather than a guessed owner or `PORT_CONFLICT`.
19. The fixed Linux/macOS coordination roots, directory/file modes, no-follow
    opens, nonblocking `flock`, close-on-exec behavior, and no-unlink rule are
    exercised directly.
20. Linux and macOS implementations provide equivalent contract behavior or
    return `PORT_UNVERIFIABLE` before process start when the required primitive
    is unsupported.
21. A candidate window too small for any statically selectable plan fails Nix
    evaluation; the equivalent hand-authored/dev model fails runtime admission
    with `MODEL_ADMISSION`, never `PORT_CONFLICT`.
22. The listener observer matrix covers an exact IPv4 listener, an IPv4 wildcard
    against arbitrary `127/8`, an exact IPv6 listener, and an IPv6 wildcard
    against both `::1` and IPv4 with `IPV6_V6ONLY` explicitly enabled/disabled;
    expected conflict follows the host bind semantics, while `TIME_WAIT` never
    counts as ownership.
23. Linux runtimes in the same network namespace derive the same network-scope
    key, while runtimes sharing the lock filesystem but using distinct network
    namespaces derive different keys; macOS derives the documented `host` scope.
24. Runtime death after local port reservation but before process recording
    leaves the next attempt with `LEASE_CONFLICT` and no prepare/spawn; after the
    lease expires, ordinary reconciliation releases it and a later start can
    proceed. This path never emits `PORT_CONFLICT`.

### Regression floor

The normal repository floor remains mandatory:

```sh
nix run .#check
nix run .#test
nix run .#gate
nix build .#nixfied-runtime
```

The implementation MUST also rerun downstream MFM `.#check`, `.#test`,
`.#test-db`, and `.#ci` after MFM adopts a project-owned verification-target
convention.

## Breaking adoption plan

This is a coordinated breaking adoption, not a data or compatibility migration.

### Phase 1: framework contract cut

In one architectural slice:

1. delete `cacheEnv` from the Nix and Rust contracts;
2. delete all obsolete implementation, output, examples, tests, and docs;
3. update the capability descriptor and its snapshot, thereby rotating the exact
   runtime ABI without changing unrelated version constants;
4. implement endpoint startup locking and conflict classification;
5. add the full acceptance floor; and
6. update `AGENTS.md`, `docs/ARCHITECTURE.md`, `docs/ADAPTERS.md`, and `README.md`
   to record the new negative cache boundary and port ownership phases.

The implementation MUST not leave the tree in an intermediate state where Nix
emits a field Rust ignores or Rust accepts a field Nix no longer declares.

### Phase 2: downstream MFM adoption

MFM will:

1. update its Nixfied pin to the new exact contract;
2. delete `verificationCachePolicy`, `cargoTargetCache`, and
   `cacheEnv.CARGO_TARGET_DIR` from its `cargoLeaf` construction;
3. declare its chosen verification target through ordinary project-owned
   invocation environment or tool configuration;
4. own worktree isolation, writer behavior, inspection, retention, cleanup,
   bypass, and corruption recovery for that target;
5. revise its Rust build ADR and capability handoff to reflect the corrected
   ownership boundary;
6. rerun the exact verification and coverage floor; and
7. remeasure performance if the chosen target placement differs from the
   qualified path.

The framework RFC does not prescribe MFM's target directory name or retention
policy. Those are deliberately downstream choices.

### Old state

The new runtime does not inspect, adopt, migrate, or specially delete old
`cacheEnv` directories.

Before changing pins, an adopter that wants to reclaim old slot state MAY use the
old exact runtime's supported `down`/`clean` flow after preserving required
evidence and confirming no owned processes remain. Alternatively, it may abandon
that disposable state and remove it through an explicit operator procedure.

No old-layout knowledge is carried into the new runtime.

## Consequences

### Positive

- Fewer model fields, public types, enums, validation paths, runtime types,
  output fields, tests, and docs.
- No cache registry, cache state machine, retention planner, GC policy, or new
  control surface.
- A sharper boundary between framework execution correctness and child-tool
  performance policy.
- No Cargo-specific pressure on the generic runtime.
- Host-global endpoint contention is reported through the existing correct error
  class before service mutation when observable.
- Persistent services rely on the OS socket they already own, not on an extra
  inherited lock protocol.
- Service reuse no longer mistakes a live but non-listening process for an
  addressable service, while `ps` retains its process-liveness scope.
- Static candidate-window infeasibility is reported at model admission rather
  than overloaded as a host port collision.
- The per-slot SQLite registry remains the only durable runtime registry.

### Negative

- Adopters that want durable tool caches must choose and operate them themselves.
- Nixfied no longer reports cache paths or identities in task summaries.
- Nixfied provides no cache-only cleanup or automatic disk bound.
- MFM must revise an accepted ADR and take responsibility for its verification
  target lifecycle.
- Existing models, output consumers, registries, and state markers are
  intentionally incompatible with the new exact runtime ABI.
- A reuse/replacement request terminates a runtime-owned process that has
  demonstrably lost a declared listener; unverifiable inspection blocks reuse
  and replacement until the process is explicitly stopped.
- The endpoint startup lock cannot atomically exclude arbitrary external
  processes that do not participate in the protocol.
- One tiny inert rendezvous file remains for each normalized endpoint the user
  has attempted; runtime cleanup deliberately does not unlink those files.

These costs are explicit consequences of preserving framework scope rather than
hidden missing features.

## Risks and mitigations

### Adopters mistake ordinary `${stateDir}` files for framework-managed caches

Mitigation: documentation states that `${stateDir}` is broad slot state only.
There is no cache vocabulary, identity, output, or targeted cleanup to imply a
stronger contract.

### Project-owned caches grow without bound

Mitigation: the project/tool owns that policy, just as it owns the cache contents
and compatibility. Nixfied does not present growth as framework state.

### Lock files become stale

Mitigation: file existence and contents have no ownership meaning. Kernel lock
state is authoritative, so an unlocked stale file is harmless. The files are
small rendezvous inodes and are intentionally not garbage-collected because
unlinking a live lock can split coordination.

### Listener ownership cannot be inspected

Mitigation: fail closed with `PORT_UNVERIFIABLE`, preserve any already-live
owned process and its reservations, and retain explicit `down`. Do not guess an
owner or start a replacement.

### A service briefly closes a declared listener during internal restart

Mitigation: the declared endpoint is a continuous addressability claim after
readiness. At a reuse/replacement attempt, a proven missing listener is a real
contract violation; the runtime terminates the owned group before releasing its
reservation rather than allowing a local rebind race. A cross-state-root gap is
the documented external race and is not hidden by `ps` supervision.

### Deadlock across multi-endpoint services

Mitigation: every runtime derives and acquires normalized keys in the same byte
order and releases the full partial set on failure.

### Runtime dies during service startup

Mitigation: the kernel releases its startup locks. Existing process-group and
registry reconciliation remains responsible for known children. The RFC does not
claim atomic reservation against an unbound surviving child.

### External process races after availability check

Mitigation: ownership verification remains mandatory. A proven external owner is
`PORT_CONFLICT`; ambiguous failures remain truthful lifecycle/readiness errors.
Socket activation is explicitly rejected rather than approximated.

## Rollback

There is no in-place compatibility rollback.

Code rollback means repinning the exact previous Nixfied revision and using its
matching model/runtime ABI. State written under one exact contract is not assumed
compatible with the other. Because cache contents are accelerators rather than
authoritative artifacts, losing or abandoning them does not change task success
semantics.

Git history remains the recovery mechanism for deleted implementation. The new
tree does not retain dormant old code for rollback convenience.

## Final decision boundary

This RFC accepts exactly two conclusions from the MFM review:

1. The existing cache abstraction points Nixfied toward responsibilities it
   should not own, so the abstraction is deleted rather than completed.
2. Cross-state-root endpoint collisions are part of Nixfied's existing service
   execution correctness, so startup is serialized transiently and proven
   conflicts use `PORT_CONFLICT`.

Everything else remains on its proper side of the boundary: Nix authors the
model, Rust owns generic impure execution, the OS owns live socket truth, and the
child/project owns tool acceleration.
