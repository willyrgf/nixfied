# RFC: Keep caches adopter-owned and coordinate endpoint startup on the host

- Status: implemented
- Date: 2026-07-18
- Scope: cache responsibility and TCP service endpoint ownership
- Origin: MFM verification-cache capability review

## Decision

Nixfied has no cache primitive. Tool acceleration is child/tool/project-owned
and may be configured only through ordinary invocation environment or
arguments. Nixfied does not identify, place, create, lock, report, retain, or
selectively clean cache artifacts.

Endpoint-bearing services remain a Nixfied runtime responsibility. Their
startup is serialized across independent state roots by a transient,
endpoint-keyed host lock. Readiness and reuse require exact kernel-observed
socket ownership by the recorded containment. The lock is released after the
atomic ready transition; the listening socket is steady-state ownership.

These decisions preserve the product boundary: Nix authors and validates one
model, Rust owns generic impure execution, the OS is the liveness authority,
and children own domain-specific acceleration.

The terms **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative below.

## Why the cache abstraction was deleted

MFM uses a persistent Cargo target for broad verification. Its measurements
made the tradeoff concrete: a stable target was about 10.49 GiB, warm
verification improved by roughly 45 percent in the qualifying sample, and
profile/target changes grew one identity to about 16.92 GiB. Independent
worktrees also accumulated path-specific artifacts, and concurrent runs could
write the same target.

The former `invocation.cacheEnv` contract gave Nixfied cache-specific types,
identity, placement, directory creation, and output evidence without giving it
writer arbitration, accounting, retention, recovery, or targeted cleanup.
Completing that contract would require a mutable-cache resource manager:
durable identities and leases, active-use reconciliation, size and age
accounting, eviction policy, deletion recovery, inspection, and operator
commands. None of those capabilities is required to decide whether a task
executed successfully.

The partial abstraction was therefore removed instead of expanded:

- `Invocation` has ordinary `env` and arguments, with no cache sub-language.
- A project MAY set a tool variable such as `CARGO_TARGET_DIR`; Nixfied does not
  interpret it.
- A worktree-relative artifact is wholly project-owned and is outside Nixfied
  cleanup.
- A child-written path derived from `${stateDir}` is ordinary slot state and is
  subject only to the existing whole-state cleanup policy. It does not gain a
  cache identity or cache-specific lifecycle.
- Tasks always execute. Existing files never synthesize success or runtime
  evidence.
- Old `cacheEnv` model data is rejected as unknown. There is no compatibility
  parser, alias, migration, or old-layout cleanup path.

This is the CACHE-1 boundary: cache is not a semantic kind or runtime resource.

## The host-global endpoint problem

The model deterministically assigns service endpoints from a slot's port
window, while each state root has its own per-slot SQLite registry. TCP
endpoints are not state-root-local. Two runtimes with independent registries
can select the same address and port, both believe their local reservation is
free, and collide only after prepare or spawn.

Nixfied already claims responsibility for service placement, process ownership,
readiness, and stable endpoint errors. It therefore MUST coordinate cooperating
runtimes at the host resource, without turning the coordination mechanism into
a second durable registry.

## Endpoint authority

Four authorities have distinct scopes:

| Fact | Authority |
| --- | --- |
| Static endpoint demand and deterministic placement | Nix-authored model and runtime admission |
| Durable runs, leases, processes, reservations, and ordered events | Per-slot SQLite registry |
| Process and listener liveness/ownership | Operating system |
| Startup serialization across independent roots | Transient host endpoint lock |

The lock file and its contents are never semantic state, liveness evidence, or
owner attribution. File existence is inert; kernel lock ownership is the only
coordination fact. The registry remains the sole durable runtime registry and
is evidence rather than an OS liveness oracle.

## Endpoint lock contract

For every declared TCP endpoint, the runtime derives a lock identity from the
resource that can collide: transport, network scope, canonical address family,
canonical loopback address, and numeric port. Project, model, service, endpoint
id, state-root, and runtime ABI identities MUST NOT affect that key.

The coordination root is runtime-selected, per effective user, independent of
`NIXFIED_STATE_DIR`, and shared by runtimes in the same relevant network and
mount scope. On Linux the key includes the current network namespace identity;
on macOS the network scope is the host. Unsupported or unsafe scope discovery
fails closed as `PORT_UNVERIFIABLE`.

The runtime MUST:

- anchor the root under the platform's fixed system temporary directory;
- create private, effective-user-owned directories and lock files;
- reject unsafe ownership, permissions, symlinks, and non-regular targets;
- derive filenames from a fixed digest, never adopter-controlled path text;
- open lock files without following symlinks and with close-on-exec enabled;
- take exclusive locks non-blockingly;
- acquire a multi-endpoint service's locks in deterministic order and release a
  partial set on failure;
- never pass lock descriptors to prepare, service, probe, task, or stop
  children; and
- never unlink or replace rendezvous files, including during `clean`, because
  doing so could split coordination across inodes.

Lock contention proves only that a cooperating startup holds the endpoint's
coordination lock. It does not prove that runtime's project or service identity.

## Startup and readiness contract

Static endpoint demand MUST fit the selected port window. Nix rejects an
over-capacity compiled model; runtime admission rejects an equivalent
hand-authored development model. Capacity failure is `MODEL_ADMISSION`, not a
host port conflict.

For an endpoint-bearing service, the runtime follows this contract:

1. Attempt exact healthy reuse using current registry, process, containment,
   lease, and socket evidence.
2. If reuse does not commit, acquire every endpoint lock before any
   service-specific reservation, prepare, spawn, or readiness mutation.
3. Under the locks, reconcile ordinary stale evidence and retry exact reuse
   once.
4. Preflight each endpoint with an exact temporary bind and a complete kernel
   listener snapshot. The temporary socket is closed and is never listened on
   or handed to the child.
5. Reserve the service and all endpoints transactionally in the selected
   per-slot registry.
6. Run prepare, spawn the service in runtime-owned containment, and durably
   record its exact process identity before treating it as started.
7. Apply the declared readiness budget. An attempt succeeds only when its probe
   succeeds, every declared endpoint has an exact listener, every conflicting
   socket record correlates to the tracked containment, and no conflicting
   record has a proven outside holder.
8. Mark the process ready and every endpoint active in one transaction, then
   release all endpoint locks.

Every error and cancellation path MUST release its locks. If a child exists,
its normal containment and exact settlement rules apply before reservations can
be released. A failure to prove termination preserves actionable escape
evidence and protected ports; it is never converted into a successful cleanup.

Wildcard and dual-stack listeners participate in conflict detection according
to host socket semantics, but a wildcard listener never satisfies an exact
declared endpoint. `TIME_WAIT`, a registry row, lock-file contents, or a failed
temporary bind alone is not listener ownership.

Linux listener truth comes from `SOCK_DIAG`; macOS listener truth comes from the
TCP PCB list. Process and descriptor enumeration correlates each observed
socket to live contained holders. Missing, truncated, unstable, or unsupported
material evidence fails closed.

## Reuse and replacement

Exact reuse is deliberately stronger than process liveness. It requires:

- the full SVC-ID-1 service, state, runtime, target, address, and endpoint
  identity match;
- a ready primary process with an exact live start-identity match;
- the complete active endpoint set;
- exact current listener satisfaction for every declared endpoint;
- containment proof for every conflicting socket record; and
- an atomic guarded borrower transaction against the same observed evidence.

Owner and borrower leases remain authoritative. Exact healthy reuse may add a
borrower. If reuse cannot be proven, an open owner or borrower lease blocks
replacement as `LEASE_CONFLICT`.

Endpoint acquisition and reconciliation MUST NOT signal a pre-existing service
to resolve a collision, lost listener, or ownership mismatch. A live service
that is not exactly reusable remains recorded and actionable. Missing or
unprovable ownership is `PORT_UNVERIFIABLE`; a proven outside listener is
`PORT_CONFLICT`. Replacement requires explicit `down`, which independently
proves and terminates runtime-owned containment, followed by a later start.

An expired process-less reservation is not a live service. Ordinary lease
reconciliation may stale that reservation transactionally after its lease loses
authority. While the lease remains open, startup returns `LEASE_CONFLICT` and
performs no prepare or spawn.

Persistent services do not inherit the startup lock. After readiness, a
`persistent-until-down` child may outlive the runtime that started it; its
listening socket protects the endpoint. Another root can acquire the now-free
startup lock but observes the socket and fails closed. When explicit `down`
closes the socket, a later start may proceed.

`ps` reports reconciled process liveness. It does not redefine liveness as
addressability and does not signal a process because a listener is missing.

## Endpoint-less services

An endpoint-less service makes no addressability claim and takes no endpoint
lock. Nothing may use endpoint placeholders toward it, its readiness and health
probes must be invocations, and its start closure must not attest
`network-listener`. It still receives the same process containment, leases,
registry evidence, and cleanup safety as any other service.

The process-less reservation expiry rule applies equally to an endpoint-less
service, simply without port rows.

## Errors and attribution

The stable classifications are:

- `PORT_CONFLICT`: a startup lock is contended, kernel observation proves a
  conflicting listener during preflight, or post-spawn observation proves a
  distinct conflicting listener outside the tracked containment;
- `PORT_UNVERIFIABLE`: safe coordination or the required socket/ownership proof
  cannot be established;
- `LEASE_CONFLICT`: a non-reusable local reservation, owner, or borrower still
  has an authoritative open lease;
- `PROC_ESCAPE`: the child violated or could not be proven absent from its
  required containment; and
- `READINESS_TIMEOUT` or `LIFECYCLE_FAILED`: startup did not become ready and no
  stronger typed endpoint or containment cause was proven.

The runtime does not parse child stderr to infer a port conflict. A bind failure
without an observed conflicting listener is `PORT_UNVERIFIABLE`, not a guessed
owner.

`PORT_CONFLICT` details identify the request and exact endpoint. They MAY
include `nixfiedOwner` with project, environment, slot, run, service instance,
and process identities only when one registry record, an exact live process
start identity, containment, and every observed socket record prove the same
owner. Lock contention alone, an open port, a PID, a command line, or stale
registry evidence is insufficient. Cross-state-root conflicts normally omit
owner attribution because there is no shared durable registry.

Diagnostics MUST NOT expose child environment values, credentials, secret
material, or arbitrary command lines.

## Residual external race

The startup lock coordinates participating Nixfied runtimes for the same
effective user and relevant namespace scope. An unrelated process, another
user, or an uncoordinated old runtime can bind after preflight and before the
child. Ownership verification still catches a competing listener when it is
observable and reports `PORT_CONFLICT`; incomplete proof fails closed.

The runtime also releases locks automatically if it dies during startup. A
surviving child may bind after that death. A later invocation can observe an
already-bound socket but cannot atomically exclude an unbound survivor.

Eliminating these windows would require socket activation, a file-descriptor
handoff, a lifetime lock, or a guardian process. This RFC intentionally does
not claim atomic reservation against arbitrary external processes.

## Rejected alternatives

### Complete or retain `cacheEnv`

Rejected. Completing it creates a cache manager outside task execution
correctness; retaining it as placement-only preserves the misleading partial
ownership contract. Adding only a worktree key, writer lock, or cache-only
clean is the same expansion in smaller increments.

### Add a host-global registry or daemon

Rejected. It would introduce another durable authority, corruption and
reconciliation domain, cleanup surface, and process lifecycle. The per-slot
registry plus transient host lock and kernel socket have sufficient, disjoint
authority.

### Hold the lock for the service lifetime

Rejected. Persistent services intentionally outlive the invoking runtime. An
inherited lock or guardian would create a new supervision protocol and duplicate
the socket's ownership role.

### Add socket activation

Rejected. Passing a bound descriptor to services would widen the generic
invocation and adapter contracts. Nixfied requires fail-closed ownership proof,
not a new service participation protocol.

### Select another port automatically

Rejected. Placement is deterministic and fail-only. Fallback would silently
change endpoint and service reuse identity.

## Consequences

The model, runtime, generated views, output, and documentation are smaller and
carry no cache lifecycle promise. Adopters must choose, isolate, inspect,
retain, and clean their tool artifacts themselves. Nixfied provides no cache
quota or cache-only cleanup.

Independent state roots now serialize cooperating starts before service
mutation, exact ownership gates readiness and reuse, and proven collisions have
truthful typed diagnostics. The SQLite registry remains the only durable
runtime authority; tiny inert lock rendezvous files are intentionally not
garbage-collected.

The cost of the narrow design is explicit: arbitrary external binders remain a
race, ambiguous host evidence blocks progress, and a live non-reusable service
requires explicit operator teardown. Those limits preserve the single-model
seam, generic runtime, fail-closed ownership, and no-daemon architecture.
